use crate::{print, storage::Store, tool_args::ToolVersion, workflow_state as state};
use clap::{Args, Subcommand};
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;
use virse_api::{CanvasCommand, CanvasTransaction, Client, Error, Result, ToolIdentity};
#[derive(Args)]
pub struct Place {
    pub id: String,
    #[command(flatten)]
    pub version: ToolVersion,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pub x: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub y: Option<f64>,
    #[arg(long, default_value_t = 820.0)]
    pub width: f64,
    #[arg(long, default_value_t = 400.0)]
    pub height: f64,
    #[arg(long)]
    pub idempotency_key: Option<String>,
}
#[derive(Subcommand)]
pub enum Cover {
    Upload { file: PathBuf },
}
pub async fn cover(command: &Cover, client: &Client, compact: bool) -> Result<u8> {
    let Cover::Upload { file } = command;
    let bytes = crate::resource_upload::read_file(file, 10 * 1024 * 1024, false)?;
    let filename = file
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::new("INVALID_INPUT", "Cover filename must be UTF-8"))?;
    let value = client
        .upload_tool_cover(virse_api::AssetUploadFile::new(filename, bytes))
        .await
        .map_err(|e| {
            state::uncertain(
                e,
                "Cover upload result unknown; no tool metadata was changed",
                None,
            )
        })?;
    print(&value, compact)?;
    Ok(0)
}
pub async fn place(
    args: &Place,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    canvas: Uuid,
    compact: bool,
) -> Result<u8> {
    if !args.width.is_finite()
        || !args.height.is_finite()
        || args.width <= 0.0
        || args.height <= 0.0
        || args.x.is_some_and(|v| !v.is_finite())
        || args.y.is_some_and(|v| !v.is_finite())
    {
        return Err(Error::new("INVALID_INPUT", "Invalid instance layout"));
    }
    let user = state::user(client).await?;
    let key = state::key(&args.idempotency_key)?;
    let identity = ToolIdentity {
        skill_id: args.id.clone(),
        version: args.version.version.clone(),
        version_id: args.version.version_id.clone(),
    };
    identity.validate()?;
    let intent = json!({"tool":identity,"locale":args.version.locale,"name":args.name,"x":args.x,"y":args.y,"width":args.width,"height":args.height});
    let scope = json!([backend, user, org, canvas, "tool-place", key]);
    let body = match state::freeze(store, &scope, &intent, None)? {
        Some(v) => v,
        None => {
            let pinned = client
                .tool_schema(canvas, org, &identity, &args.version.locale)
                .await?
                .pinned_identity()?;
            let mut value = json!({"type":"skill","skill_id":pinned.skill_id,"version_id":pinned.version_id,"layout":{"size":{"width":args.width,"height":args.height}}});
            if let Some(name) = &args.name {
                value["name"] = json!(name);
            }
            if args.x.is_some() || args.y.is_some() {
                value["layout"]["position"] =
                    json!({"x":args.x.unwrap_or(0.0),"y":args.y.unwrap_or(0.0)});
            }
            let request = CanvasTransaction {
                org_id: org,
                operations: vec![CanvasCommand {
                    action: "element.create".into(),
                    id: Uuid::new_v4(),
                    value,
                    expected_revisions: Default::default(),
                }],
            };
            state::freeze(store, &scope, &intent, Some(json!(request)))?.unwrap()
        }
    };
    let request: CanvasTransaction = serde_json::from_value(body)
        .map_err(|_| Error::new("CONFIG_ERROR", "Invalid instance record"))?;
    let result = client
        .canvas_transaction(canvas, &request, &key, user)
        .await
        .map_err(|e| {
            state::uncertain(
                e,
                "Tool placement result unknown; repeat original key",
                Some(&key),
            )
        })?;
    print(
        &json!({"idempotency_key":key,"result":result,"started":false}),
        compact,
    )?;
    Ok(0)
}
