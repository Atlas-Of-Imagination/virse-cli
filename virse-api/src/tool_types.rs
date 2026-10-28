//! Public marketplace JSON is camelCase; V2 run JSON is snake_case.
use crate::{ElementLayout, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolView {
    Created,
    Mine,
    Store,
}
impl ToolView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Mine => "mine",
            Self::Store => "store",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolScope {
    Personal,
    Team,
}
impl ToolScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Team => "team",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolListOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    pub locale: String,
}
impl Default for ToolListOptions {
    fn default() -> Self {
        Self {
            organization_id: None,
            query: None,
            category_id: None,
            limit: None,
            offset: None,
            locale: "zh".into(),
        }
    }
}
/// `id` identifies a formal SkillRelease; only `artifact_version_id` is an execution pin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRelease {
    pub id: String,
    pub artifact_version_id: String,
    pub version: String,
    pub current: bool,
    #[serde(default)]
    pub legacy: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolVersions {
    pub versions: Vec<ToolRelease>,
}
#[derive(Debug, Clone, Copy)]
pub enum ToolMutation {
    Add,
    Remove,
    Enable,
    Disable,
    Star,
    Unstar,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    pub skill_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}
impl ToolIdentity {
    pub fn validate(&self) -> Result<()> {
        validate_tool_id(&self.skill_id)?;
        if self.version_id.is_some() && self.version.is_some() {
            return Err(invalid("Choose version or artifact version ID, not both"));
        }
        if self.version.as_ref().is_some_and(|s| s.trim().is_empty())
            || self
                .version_id
                .as_ref()
                .is_some_and(|s| s.trim().is_empty())
        {
            return Err(invalid("Version must not be empty"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub skill_id: String,
    pub version_id: Option<String>,
    pub version: Option<String>,
    pub input_schema: Option<Value>,
}
impl ToolSchema {
    pub fn pinned_identity(&self) -> Result<ToolIdentity> {
        let id = self
            .version_id
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                Error::new(
                    "TOOL_VERSION_NOT_IMMUTABLE",
                    "Schema has no immutable artifact version; publish the tool before running it",
                )
            })?;
        Ok(ToolIdentity {
            skill_id: self.skill_id.clone(),
            version_id: Some(id.clone()),
            version: None,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolElement {
    #[serde(rename = "type")]
    pub kind: String,
    pub skill_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub layout: ElementLayout,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_element: Option<ToolElement>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRunRequest {
    pub org_id: Uuid,
    pub tool: ToolIdentity,
    #[serde(default)]
    pub target: ToolTarget,
    #[serde(default)]
    pub inputs: Map<String, Value>,
    #[serde(default)]
    pub instruction: String,
    pub locale: String,
    #[serde(default)]
    pub options: Map<String, Value>,
    #[serde(default)]
    pub aesthetic_sources: Vec<Value>,
}
impl ToolRunRequest {
    pub fn validate(&self) -> Result<()> {
        self.tool.validate()?;
        if self.tool.version_id.is_none() {
            return Err(invalid(
                "Run requires an immutable artifact version_id from tool schema",
            ));
        }
        validate_locale(&self.locale)?;
        if self.instruction.chars().count() > 100_000 {
            return Err(invalid("Instruction exceeds 100000 characters"));
        }
        if self.aesthetic_sources.len() > 100
            || self.aesthetic_sources.iter().any(|s| !s.is_object())
        {
            return Err(invalid(
                "At most 100 aesthetic source objects are supported",
            ));
        }
        if self.target.element_id.is_some() && self.target.create_element.is_some() {
            return Err(invalid("Choose an existing element or a new skill node"));
        }
        if let Some(node) = &self.target.create_element {
            node.layout.validate()?;
            if node.kind != "skill"
                || node.skill_id != self.tool.skill_id
                || node
                    .version_id
                    .as_ref()
                    .is_some_and(|id| Some(id) != self.tool.version_id.as_ref())
            {
                return Err(invalid("Skill node identity must match the pinned tool"));
            }
            if node
                .name
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.chars().count() > 255)
            {
                return Err(invalid("Node name must contain 1–255 characters"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRunActions {
    pub cancel: bool,
    pub interrupt: bool,
    pub resume: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRun {
    pub run_id: Uuid,
    pub kind: String,
    pub canvas_id: Uuid,
    pub element_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
    pub session_id: Option<String>,
    pub agent_turn_id: Option<String>,
    pub status: String,
    pub revision: u64,
    pub result: Value,
    pub error: Option<Value>,
    pub actions: ToolRunActions,
    pub tool: Option<ToolIdentity>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl ToolRun {
    pub fn stop_waiting(&self) -> bool {
        matches!(
            self.status.as_str(),
            "succeeded" | "failed" | "cancelled" | "needs_attention"
        )
    }
}
#[derive(Debug, Clone, Copy)]
pub enum ToolRunAction {
    Cancel,
    Interrupt,
    Resume,
}
impl ToolRunAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::Interrupt => "interrupt",
            Self::Resume => "resume",
        }
    }
}
pub(crate) fn invalid(message: &str) -> Error {
    Error::new("INVALID_INPUT", message)
}
pub(crate) fn validate_locale(locale: &str) -> Result<()> {
    if matches!(locale, "zh" | "en") {
        Ok(())
    } else {
        Err(invalid("Locale must be zh or en"))
    }
}
pub(crate) fn validate_tool_id(id: &str) -> Result<()> {
    // IDs occupy one path segment. Reject delimiters instead of allowing URL reinterpretation.
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        Err(invalid("Invalid tool ID"))
    } else {
        Ok(())
    }
}
