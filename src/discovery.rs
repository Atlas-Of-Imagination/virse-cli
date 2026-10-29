use crate::{print, resource_paths::Resolver};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::json;
use uuid::Uuid;
use virse_api::{Client, Error, Result};

#[derive(Subcommand)]
pub enum Space {
    /// List personal/team spaces and your server-reported role (no --org required).
    List,
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Mode {
    Image,
    Video,
    Text,
    #[value(name = "3d")]
    Model3d,
}
impl Mode {
    fn api(self) -> virse_api::ModelMode {
        match self {
            Self::Image => virse_api::ModelMode::Image,
            Self::Video => virse_api::ModelMode::Video,
            Self::Text => virse_api::ModelMode::Text,
            Self::Model3d => virse_api::ModelMode::Model3d,
        }
    }
}
#[derive(Args)]
pub struct ModelList {
    #[arg(long, value_enum)]
    pub mode: Vec<Mode>,
    /// Include disabled, coming-soon and plan-locked models and their reasons.
    #[arg(long)]
    pub include_disabled: bool,
    #[arg(long)]
    pub favorites: bool,
}
#[derive(Subcommand)]
pub enum Model {
    List(ModelList),
    /// Get all catalog entries matching an exact model key, including disabled entries.
    Get {
        key: String,
    },
    Favorite {
        key: String,
    },
    Unfavorite {
        key: String,
    },
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Representation {
    Preview,
    Content,
}
impl Representation {
    pub fn api(self) -> virse_api::AssetRepresentation {
        match self {
            Self::Preview => virse_api::AssetRepresentation::Preview,
            Self::Content => virse_api::AssetRepresentation::Content,
        }
    }
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Sort {
    Name,
    CreatedAt,
    ModifiedAt,
    Size,
    Custom,
}
impl Sort {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::CreatedAt => "created_at",
            Self::ModifiedAt => "modified_at",
            Self::Size => "size",
            Self::Custom => "custom",
        }
    }
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Order {
    Asc,
    Desc,
}
impl Order {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Access {
    View,
    Use,
    Edit,
    Manage,
}
impl Access {
    pub fn number(self) -> u8 {
        match self {
            Self::View => 1,
            Self::Use => 2,
            Self::Edit => 3,
            Self::Manage => 4,
        }
    }
}
#[derive(Args)]
pub struct CreateCanvas {
    pub name: String,
    /// Existing resource folder path or @UUID; defaults to organization root.
    #[arg(long, default_value = "/")]
    pub parent: String,
}
#[derive(Args)]
pub struct Contents {
    /// Canvas resource path or @resource UUID, not its canvas-space ID.
    pub resource: String,
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
    #[arg(long,default_value_t=100,value_parser=clap::value_parser!(u16).range(1..=200))]
    pub limit: u16,
    #[arg(long)]
    pub search: Option<String>,
    #[arg(long,value_parser=["image","video","model3d","document","aesthetic"])]
    pub kind: Vec<String>,
    #[arg(long,value_parser=["word","pdf"])]
    pub document_type: Vec<String>,
    #[arg(long,default_value="created-at",value_parser=["name","created-at","modified-at","size"])]
    pub sort: String,
    #[arg(long, value_enum, default_value = "asc")]
    pub order: Order,
}
fn uncertain(error: Error, code: &str, message: &str) -> Error {
    if error.retryable
        || error.status.is_some_and(|s| s >= 500)
        || matches!(error.code.as_str(), "INVALID_RESPONSE" | "internal")
    {
        Error {
            status: error.status,
            request_id: error.request_id,
            details: error.details,
            ..Error::new(code, message)
        }
    } else {
        error
    }
}
pub async fn models(command: &Model, client: &Client, org: Uuid, json_output: bool) -> Result<u8> {
    match command {
        Model::List(options) => {
            let mut catalog = client
                .model_catalog(
                    org,
                    &options.mode.iter().map(|m| m.api()).collect::<Vec<_>>(),
                    options.include_disabled,
                )
                .await?;
            if options.favorites {
                catalog.models.retain(|m| {
                    m.extra
                        .get("user_preferences")
                        .and_then(|v| v.get("is_favorite"))
                        .and_then(|v| v.as_bool())
                        == Some(true)
                });
            }
            print(&catalog, json_output)?;
        }
        Model::Get { key } => {
            let mut catalog = client.model_catalog(org, &[], true).await?;
            catalog.models.retain(|m| m.model_key == *key);
            if catalog.models.is_empty() {
                return Err(Error::new(
                    "MODEL_NOT_FOUND",
                    "Model not present in this space's catalog",
                ));
            }
            print(&catalog, json_output)?;
        }
        Model::Favorite { key } | Model::Unfavorite { key } => {
            let result = client
                .set_model_favorite(org, key, matches!(command, Model::Favorite { .. }))
                .await
                .map_err(|e| {
                    uncertain(
                        e,
                        "MODEL_PREFERENCE_UNCERTAIN",
                        "Favorite result unknown; reread model get/list before retrying",
                    )
                })?;
            print(&result, json_output)?;
        }
    }
    Ok(0)
}
pub async fn create_canvas(
    args: &CreateCanvas,
    client: &Client,
    org: Uuid,
    json_output: bool,
) -> Result<u8> {
    if args.name.trim().is_empty() || args.name.chars().count() > 255 {
        return Err(Error::new(
            "INVALID_INPUT",
            "Canvas name must contain 1..255 characters",
        ));
    }
    let parent = Resolver::new(client, org).folder(&args.parent).await?;
    let result=client.create_canvas(org,parent,&args.name).await.map_err(|e|uncertain(e,"CANVAS_CREATION_UNCERTAIN","Canvas creation may have succeeded. List the target folder before retrying; this endpoint has no idempotency contract"))?;
    print(&result, json_output)?;
    Ok(0)
}
pub async fn contents(
    args: &Contents,
    client: &Client,
    org: Uuid,
    json_output: bool,
) -> Result<u8> {
    let item = Resolver::new(client, org).item(&args.resource).await?;
    if item.kind != "canvas" {
        return Err(Error::new(
            "NOT_A_CANVAS",
            "Choose a canvas resource path or resource ID",
        ));
    }
    let mut query = virse_api::CanvasContentsQuery::new(org, item.id);
    query.offset = args.offset;
    query.limit = args.limit;
    query.query = args.search.clone();
    query.types = args.kind.clone();
    query.document_types = args.document_type.clone();
    query.sort_by = match args.sort.as_str() {
        "size" => "size_bytes".into(),
        other => other.replace('-', "_"),
    };
    query.sort_order = args.order.wire().into();
    let result = client.canvas_contents(&query).await?;
    let expected = item
        .extra
        .get("target_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    if expected.is_some_and(|id| id != result.canvas_id) {
        return Err(Error::new(
            "INVALID_RESPONSE",
            "Content belongs to another canvas",
        ));
    }
    print(&result, json_output)?;
    if result.has_more {
        eprintln!(
            "More contents available; repeat with the same filters and --offset {}",
            result.offset + result.items.len() as u64
        );
    }
    Ok(0)
}
pub async fn resource_preview(
    client: &Client,
    org: Uuid,
    target: &str,
    representation: Representation,
    output: &std::path::Path,
    json_output: bool,
) -> Result<u8> {
    if output.try_exists().map_err(crate::storage::io_error)? {
        return Err(Error::new("OUTPUT_EXISTS", "Choose a new output path"));
    }
    let item = Resolver::new(client, org).item(target).await?;
    let info = client
        .resource_presentation_info(org, item.id, representation.api())
        .await?;
    let size = crate::download::save_info(client, &info, output).await?;
    print(
        &json!({"resource_id":item.id,"path":output,"bytes":size,"representation":representation.api(),"content":info.extra.get("content"),"source_version":info.source_version()}),
        json_output,
    )?;
    Ok(0)
}

pub async fn asset_preview(
    client: &Client,
    org: Uuid,
    asset: Uuid,
    canvas: Option<Uuid>,
    representation: Representation,
    output: &std::path::Path,
    json_output: bool,
) -> Result<u8> {
    if output.try_exists().map_err(crate::storage::io_error)? {
        return Err(Error::new("OUTPUT_EXISTS", "Choose a new output path"));
    }
    let info = client
        .presentation_info_in(
            asset,
            &virse_api::AccessContext {
                org_id: org,
                dentry_id: None,
                canvas_space_id: canvas,
            },
            representation.api(),
        )
        .await?;
    let size = crate::download::save_info(client, &info, output).await?;
    print(
        &json!({"asset_id":asset,"path":output,"bytes":size,"representation":representation.api(),"content":info.extra.get("content"),"source_version":info.source_version()}),
        json_output,
    )?;
    Ok(0)
}
