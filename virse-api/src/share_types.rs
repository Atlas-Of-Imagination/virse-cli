use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "subject_type",
    content = "subject_id",
    rename_all = "snake_case"
)]
pub enum ShareSubject {
    Member(String),
    Group(Uuid),
    Email(String),
    Org,
    Link,
}
impl ShareSubject {
    pub fn member(user: Uuid, org: Uuid) -> Self {
        Self::Member(format!("{user}@{org}"))
    }
    pub fn validate(&self, role: u8) -> Result<()> {
        let cap = if matches!(self, Self::Member(_) | Self::Email(_)) {
            4
        } else {
            3
        };
        if role > cap || (role == 0 && !matches!(self, Self::Org | Self::Link)) {
            return Err(Error::new(
                "INVALID_INPUT",
                "Member/email roles are view..manage; group roles are view..edit; organization/link roles are none..edit",
            ));
        }
        match self {
            Self::Member(id) => {
                let (user, org) = id.split_once('@').ok_or_else(|| {
                    Error::new(
                        "INVALID_INPUT",
                        "Member identity must be UUID@organization-UUID",
                    )
                })?;
                if Uuid::parse_str(user).is_err() || Uuid::parse_str(org).is_err() {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        "Member identity must be UUID@organization-UUID",
                    ));
                }
            }
            Self::Email(email)
                if !email.contains('@')
                    || email.trim() != email
                    || email.chars().any(char::is_control) =>
            {
                return Err(Error::new("INVALID_INPUT", "Invalid email subject"));
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRuleInput {
    #[serde(flatten)]
    pub subject: ShareSubject,
    pub role: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShareChange {
    RuleSet {
        #[serde(flatten)]
        rule: ShareRuleInput,
    },
    RuleRemove {
        rule_id: Uuid,
    },
}
impl ShareChange {
    pub fn validate(&self) -> Result<()> {
        if let Self::RuleSet { rule } = self {
            rule.subject.validate(rule.role)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRule {
    pub rule_id: Option<String>,
    pub row_type: String,
    pub subject_type: Option<String>,
    pub subject_id: Option<String>,
    pub display: Option<String>,
    pub role: u8,
    pub removable: bool,
    pub max_role: u8,
    pub inherited_from: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareSettings {
    pub dentry_id: Uuid,
    pub owner_org_id: Option<Uuid>,
    pub link_url: Option<String>,
    pub token: Option<String>,
    pub my_access: u8,
    pub is_creator: bool,
    pub can_manage: bool,
    pub rules: Vec<ShareRule>,
    pub visibility_summary: Value,
    pub external_sharing: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareImpact {
    pub gain_count: u64,
    pub lose_count: u64,
    pub retained: Vec<Value>,
    pub retained_count: u64,
    pub link_role_before: u8,
    pub link_role_after: u8,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovedShareRule {
    pub removed: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearedShareSettings {
    pub removed_grants: u64,
    pub link_role: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareResolution {
    /// When true, the caller must explicitly claim before resolving again.
    pub claimable: bool,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareClaimed {
    pub claimed: bool,
    pub area: String,
    pub dentry: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone)]
pub enum ShareClaimTarget {
    Token(String),
    Resource(Uuid),
}
