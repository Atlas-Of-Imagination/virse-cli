use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceItem {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub asset_id: Option<Uuid>,
    #[serde(default)]
    pub capabilities: BTreeMap<String, bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl ResourceItem {
    pub fn is_folder(&self) -> bool {
        self.kind == "folder"
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePage {
    pub items: Vec<ResourceItem>,
    pub next_cursor: Option<String>,
    pub snapshot: String,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDetail {
    pub resource: ResourceItem,
    pub kind: String,
    pub asset: Option<Value>,
    pub detail: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessContext {
    pub org_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dentry_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_space_id: Option<Uuid>,
}
impl AccessContext {
    pub fn organization(org_id: Uuid) -> Self {
        Self {
            org_id,
            dentry_id: None,
            canvas_space_id: None,
        }
    }
    pub(crate) fn query(&self) -> Result<Vec<(&'static str, String)>> {
        if self.dentry_id.is_some() && self.canvas_space_id.is_some() {
            return Err(Error::new(
                "INVALID_INPUT",
                "Only one access location may be supplied",
            ));
        }
        let mut query = vec![("org_id", self.org_id.to_string())];
        if let Some(id) = self.dentry_id {
            query.push(("dentry_id", id.to_string()));
        }
        if let Some(id) = self.canvas_space_id {
            query.push(("canvas_space_id", id.to_string()));
        }
        Ok(query)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceQuery {
    pub context: AccessContext,
    pub parent_id: Option<Uuid>,
    pub recursive: bool,
    pub query: Option<String>,
    pub kinds: Vec<String>,
    pub tab: String,
    pub sort: String,
    pub direction: String,
    pub entry_points: bool,
    pub cursor: Option<String>,
    pub limit: u16,
}
impl ResourceQuery {
    pub fn new(org: Uuid) -> Self {
        Self {
            context: AccessContext::organization(org),
            parent_id: None,
            recursive: false,
            query: None,
            kinds: vec![],
            tab: "all".into(),
            sort: "name".into(),
            direction: "asc".into(),
            entry_points: false,
            cursor: None,
            limit: 200,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOperation {
    Rename,
    Move,
    Copy,
    Trash,
    Restore,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDestination {
    pub org_id: Uuid,
    pub kind: String,
    pub parent_id: Option<Uuid>,
}
impl ResourceDestination {
    pub fn folder(org_id: Uuid, parent_id: Option<Uuid>) -> Self {
        Self {
            org_id,
            kind: "drive".into(),
            parent_id,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMutation {
    pub context: AccessContext,
    pub op: ResourceOperation,
    pub ids: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<ResourceDestination>,
    pub confirm_widen: bool,
    pub confirm_narrow: bool,
}
impl ResourceMutation {
    pub fn validate(&self) -> Result<()> {
        let invalid = || Error::new("INVALID_INPUT", "Invalid resource operation");
        if self.ids.is_empty()
            || self.ids.len() > 200
            || self.ids.iter().collect::<HashSet<_>>().len() != self.ids.len()
            || self.context.dentry_id.is_some()
            || self.context.canvas_space_id.is_some()
        {
            return Err(invalid());
        }
        if self.op == ResourceOperation::Rename
            && (self.ids.len() != 1
                || self
                    .name
                    .as_ref()
                    .is_none_or(|s| s.trim().is_empty() || s.chars().count() > 255))
        {
            return Err(invalid());
        }
        if matches!(self.op, ResourceOperation::Copy | ResourceOperation::Move)
            != self.destination.is_some()
        {
            return Err(invalid());
        }
        if let Some(destination) = &self.destination
            && (destination.org_id != self.context.org_id || destination.kind != "drive")
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceFailure {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub details: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceOutcome {
    pub id: Uuid,
    pub outcome: String,
    pub error: Option<ResourceFailure>,
    pub data: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBatch {
    pub items: Vec<ResourceOutcome>,
}
impl ResourceBatch {
    pub fn succeeded(&self) -> bool {
        self.items.iter().all(|item| item.outcome == "succeeded")
    }
}
