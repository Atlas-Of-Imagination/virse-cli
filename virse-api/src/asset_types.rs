//! 公共资产契约。响应保留未知字段；SDK 接收已冻结的字节，不读取本地文件。
use crate::{AccessContext, Error, ResourceItem, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

pub const MAX_ASSET_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct AssetUploadFile {
    pub filename: String,
    pub bytes: Vec<u8>,
    /// 仅为传输提示；实际格式与结构由服务端验证。
    pub mime_type: Option<String>,
}
impl AssetUploadFile {
    pub fn new(filename: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            filename: filename.into(),
            bytes,
            mime_type: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.bytes.is_empty() || self.bytes.len() > MAX_ASSET_UPLOAD_BYTES {
            return Err(Error::new(
                "INVALID_INPUT",
                "Asset file must contain 1 byte to 100 MiB",
            ));
        }
        if self.filename.trim().is_empty() || self.filename.chars().any(char::is_control) {
            return Err(Error::new(
                "INVALID_INPUT",
                "Asset filename must be nonempty and contain no control characters",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Destination {
    Drive {
        org_id: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_id: Option<Uuid>,
    },
    Canvas {
        org_id: Uuid,
        canvas_space_id: Uuid,
    },
    Unbundled {
        org_id: Uuid,
    },
    Space {
        org_id: Uuid,
        space_id: Uuid,
    },
}
impl Destination {
    pub fn folder(org_id: Uuid, parent_id: Option<Uuid>) -> Self {
        Self::Drive { org_id, parent_id }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadConflict {
    #[default]
    Rename,
    Skip,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetRetention {
    #[default]
    Persistent,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadOptions {
    pub destination: Destination,
    #[serde(default)]
    pub conflict: UploadConflict,
    #[serde(default)]
    pub retention: AssetRetention,
}
impl UploadOptions {
    pub fn new(destination: Destination) -> Self {
        Self {
            destination,
            conflict: UploadConflict::Rename,
            retention: AssetRetention::Persistent,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.retention == AssetRetention::Pending
            && !matches!(self.destination, Destination::Unbundled { .. })
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Pending retention requires an unbundled destination",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceOptions {
    pub context: AccessContext,
    /// 必须来自服务端读取；SDK 不猜测 revision，也不自动处理冲突。
    pub expected_revision: u64,
    #[serde(default)]
    pub reset_model_state: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_state_revision: Option<u64>,
}
impl ReplaceOptions {
    pub fn new(context: AccessContext, expected_revision: u64) -> Self {
        Self {
            context,
            expected_revision,
            reset_model_state: false,
            expected_state_revision: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.context.query()?;
        if self.expected_revision == 0 {
            return Err(Error::new(
                "INVALID_INPUT",
                "Replacement requires a server content revision of at least 1",
            ));
        }
        if self.reset_model_state && self.expected_state_revision.is_none() {
            return Err(Error::new(
                "INVALID_INPUT",
                "Resetting model state requires its server state revision",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Image,
    Video,
    Document,
    #[serde(rename = "model3d")]
    Model3d,
    /// 后端增加类型时仍能读取并无损输出 DTO。
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    Word,
    Pdf,
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetChecksum {
    pub algorithm: String,
    pub value: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetContent {
    /// 0 表示尚未提交首份内容，不能用于替换请求。
    pub revision: u64,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub checksum: Option<AssetChecksum>,
    pub metadata_state: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetCapabilities {
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub download: bool,
    #[serde(default)]
    pub copy: bool,
    #[serde(default)]
    pub replace_content: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub asset_type: AssetType,
    pub document_type: Option<DocumentType>,
    pub status: String,
    pub description: Option<String>,
    pub content: AssetContent,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    #[serde(default)]
    pub capabilities: AssetCapabilities,
    #[serde(default)]
    pub attributes: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResult {
    pub asset: Asset,
    pub resource: Option<ResourceItem>,
    /// 同名跳过不表示两份文件的字节相同。
    pub skipped: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetQuery {
    pub context: AccessContext,
    #[serde(default)]
    pub types: Vec<AssetType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_type: Option<DocumentType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default)]
    pub include_public: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u16,
}
impl AssetQuery {
    pub fn new(org_id: Uuid) -> Self {
        Self {
            context: AccessContext::organization(org_id),
            types: vec![],
            document_type: None,
            query: None,
            include_public: false,
            cursor: None,
            limit: 100,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.context.query()?;
        if !(1..=200).contains(&self.limit)
            || self.query.as_ref().is_some_and(|s| s.chars().count() > 256)
            || self
                .cursor
                .as_ref()
                .is_some_and(|s| s.chars().count() > 256)
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Invalid asset query limit, search text or cursor",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPage {
    pub items: Vec<Asset>,
    pub next_cursor: Option<String>,
    pub snapshot: String,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_cursor: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetRepresentation {
    #[default]
    Preview,
    Content,
}
impl AssetRepresentation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Content => "content",
        }
    }
}
