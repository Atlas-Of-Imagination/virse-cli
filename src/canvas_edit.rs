use crate::{print, storage::Store, workflow_state as state};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};
use uuid::Uuid;
use virse_api::{CanvasCommand, CanvasTransaction, Client, EntityKind, Error, Result};

#[derive(Args)]
pub struct WriteOptions {
    #[arg(long)]
    pub idempotency_key: Option<String>,
    /// Optional server revision map from a previous read, e.g. {"layout":2}.
    #[arg(long)]
    pub revisions: Option<String>,
}
#[derive(Subcommand)]
pub enum EntityCommand {
    List {
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long,default_value_t=100,value_parser=clap::value_parser!(u16).range(1..=200))]
        limit: u16,
    },
    Get {
        id: Uuid,
    },
    /// JSON containing the existing backend element/group/edge creation fields.
    Create {
        #[arg(long)]
        file: PathBuf,
        #[command(flatten)]
        options: WriteOptions,
    },
    /// JSON patch; revision checks are mandatory and can be supplied explicitly.
    Update {
        id: Uuid,
        #[arg(long)]
        file: PathBuf,
        #[command(flatten)]
        options: WriteOptions,
    },
    Delete {
        id: Uuid,
        #[command(flatten)]
        options: WriteOptions,
    },
    Name {
        id: Uuid,
        name: String,
        #[command(flatten)]
        options: WriteOptions,
    },
}
#[derive(Clone, Copy, ValueEnum)]
pub enum LayoutMode {
    Row,
    Column,
    Grid,
}
#[derive(Subcommand)]
pub enum Deleted {
    List,
    Restore {
        id: Uuid,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
}
#[derive(Subcommand)]
pub enum CanvasExtra {
    #[command(subcommand)]
    Element(EntityCommand),
    #[command(subcommand)]
    Group(EntityCommand),
    #[command(subcommand)]
    Edge(EntityCommand),
    /// Apply an atomic V2 transaction from JSON with explicit operation revisions.
    Transaction {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Arrange selected elements, preserving content and checking their layout revisions.
    Layout {
        #[arg(long, required_unless_present = "group", conflicts_with = "group")]
        element: Vec<Uuid>,
        #[arg(long)]
        group: Option<Uuid>,
        #[arg(long, value_enum, default_value = "grid")]
        mode: LayoutMode,
        #[arg(long, default_value_t = 40.0)]
        gap: f64,
        #[arg(long)]
        columns: Option<u16>,
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        x: f64,
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        y: f64,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Copy selected nodes and internal edges, optionally with their group container.
    Copy {
        #[arg(long, required_unless_present = "group", conflicts_with = "group")]
        element: Vec<Uuid>,
        #[arg(long)]
        group: Option<Uuid>,
        #[arg(long)]
        to_canvas: Option<Uuid>,
        #[arg(long, default_value_t = 80.0, allow_hyphen_values = true)]
        dx: f64,
        #[arg(long, default_value_t = 80.0, allow_hyphen_values = true)]
        dy: f64,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    #[command(subcommand)]
    Deleted(Deleted),
    /// Package selected or grouped byte assets; unsupported inline nodes are reported in the manifest.
    Pack {
        #[arg(long, required_unless_present = "group", conflicts_with = "group")]
        element: Vec<Uuid>,
        #[arg(long)]
        group: Option<Uuid>,
        #[arg(long)]
        output: PathBuf,
    },
}
pub struct Context<'a> {
    pub client: &'a Client,
    pub store: &'a Store,
    pub backend: &'a str,
    pub org: Uuid,
    pub canvas: Uuid,
    pub compact: bool,
}
fn invalid(s: &str) -> Error {
    Error::new("INVALID_INPUT", s)
}
fn explicit_revisions(value: &Option<String>) -> Result<Option<BTreeMap<String, u64>>> {
    value
        .as_ref()
        .map(|s| {
            serde_json::from_str(s)
                .map_err(|_| invalid("Revisions must be a JSON object of nonnegative integers"))
        })
        .transpose()
}
async fn execute(ctx: &Context<'_>, key: &str, user: Uuid, value: Value) -> Result<u8> {
    let request: CanvasTransaction =
        serde_json::from_value(value).map_err(|_| invalid("Invalid saved transaction"))?;
    if request.org_id != ctx.org {
        return Err(invalid(
            "Transaction organization differs from selected --org",
        ));
    }
    let result=ctx.client.canvas_transaction(ctx.canvas,&request,key,user).await.map_err(|e|state::uncertain(e,"Canvas result unknown; repeat original command/key. Do not refresh revisions to force a write",Some(key)))?;
    print(&json!({"idempotency_key":key,"result":result}), ctx.compact)?;
    Ok(0)
}
async fn entity(command: &EntityCommand, kind: EntityKind, ctx: &Context<'_>) -> Result<u8> {
    match command {
        EntityCommand::List { offset, limit } => {
            return print(
                &ctx.client
                    .canvas_entities(ctx.canvas, ctx.org, kind, *offset, *limit)
                    .await?,
                ctx.compact,
            )
            .map(|_| 0);
        }
        EntityCommand::Get { id } => {
            return print(
                &ctx.client
                    .canvas_entity(ctx.canvas, ctx.org, kind, *id)
                    .await?,
                ctx.compact,
            )
            .map(|_| 0);
        }
        _ => {}
    }
    let (action, id, mut value, options) = match command {
        EntityCommand::Create { file, options } => {
            ("create", None, state::json_file(file).await?, options)
        }
        EntityCommand::Update { id, file, options } => {
            ("update", Some(*id), state::json_file(file).await?, options)
        }
        EntityCommand::Delete { id, options } => ("delete", Some(*id), json!({}), options),
        EntityCommand::Name { id, name, options } => {
            if name.trim().is_empty() || name.chars().count() > 255 {
                return Err(invalid("Name must contain 1..255 characters"));
            }
            let field = match kind {
                EntityKind::Element => "name",
                EntityKind::Group => "title",
                EntityKind::Edge => return Err(invalid("Edges do not have names")),
            };
            ("update", Some(*id), json!({field:name}), options)
        }
        _ => unreachable!(),
    };
    if !value.is_object()
        || value.get("org_id").is_some()
        || value.get("expected_revisions").is_some()
    {
        return Err(invalid(
            "File must be an object containing only entity fields; use --revisions for CAS",
        ));
    }
    if action != "create" && value.get("id").is_some() {
        return Err(invalid(
            "Entity ID is the command argument, not a patch field",
        ));
    }
    let explicit = explicit_revisions(&options.revisions)?;
    let key = state::key(&options.idempotency_key)?;
    let user = state::user(ctx.client).await?;
    let scope = json!([ctx.backend, user, ctx.org, ctx.canvas, "canvas-edit", key]);
    let intent = json!({"kind":kind,"action":action,"id":id,"value":value,"revisions":explicit});
    let body = match state::freeze(ctx.store, &scope, &intent, None)? {
        Some(v) => v,
        None => {
            let id = match id {
                Some(id) => id,
                None => match value.as_object_mut().unwrap().remove("id") {
                    Some(v) => {
                        serde_json::from_value(v).map_err(|_| invalid("Invalid new entity ID"))?
                    }
                    None => Uuid::new_v4(),
                },
            };
            let revisions = if action == "create" {
                BTreeMap::new()
            } else {
                match explicit {
                    Some(v) => v,
                    None => state::revisions(
                        &ctx.client
                            .canvas_entity(ctx.canvas, ctx.org, kind, id)
                            .await?,
                    )?,
                }
            };
            let request = CanvasTransaction {
                org_id: ctx.org,
                operations: vec![CanvasCommand {
                    action: format!("{}.{}", kind.singular(), action),
                    id,
                    value,
                    expected_revisions: revisions,
                }],
            };
            request.validate()?;
            state::freeze(ctx.store, &scope, &intent, Some(json!(request)))?.unwrap()
        }
    };
    execute(ctx, &key, user, body).await
}
pub async fn selected(
    ctx: &Context<'_>,
    ids: &[Uuid],
    group: Option<Uuid>,
) -> Result<(Vec<Uuid>, Option<Value>)> {
    selected_limit(ctx, ids, group, 100).await
}
pub async fn selected_limit(
    ctx: &Context<'_>,
    ids: &[Uuid],
    group: Option<Uuid>,
    limit: usize,
) -> Result<(Vec<Uuid>, Option<Value>)> {
    let value = match group {
        Some(id) => Some(
            ctx.client
                .canvas_entity(ctx.canvas, ctx.org, EntityKind::Group, id)
                .await?,
        ),
        None => None,
    };
    let ids = match &value {
        Some(g) => crate::resource_ops::group_elements(g)?,
        None => ids.to_vec(),
    };
    if ids.is_empty() || ids.len() > limit || ids.iter().collect::<HashSet<_>>().len() != ids.len()
    {
        return Err(invalid(
            "Choose distinct elements within the operation limit",
        ));
    }
    Ok((ids, value))
}
pub fn arrange(
    nodes: &[Value],
    mode: LayoutMode,
    gap: f64,
    columns: Option<u16>,
    x: f64,
    y: f64,
) -> Result<Vec<(f64, f64)>> {
    if !gap.is_finite() || gap < 0.0 || !x.is_finite() || !y.is_finite() || columns == Some(0) {
        return Err(invalid(
            "Layout coordinates must be finite, gap nonnegative and columns positive",
        ));
    }
    let columns = match mode {
        LayoutMode::Row => nodes.len(),
        LayoutMode::Column => 1,
        LayoutMode::Grid => columns
            .map(usize::from)
            .unwrap_or_else(|| (nodes.len() as f64).sqrt().ceil() as usize),
    }
    .max(1);
    let mut result = vec![];
    let (mut px, mut py, mut row_height) = (x, y, 0.0_f64);
    for (i, node) in nodes.iter().enumerate() {
        let w = node
            .pointer("/layout/size/width")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| invalid("Element has no usable width"))?;
        let h = node
            .pointer("/layout/size/height")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| invalid("Element has no usable height"))?;
        if i > 0 && i % columns == 0 {
            px = x;
            py += row_height + gap;
            row_height = 0.0;
        }
        if !px.is_finite() || !py.is_finite() {
            return Err(invalid("Layout overflow"));
        }
        result.push((px, py));
        px += w + gap;
        row_height = row_height.max(h);
    }
    Ok(result)
}
pub async fn run(command: &CanvasExtra, ctx: &Context<'_>) -> Result<u8> {
    match command {
        CanvasExtra::Element(c) => entity(c, EntityKind::Element, ctx).await,
        CanvasExtra::Group(c) => entity(c, EntityKind::Group, ctx).await,
        CanvasExtra::Edge(c) => entity(c, EntityKind::Edge, ctx).await,
        CanvasExtra::Transaction {
            file,
            idempotency_key,
        } => {
            let body = state::json_file(file).await?;
            let request: CanvasTransaction = serde_json::from_value(body.clone())
                .map_err(|_| invalid("Invalid canvas transaction"))?;
            request.validate()?;
            if request.org_id != ctx.org {
                return Err(invalid(
                    "Transaction org_id differs from selected organization",
                ));
            }
            let user = state::user(ctx.client).await?;
            let key = state::key(idempotency_key)?;
            let scope = json!([ctx.backend, user, ctx.org, ctx.canvas, "canvas-edit", key]);
            let body = state::freeze(ctx.store, &scope, &body, Some(body.clone()))?.unwrap();
            execute(ctx, &key, user, body).await
        }
        CanvasExtra::Layout {
            element,
            group,
            mode,
            gap,
            columns,
            x,
            y,
            idempotency_key,
        } => {
            let key = state::key(idempotency_key)?;
            let user = state::user(ctx.client).await?;
            let scope = json!([ctx.backend, user, ctx.org, ctx.canvas, "canvas-layout", key]);
            let mode_name = match mode {
                LayoutMode::Row => "row",
                LayoutMode::Column => "column",
                LayoutMode::Grid => "grid",
            };
            let intent = json!({"ids":element,"group":group,"mode":mode_name,"gap":gap,"columns":columns,"x":x,"y":y});
            let body = match state::freeze(ctx.store, &scope, &intent, None)? {
                Some(v) => v,
                None => {
                    let (ids, group_value) = selected(ctx, element, *group).await?;
                    if group_value.is_some() && ids.len() >= 100 {
                        return Err(invalid(
                            "Group layout supports at most 99 members in one transaction",
                        ));
                    }
                    let mut nodes = vec![];
                    for id in &ids {
                        nodes.push(
                            ctx.client
                                .canvas_entity(ctx.canvas, ctx.org, EntityKind::Element, *id)
                                .await?,
                        );
                    }
                    let positions = arrange(&nodes, *mode, *gap, *columns, *x, *y)?;
                    let width = nodes
                        .iter()
                        .zip(&positions)
                        .map(|(n, (px, _))| px + n["layout"]["size"]["width"].as_f64().unwrap() - x)
                        .fold(0.0_f64, f64::max);
                    let height = nodes
                        .iter()
                        .zip(&positions)
                        .map(|(n, (_, py))| {
                            py + n["layout"]["size"]["height"].as_f64().unwrap() - y
                        })
                        .fold(0.0_f64, f64::max);
                    let mut operations = ids
                        .into_iter()
                        .zip(nodes)
                        .zip(positions)
                        .map(|((id, node), (x, y))| {
                            Ok(CanvasCommand {
                                action: "element.update".into(),
                                id,
                                value: json!({"layout":{"position":{"x":x,"y":y}}}),
                                expected_revisions: state::revisions(&node)?,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if let (Some(id), Some(g)) = (group, group_value) {
                        operations.push(CanvasCommand{action:"group.update".into(),id:*id,value:json!({"position":{"x":x,"y":y},"size":{"width":width,"height":height}}),expected_revisions:state::revisions(&g)?});
                    }
                    state::freeze(
                        ctx.store,
                        &scope,
                        &intent,
                        Some(json!(CanvasTransaction {
                            org_id: ctx.org,
                            operations
                        })),
                    )?
                    .unwrap()
                }
            };
            execute(ctx, &key, user, body).await
        }
        CanvasExtra::Deleted(Deleted::List) => {
            print(&ctx.client.recently_deleted(ctx.canvas).await?, ctx.compact).map(|_| 0)
        }
        CanvasExtra::Deleted(Deleted::Restore {
            id,
            idempotency_key,
        }) => {
            let key = state::key(idempotency_key)?;
            let user = state::user(ctx.client).await?;
            let scope = json!([ctx.backend, user, ctx.canvas, "restore-media", key]);
            let intent = json!({"id":id});
            state::freeze(ctx.store, &scope, &intent, Some(intent.clone()))?;
            let result = ctx
                .client
                .restore_canvas_media(ctx.canvas, *id, &key)
                .await
                .map_err(|e| {
                    state::uncertain(
                        e,
                        "Media restore result unknown; repeat original ID/key",
                        Some(&key),
                    )
                })?;
            print(&json!({"idempotency_key":key,"result":result}), ctx.compact)?;
            Ok(0)
        }
        CanvasExtra::Copy {
            element,
            group,
            to_canvas,
            dx,
            dy,
            idempotency_key,
        } => {
            copy(
                ctx,
                element,
                *group,
                to_canvas.unwrap_or(ctx.canvas),
                (*dx, *dy),
                idempotency_key,
            )
            .await
        }
        CanvasExtra::Pack {
            element,
            group,
            output,
        } => crate::delivery::pack_canvas(ctx, element, *group, output).await,
    }
}
fn remap(value: &mut Value, ids: &HashMap<String, String>) {
    match value {
        Value::String(v) => {
            if let Some(next) = ids.get(v) {
                *v = next.clone();
            }
        }
        Value::Array(rows) => {
            for v in rows {
                remap(v, ids)
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                remap(v, ids)
            }
        }
        _ => {}
    }
}
async fn copy(
    ctx: &Context<'_>,
    elements: &[Uuid],
    group: Option<Uuid>,
    target: Uuid,
    offset: (f64, f64),
    given: &Option<String>,
) -> Result<u8> {
    if !offset.0.is_finite() || !offset.1.is_finite() {
        return Err(invalid("Copy offset must be finite"));
    }
    let key = state::key(given)?;
    let user = state::user(ctx.client).await?;
    let scope = json!([
        ctx.backend,
        user,
        ctx.org,
        ctx.canvas,
        target,
        "canvas-copy",
        key
    ]);
    let intent = json!({"elements":elements,"group":group,"offset":offset});
    let plan = match state::freeze(ctx.store, &scope, &intent, None)? {
        Some(v) => v,
        None => {
            let (ids, group_value) = selected(ctx, elements, group).await?;
            let mut mapping = HashMap::new();
            for id in &ids {
                mapping.insert(id.to_string(), Uuid::new_v4().to_string());
            }
            if let Some(id) = group {
                mapping.insert(id.to_string(), Uuid::new_v4().to_string());
            }
            let mut nodes = vec![];
            for id in &ids {
                let node = ctx
                    .client
                    .canvas_entity(ctx.canvas, ctx.org, EntityKind::Element, *id)
                    .await?;
                if node["type"] == "markspace"
                    || node.pointer("/expanded/markspace_document").is_some()
                {
                    return Err(invalid(
                        "Copy of an editable MarkSpace document is not supported by this paste protocol; export its saved image first",
                    ));
                }
                let mut value = json!({"element_id":mapping[&id.to_string()],"element_type":node["type"],"position":node["layout"]["position"],"size":node["layout"]["size"],"rotation":node["layout"]["rotation"],"z_index":node["layout"]["z_index"],"name":node["name"],"style":node["attributes"]["style"],"text":node["attributes"]["text"],"text_rich":node["attributes"]["text_rich"],"skill_id":node["attributes"]["skill_id"],"generation_config":node["generation"]["config"],"artifact_id":node["generation"]["artifact_id"],"artifact_version_id":node["generation"]["artifact_version_id"]});
                if node["content_ref"]["kind"] == "asset" {
                    value["asset_id"] = node["content_ref"]["id"].clone();
                }
                if node["content_ref"]["kind"] == "aesthetic" {
                    value["style_id"] = node["content_ref"]["id"].clone();
                }
                if node["type"] == "agent" {
                    value["source_block_id"] = json!(id);
                }
                let x = node
                    .pointer("/layout/position/x")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| invalid("Missing source position"))?
                    + offset.0;
                let y = node
                    .pointer("/layout/position/y")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| invalid("Missing source position"))?
                    + offset.1;
                if !x.is_finite() || !y.is_finite() {
                    return Err(invalid("Copy offset overflow"));
                }
                value["position"] = json!({"x":x,"y":y});
                remap(&mut value["generation_config"], &mapping);
                nodes.push(value);
            }
            let mut groups = vec![];
            if let Some(original) = group_value {
                let id = group.unwrap();
                let mut value =
                    json!({"group_id":mapping[&id.to_string()],"elements":original["elements"]});
                for field in [
                    "title",
                    "kind",
                    "style_id",
                    "workflow_id",
                    "layout_mode",
                    "layout_config",
                    "bounds_mode",
                    "artifact_id",
                    "artifact_version_id",
                ] {
                    if let Some(v) = original.get(field) {
                        value[field] = v.clone();
                    }
                }
                let gx = original["position_x"]
                    .as_f64()
                    .or_else(|| original["position"]["x"].as_f64())
                    .unwrap_or(0.0)
                    + offset.0;
                let gy = original["position_y"]
                    .as_f64()
                    .or_else(|| original["position"]["y"].as_f64())
                    .unwrap_or(0.0)
                    + offset.1;
                let gw = original["size_width"]
                    .as_f64()
                    .or_else(|| original["size"]["width"].as_f64())
                    .unwrap_or(1.0);
                let gh = original["size_height"]
                    .as_f64()
                    .or_else(|| original["size"]["height"].as_f64())
                    .unwrap_or(1.0);
                if ![gx, gy, gw, gh].iter().all(|v| v.is_finite()) {
                    return Err(invalid("Group bounds are not finite"));
                }
                value["position"] = json!({"x":gx,"y":gy});
                value["size"] = json!({"width":gw,"height":gh});
                remap(&mut value["elements"], &mapping);
                groups.push(value);
            }
            let mut edges = vec![];
            let mut page = 0;
            loop {
                let result = ctx
                    .client
                    .canvas_entities(ctx.canvas, ctx.org, EntityKind::Edge, page, 200)
                    .await?;
                let rows = result
                    .get("items")
                    .and_then(Value::as_array)
                    .ok_or_else(|| Error::new("INVALID_RESPONSE", "Missing edge page"))?;
                for row in rows {
                    let source = row["source_element_id"]
                        .as_str()
                        .or_else(|| row["source_group_id"].as_str());
                    let dest = row["target_element_id"]
                        .as_str()
                        .or_else(|| row["target_group_id"].as_str());
                    if source.is_some_and(|v| mapping.contains_key(v))
                        && dest.is_some_and(|v| mapping.contains_key(v))
                    {
                        let mut edge = row.clone();
                        remap(&mut edge, &mapping);
                        edge["edge_id"] = json!(Uuid::new_v4());
                        if let Some(obj) = edge.as_object_mut() {
                            for field in
                                ["id", "project_id", "revisions", "created_at", "modified_at"]
                            {
                                obj.remove(field);
                            }
                        }
                        edges.push(edge);
                    }
                }
                if rows.len() < 200 || result.get("has_more") == Some(&Value::Bool(false)) {
                    break;
                }
                page += 200;
                if page > 10000 {
                    return Err(invalid(
                        "Canvas edge scan exceeds limit; use a smaller source canvas",
                    ));
                }
            }
            state::freeze(ctx.store,&scope,&intent,Some(json!({"source_workspace_id":ctx.canvas,"external_dependency_policy":"skip","elements":nodes,"groups":groups,"edges":edges})))?.unwrap()
        }
    };
    let result = ctx
        .client
        .canvas_paste(target, &plan, &key)
        .await
        .map_err(|e| {
            state::uncertain(
                e,
                "Copy result unknown; repeat the same selection and key",
                Some(&key),
            )
        })?;
    print(&json!({"idempotency_key":key,"result":result}), ctx.compact)?;
    Ok(0)
}
