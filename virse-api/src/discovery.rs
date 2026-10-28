//! Space/model discovery and read-only balance. No implicit context selection.
use crate::{Client, Error, ResourceItem, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: Uuid,
    pub name: Option<String>,
    pub org_type: String,
    pub my_role: Option<String>,
    pub relation_type: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organizations {
    pub organizations: Vec<Organization>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelMode {
    Image,
    Video,
    Text,
    #[serde(rename = "3d")]
    Model3d,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogModel {
    pub model_key: String,
    pub mode: String,
    pub display_name: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalog {
    pub schema_version: u32,
    pub catalog_version: String,
    pub models: Vec<CatalogModel>,
    #[serde(default)]
    pub defaults: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFavorite {
    pub model_key: String,
    pub is_favorite: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub org_id: Uuid,
    /// Monetary/credit values stay in their original JSON representation.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedCanvas {
    pub canvas_space_id: Uuid,
    pub dentry: ResourceItem,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasContents {
    pub canvas_id: Uuid,
    pub items: Vec<Value>,
    pub total: u64,
    pub offset: u64,
    pub limit: u16,
    pub has_more: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CanvasContentsQuery {
    pub org_id: Uuid,
    pub dentry_id: Uuid,
    pub offset: u32,
    pub limit: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub document_types: Vec<String>,
    pub sort_by: String,
    pub sort_order: String,
}
impl CanvasContentsQuery {
    pub fn new(org_id: Uuid, dentry_id: Uuid) -> Self {
        Self {
            org_id,
            dentry_id,
            offset: 0,
            limit: 100,
            query: None,
            types: vec![],
            document_types: vec![],
            sort_by: "created_at".into(),
            sort_order: "asc".into(),
        }
    }
    fn validate(&self) -> Result<()> {
        if !(1..=200).contains(&self.limit)
            || self.query.as_ref().is_some_and(|s| s.chars().count() > 255)
            || self.types.len() > 5
            || self.types.iter().any(|s| {
                !matches!(
                    s.as_str(),
                    "image" | "video" | "model3d" | "document" | "aesthetic"
                )
            })
            || self.document_types.len() > 2
            || self
                .document_types
                .iter()
                .any(|s| !matches!(s.as_str(), "word" | "pdf"))
            || !matches!(
                self.sort_by.as_str(),
                "name" | "created_at" | "modified_at" | "size_bytes"
            )
            || !matches!(self.sort_order.as_str(), "asc" | "desc")
        {
            return Err(Error::new("INVALID_INPUT", "Invalid canvas content query"));
        }
        Ok(())
    }
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::protocol())
}
impl Client {
    pub async fn organizations(&self) -> Result<Organizations> {
        parse(
            self.request(
                Method::POST,
                "get_organization_by_user",
                &[],
                Some(json!({"relation_types":["has_owner","has_member"]})),
                None,
            )
            .await?,
        )
    }
    pub async fn model_catalog(
        &self,
        org: Uuid,
        modes: &[ModelMode],
        include_disabled: bool,
    ) -> Result<ModelCatalog> {
        let all_modes = [
            ModelMode::Image,
            ModelMode::Text,
            ModelMode::Video,
            ModelMode::Model3d,
        ];
        let modes = if modes.is_empty() {
            &all_modes[..]
        } else {
            modes
        };
        let catalog:ModelCatalog=parse(self.request(Method::POST,"generation/models/catalog",&[],Some(json!({"organization_id":org,"modes":modes,"include_disabled":include_disabled,"include_user_preferences":true,"schema_version":2})),None).await?)?;
        if catalog.schema_version != 2 {
            return Err(Error::protocol());
        }
        Ok(catalog)
    }
    /// One attempt; callers can reread preferences after uncertain results.
    pub async fn set_model_favorite(
        &self,
        org: Uuid,
        key: &str,
        is_favorite: bool,
    ) -> Result<ModelFavorite> {
        if key.trim().is_empty() || key.chars().any(char::is_control) {
            return Err(Error::new("INVALID_INPUT", "Invalid model key"));
        }
        let mut url = self.url("generation/models/")?;
        {
            let mut parts = url.path_segments_mut().map_err(|_| Error::protocol())?;
            parts.pop_if_empty().push(key).push("favorite");
        }
        let result: ModelFavorite = parse(
            self.request(
                Method::PATCH,
                url.as_str(),
                &[],
                Some(json!({"organization_id":org,"is_favorite":is_favorite})),
                None,
            )
            .await?,
        )?;
        if result.model_key != key || result.is_favorite != is_favorite {
            return Err(Error::protocol());
        }
        Ok(result)
    }
    pub async fn balance(&self, org: Uuid) -> Result<Balance> {
        let result: Balance = parse(
            self.request(
                Method::POST,
                "billing/balance",
                &[],
                Some(json!({"organization_id":org})),
                None,
            )
            .await
            .map_err(|mut error| {
                if error.message == "NOT_ORG_MEMBER" {
                    error.code = "not_org_member".into();
                    error.status = Some(403);
                }
                error
            })?,
        )?;
        if result.org_id != org {
            return Err(Error::protocol());
        }
        Ok(result)
    }
    /// The legacy canvas-create endpoint has no idempotency contract: never retry blindly.
    pub async fn create_canvas(
        &self,
        org: Uuid,
        parent: Option<Uuid>,
        name: &str,
    ) -> Result<CreatedCanvas> {
        if name.trim().is_empty() || name.chars().count() > 255 {
            return Err(Error::new(
                "INVALID_INPUT",
                "Canvas name must contain 1..255 characters",
            ));
        }
        let result: CreatedCanvas = parse(
            self.request(
                Method::POST,
                "versa/canvas/create",
                &[],
                Some(json!({"org_id":org,"parent_id":parent,"name":name})),
                None,
            )
            .await?,
        )?;
        if result.dentry.kind != "canvas"
            || result
                .dentry
                .extra
                .get("target_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                != Some(result.canvas_space_id)
        {
            return Err(Error::protocol());
        }
        Ok(result)
    }
    pub async fn canvas_contents(&self, query: &CanvasContentsQuery) -> Result<CanvasContents> {
        query.validate()?;
        let mut body = serde_json::to_value(query).map_err(|_| Error::protocol())?;
        body["contract_version"] = 2.into();
        let result: CanvasContents = parse(
            self.request(Method::POST, "versa/canvas/contents", &[], Some(body), None)
                .await?,
        )?;
        if result.offset != u64::from(query.offset)
            || result.limit != query.limit
            || result.items.len() > usize::from(query.limit)
        {
            return Err(Error::protocol());
        }
        Ok(result)
    }
}
