use crate::{Client, Error, Point, Result, Size};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetElementKind {
    Image,
    Video,
    #[serde(rename = "3d")]
    Model3d,
    Document,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingAsset {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub document_type: Option<String>,
}
impl ExistingAsset {
    pub fn element_kind(&self) -> Result<AssetElementKind> {
        if self.status != "ready" {
            return Err(Error::new(
                "ASSET_NOT_READY",
                "Only ready assets can be placed on the canvas",
            ));
        }
        match self.kind.as_str() {
            "image" => Ok(AssetElementKind::Image),
            "video" => Ok(AssetElementKind::Video),
            "model3d" => Ok(AssetElementKind::Model3d),
            "document" if matches!(self.document_type.as_deref(), Some("word" | "pdf")) => {
                Ok(AssetElementKind::Document)
            }
            _ => Err(Error::new(
                "UNSUPPORTED_ASSET",
                "Supported assets are images, videos, GLB, Word and PDF",
            )),
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementLayout {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
}
impl ElementLayout {
    pub fn validate(&self) -> Result<()> {
        if self
            .position
            .as_ref()
            .is_some_and(|p| !p.x.is_finite() || !p.y.is_finite())
            || self.size.as_ref().is_some_and(|s| {
                !s.width.is_finite() || !s.height.is_finite() || s.width <= 0.0 || s.height <= 0.0
            })
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Layout must contain finite coordinates and positive dimensions",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetContentRef {
    Asset { id: Uuid },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAssetElement {
    pub org_id: Uuid,
    #[serde(rename = "type")]
    pub kind: AssetElementKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub layout: ElementLayout,
    pub content_ref: AssetContentRef,
}
impl CreateAssetElement {
    pub fn from_asset(
        org_id: Uuid,
        asset: &ExistingAsset,
        name: Option<String>,
        layout: ElementLayout,
    ) -> Result<Self> {
        let request = Self {
            org_id,
            kind: asset.element_kind()?,
            name,
            layout,
            content_ref: AssetContentRef::Asset { id: asset.id },
        };
        request.validate()?;
        Ok(request)
    }
    pub fn validate(&self) -> Result<()> {
        self.layout.validate()?;
        if self
            .name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty() || name.chars().count() > 255)
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Element name must contain 1..255 characters",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasAssetElement {
    pub id: Uuid,
    pub canvas_id: Uuid,
    #[serde(rename = "type")]
    pub kind: AssetElementKind,
    pub content_ref: AssetContentRef,
    pub layout: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedAssetElement {
    pub element: CanvasAssetElement,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Client {
    /// One idempotent HTTP attempt. The server chooses default layout and owns asset binding.
    pub async fn create_asset_element(
        &self,
        canvas: Uuid,
        body: &CreateAssetElement,
        key: &str,
        expected_user: Uuid,
    ) -> Result<CreatedAssetElement> {
        body.validate()?;
        let value = self
            .expecting_user(expected_user)
            .request(
                Method::POST,
                &format!("v2/canvas/projects/{canvas}/elements"),
                &[],
                Some(json!(body)),
                Some(key),
            )
            .await?;
        let result: CreatedAssetElement =
            serde_json::from_value(value).map_err(|_| Error::protocol())?;
        if result.element.canvas_id != canvas || result.element.kind != body.kind {
            return Err(Error::protocol());
        }
        Ok(result)
    }
}
