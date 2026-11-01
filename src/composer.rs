use crate::{canvas_edit::Context, print, workflow_state as state};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
use uuid::Uuid;
use virse_api::{CanvasCommand, CanvasTransaction, ComposerSocket, EntityKind, Error, Result};
#[derive(Args)]
pub struct Wait {
    #[arg(long)]
    wait: bool,
    #[arg(long,default_value_t=600,value_parser=clap::value_parser!(u64).range(1..))]
    timeout: u64,
}
#[derive(Args)]
pub struct Send {
    id: Uuid,
    #[arg(long,required_unless_present_any=["input_file","request_file"],conflicts_with_all=["input_file","request_file"])]
    input: Option<String>,
    #[arg(long, conflicts_with = "request_file")]
    input_file: Option<String>,
    #[arg(long,conflicts_with_all=["mode","reference"])]
    request_file: Option<PathBuf>,
    #[arg(long,value_parser=["plan","auto"])]
    mode: Option<String>,
    #[arg(long)]
    reference: Vec<Uuid>,
    #[command(flatten)]
    observe: Wait,
}
#[derive(Subcommand)]
pub enum Composer {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    List {
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long,default_value_t=100,value_parser=clap::value_parser!(u16).range(1..=200))]
        limit: u16,
    },
    Get {
        id: Uuid,
    },
    Rename {
        id: Uuid,
        name: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    Delete {
        id: Uuid,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    Send(Send),
    History {
        id: Uuid,
        #[arg(long,default_value_t=20,value_parser=clap::value_parser!(u16).range(1..=100))]
        limit: u16,
        #[arg(long)]
        before: Option<String>,
    },
    Watch {
        id: Uuid,
        #[arg(long)]
        turn: Option<String>,
        #[arg(long,default_value_t=600,value_parser=clap::value_parser!(u64).range(1..))]
        timeout: u64,
    },
    Stop {
        id: Uuid,
        #[command(flatten)]
        observe: Wait,
    },
}
fn invalid(m: &str) -> Error {
    Error::new("INVALID_INPUT", m)
}
async fn node(ctx: &Context<'_>, id: Uuid) -> Result<Value> {
    let n = ctx
        .client
        .canvas_entity(ctx.canvas, ctx.org, EntityKind::Element, id)
        .await?;
    if n["type"] != "agent" {
        return Err(invalid("Element is not a Composer"));
    }
    Ok(n)
}
fn style(n: &Value) -> Value {
    n.pointer("/attributes/style").cloned().unwrap_or(json!({}))
}
fn terminal(s: &str) -> Option<u8> {
    match s {
        "completed" => Some(0),
        "cancelled" | "failed" => Some(6),
        _ => None,
    }
}
fn frame_terminal(f: &Value) -> Option<u8> {
    [
        f.get("event"),
        f.pointer("/data/event"),
        f.pointer("/data/type"),
        f.pointer("/data/kind"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find_map(|s| match s {
        "turn_completed" => Some(0),
        "turn_cancelled" | "turn_aborted" | "turn_failed" => Some(6),
        _ => None,
    })
}
fn selected(s: &Value, run: &Option<String>, turn: &Option<String>) -> Option<Value> {
    let matches = |r: &Value| {
        turn.as_ref()
            .is_some_and(|t| r["agent_turn_id"].as_str() == Some(t))
            || run
                .as_ref()
                .is_some_and(|t| r["run_id"].as_str() == Some(t))
    };
    s.get("runs")
        .and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|r| matches(r)))
        .cloned()
        .or_else(|| matches(s).then(|| s.clone()))
}
async fn replay(
    ctx: &Context<'_>,
    id: Uuid,
    turn: &str,
    seen: &mut HashSet<String>,
) -> Result<Option<u8>> {
    let mut before = None;
    let mut cursors = HashSet::new();
    for _ in 0..100 {
        let events = ctx
            .client
            .composer_history(id, 100, before.as_deref())
            .await?;
        let mut active = false;
        let mut next = None;
        let mut more = false;
        for event in events {
            if event.event == "error" {
                return Err(Error::new(
                    "OBSERVATION_UNAVAILABLE",
                    "History replay failed",
                ));
            }
            if event.data["type"] == "history_meta" {
                more = event.data["hasMore"].as_bool().unwrap_or(false);
                next = event.data["nextCursor"].as_str().map(str::to_owned);
            }
            if let Some(t) = event.data.get("turnId").and_then(Value::as_str) {
                active = t == turn;
            }
            if active {
                let frame = json!(event);
                let end = frame_terminal(&frame);
                let key = format!(
                    "history:{}:{}",
                    before.as_deref().unwrap_or("latest"),
                    crate::storage::digest(&frame.to_string())
                );
                if seen.insert(key) {
                    print(
                        &json!({"type":"composer_history_event","frame":frame}),
                        ctx.compact,
                    )?;
                }
                if end.is_some() {
                    return Ok(end);
                }
            }
        }
        if !more {
            return Ok(None);
        }
        let cursor = next.ok_or_else(|| invalid("History pagination cursor missing"))?;
        if !cursors.insert(cursor.clone()) {
            return Err(invalid("History pagination cursor repeated"));
        }
        before = Some(cursor);
    }
    Err(Error::new(
        "OBSERVATION_UNAVAILABLE",
        "History recovery limit reached; use paginated composer history",
    ))
}
async fn observe(
    ctx: &Context<'_>,
    id: Uuid,
    mut socket: ComposerSocket,
    run: Option<String>,
    mut turn: Option<String>,
    timeout: u64,
) -> Result<u8> {
    let work = async {
        let mut reconnects = 0;
        let mut seen = HashSet::new();
        let mut last = tokio::time::Instant::now();
        loop {
            let s = style(&node(ctx, id).await?);
            if selected(&s, &run, &turn).is_none()
                && let Some(t) = &turn
                && let Some(code) = replay(ctx, id, t, &mut seen).await?
            {
                return Ok(code);
            }
            if let Some(r) = selected(&s, &run, &turn) {
                if turn.is_none() {
                    turn = r["agent_turn_id"].as_str().map(str::to_owned);
                }
                if let Some(code) = r["status"].as_str().and_then(terminal) {
                    print(&json!({"type":"composer_status","run":r}), ctx.compact)?;
                    return Ok(code);
                }
            }
            let disconnected = loop {
                match socket.next().await {
                    Ok(frame) => {
                        if frame["type"] == "observer_heartbeat" {
                            if last.elapsed().as_secs() > 60 {
                                break true;
                            }
                            continue;
                        }
                        last = tokio::time::Instant::now();
                        if frame["type"] == "agent_stream_frame"
                            && frame["agent_block_id"] == id.to_string()
                            && turn
                                .as_ref()
                                .is_some_and(|t| frame["agent_turn_id"].as_str() == Some(t))
                        {
                            let f = &frame["frame"];
                            let key = f
                                .get("seq")
                                .or_else(|| f.get("id"))
                                .filter(|v| !v.is_null())
                                .map(|v| format!("live:{v}"))
                                .or_else(|| f.pointer("/data/seq").map(|v| v.to_string()));
                            if key.is_none_or(|k| seen.insert(k)) {
                                print(&json!({"type":"composer_event","frame":f}), ctx.compact)?;
                            }
                            if let Some(code) = frame_terminal(f) {
                                return Ok(code);
                            }
                        }
                        if frame["type"] == "canvas_operation"
                            && frame
                                .pointer("/operation/element_id")
                                .and_then(Value::as_str)
                                == Some(&id.to_string())
                        {
                            break false;
                        }
                    }
                    Err(e)
                        if matches!(
                            e.code.as_str(),
                            "WS_ACCESS_DENIED" | "CLIENT_UPGRADE_REQUIRED"
                        ) =>
                    {
                        return Err(e);
                    }
                    Err(_) => break true,
                }
            };
            if !disconnected {
                continue;
            }
            reconnects += 1;
            if reconnects > 5 {
                return Err(Error::new(
                    "OBSERVATION_UNAVAILABLE",
                    "Observer recovery exhausted; use composer history/watch",
                ));
            }
            socket = ctx.client.composer_socket(ctx.canvas).await?;
            last = tokio::time::Instant::now();
            if let Some(t) = &turn
                && let Some(code) = replay(ctx, id, t, &mut seen).await?
            {
                return Ok(code);
            }
        }
    };
    tokio::select! {v=work=>v,_=tokio::signal::ctrl_c()=>Ok(130),_=tokio::time::sleep(std::time::Duration::from_secs(timeout))=>Err(Error::new("WAIT_TIMEOUT","Stopped observing; Composer may still be running"))}
}
async fn mutation(
    ctx: &Context<'_>,
    id: Option<Uuid>,
    action: &str,
    value: Value,
    key: &Option<String>,
) -> Result<u8> {
    let key = state::key(key)?;
    let user = state::user(ctx.client).await?;
    let intent = json!([id, action, value]);
    let scope = json!([ctx.backend, user, ctx.org, ctx.canvas, "composer-node", key]);
    let body = match state::freeze(ctx.store, &scope, &intent, None)? {
        Some(v) => v,
        None => {
            let revisions = match id {
                Some(id) => state::revisions(&node(ctx, id).await?)?,
                None => BTreeMap::new(),
            };
            let request = CanvasTransaction {
                org_id: ctx.org,
                operations: vec![CanvasCommand {
                    action: format!("element.{action}"),
                    id: id.unwrap_or_else(Uuid::new_v4),
                    value,
                    expected_revisions: revisions,
                }],
            };
            state::freeze(ctx.store, &scope, &intent, Some(json!(request)))?.unwrap()
        }
    };
    let result = ctx
        .client
        .canvas_transaction(
            ctx.canvas,
            &serde_json::from_value(body).map_err(|_| invalid("Invalid saved request"))?,
            &key,
            user,
        )
        .await
        .map_err(|e| {
            state::uncertain(
                e,
                "Composer node result unknown; reuse original key",
                Some(&key),
            )
        })?;
    print(&json!({"idempotency_key":key,"result":result}), ctx.compact)?;
    Ok(0)
}
pub async fn run(c: &Composer, ctx: &Context<'_>) -> Result<u8> {
    match c {
        Composer::Create {
            name,
            idempotency_key,
        } => {
            return mutation(
                ctx,
                None,
                "create",
                json!({"type":"agent","name":name}),
                idempotency_key,
            )
            .await;
        }
        Composer::Rename {
            id,
            name,
            idempotency_key,
        } => {
            return mutation(
                ctx,
                Some(*id),
                "update",
                json!({"name":name}),
                idempotency_key,
            )
            .await;
        }
        Composer::Delete {
            id,
            idempotency_key,
        } => return mutation(ctx, Some(*id), "delete", json!({}), idempotency_key).await,
        Composer::List { offset, limit } => {
            print(
                &ctx.client
                    .composer_list(ctx.canvas, ctx.org, *offset, *limit)
                    .await?,
                ctx.compact,
            )?;
        }
        Composer::Get { id } => print(&node(ctx, *id).await?, ctx.compact)?,
        Composer::History { id, limit, before } => {
            node(ctx, *id).await?;
            for event in ctx
                .client
                .composer_history(*id, *limit, before.as_deref())
                .await?
            {
                print(&event, ctx.compact)?;
            }
        }
        Composer::Watch { id, turn, timeout } => {
            let socket = ctx.client.composer_socket(ctx.canvas).await?;
            let s = style(&node(ctx, *id).await?);
            let run = if turn.is_none() {
                s["run_id"].as_str().map(str::to_owned)
            } else {
                None
            };
            if run.is_none() && turn.is_none() {
                print(&json!({"status":"idle"}), ctx.compact)?;
                return Ok(0);
            }
            return observe(ctx, *id, socket, run, turn.clone(), *timeout).await;
        }
        Composer::Stop { id, observe: w } => {
            let s = style(&node(ctx, *id).await?);
            let run = s["run_id"].as_str().map(str::to_owned);
            let turn = s["agent_turn_id"].as_str().map(str::to_owned);
            let socket = if w.wait {
                Some(ctx.client.composer_socket(ctx.canvas).await?)
            } else {
                None
            };
            let result = ctx.client.composer_interrupt(*id).await?;
            print(
                &json!({"type":"interrupt_requested","result":result}),
                ctx.compact,
            )?;
            if let Some(socket) = socket {
                if run.is_none() && turn.is_none() {
                    return Err(Error::new(
                        "WORKFLOW_UNCERTAIN",
                        "No execution identity to confirm stopping",
                    ));
                }
                return observe(ctx, *id, socket, run, turn, w.timeout).await;
            }
        }
        Composer::Send(args) => {
            node(ctx, args.id).await?;
            let mut body = if let Some(file) = &args.request_file {
                state::json_file(file).await?
            } else {
                let input = match &args.input_file {
                    Some(file) => {
                        let bytes = if file == "-" {
                            use std::io::Read;
                            let mut b = vec![];
                            std::io::stdin()
                                .take(1024 * 1024 + 1)
                                .read_to_end(&mut b)
                                .map_err(crate::storage::io_error)?;
                            b
                        } else {
                            crate::resource_upload::read_file(
                                std::path::Path::new(file),
                                1024 * 1024,
                                false,
                            )?
                        };
                        if bytes.len() > 1024 * 1024 {
                            return Err(invalid("Input exceeds 1 MiB"));
                        }
                        String::from_utf8(bytes).map_err(|_| invalid("Input must be UTF-8"))?
                    }
                    None => args.input.clone().unwrap_or_default(),
                };
                let references: serde_json::Map<String, Value> = args
                    .reference
                    .iter()
                    .enumerate()
                    .map(|(i, id)| (i.to_string(), json!({"source_element_id":id})))
                    .collect();
                json!({"input":input,"mode":args.mode.as_deref().unwrap_or("auto"),"references":references})
            };
            if !body.is_object()
                || body
                    .get("input")
                    .and_then(Value::as_str)
                    .is_none_or(|s| s.trim().is_empty())
            {
                return Err(invalid("Request must contain nonempty input"));
            }
            if body
                .get("organization_id")
                .is_some_and(|v| v != &json!(ctx.org))
            {
                return Err(invalid("Request organization differs from --org"));
            }
            const FIELDS: &[&str] = &[
                "input",
                "mode",
                "context",
                "references",
                "agent_model",
                "allowed_models",
                "allowed_aspect_ratios",
                "allowed_resolutions",
                "allowedStyles",
                "allowed_styles",
                "organization_id",
                "image_quality",
                "language",
                "generation_settings",
            ];
            if body
                .as_object()
                .unwrap()
                .keys()
                .any(|k| !FIELDS.contains(&k.as_str()))
            {
                return Err(invalid("Unsupported Composer request field"));
            }
            if body
                .get("mode")
                .is_some_and(|v| !matches!(v.as_str(), Some("auto" | "plan")))
            {
                return Err(invalid("Mode must be auto or plan"));
            }
            body["organization_id"] = json!(ctx.org);
            let user = state::user(ctx.client).await?;
            let rid = Uuid::new_v4().to_string();
            let name = format!("composer-send-{rid}.json");
            let record = json!({"backend":ctx.backend,"user":user,"org":ctx.org,"canvas":ctx.canvas,"block":args.id,"request_id":rid,"request_digest":crate::storage::digest(&body.to_string()),"status":"sending"});
            let _lock = state::exclusive(
                ctx.store,
                &json!([ctx.backend, user, ctx.org, ctx.canvas, args.id, "send"]),
            )?;
            let socket = if args.observe.wait {
                Some(ctx.client.composer_socket(ctx.canvas).await?)
            } else {
                None
            };
            ctx.store.save(&name, &record)?;
            eprintln!("Composer request: {rid}");
            let accepted = match ctx.client.composer_send(args.id, body, &rid).await {
                Ok(a) => a,
                Err(e) => {
                    if let Err(save_error) = ctx.store.save(
                        &name,
                        &json!({"request":record,"status":"unconfirmed","error":e.code}),
                    ) {
                        eprintln!("Could not update request record: {save_error}");
                    }
                    if e.code != "COMPATIBILITY_CHECK_FAILED"
                        && (e.retryable || e.status.is_none() || e.code == "INVALID_RESPONSE")
                    {
                        return Err(Error {
                            request_id: Some(rid),
                            ..Error::new(
                                "WORKFLOW_UNCERTAIN",
                                "Message acceptance unknown. Do not resend automatically; inspect composer history/get",
                            )
                        });
                    }
                    return Err(e);
                }
            };
            if let Err(e) = ctx.store.save(
                &name,
                &json!({"request":record,"status":"accepted","accepted":accepted}),
            ) {
                eprintln!(
                    "Run {} accepted, but local record update failed: {e}",
                    accepted.run_id
                );
            }
            print(&accepted, ctx.compact)?;
            if let Some(socket) = socket {
                return observe(
                    ctx,
                    args.id,
                    socket,
                    Some(accepted.run_id),
                    accepted.agent_turn_id,
                    args.observe.timeout,
                )
                .await;
            }
        }
    }
    Ok(0)
}
