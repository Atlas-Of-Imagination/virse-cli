use clap::{Args, Subcommand, ValueEnum};
use uuid::Uuid;
use virse_api::{Error, Result, ShareRuleInput, ShareSubject};

#[derive(Clone, Copy, ValueEnum)]
pub enum Role {
    None,
    View,
    Use,
    Edit,
    Manage,
}
impl Role {
    pub fn number(self) -> u8 {
        match self {
            Self::None => 0,
            Self::View => 1,
            Self::Use => 2,
            Self::Edit => 3,
            Self::Manage => 4,
        }
    }
}
#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct Audience {
    /// User UUID, or UUID@organization-UUID. Bare UUID uses the selected --org.
    #[arg(long)]
    pub member: Option<String>,
    #[arg(long)]
    pub group: Option<Uuid>,
    /// Create/update a pending email invitation; acceptance is a separate action.
    #[arg(long)]
    pub email: Option<String>,
    /// Members of the resource's owning organization.
    #[arg(long)]
    pub org_members: bool,
    /// Logged-in users holding the link (not anonymous access).
    #[arg(long)]
    pub link: bool,
}
impl Audience {
    pub fn rule(&self, org: Uuid, role: Role) -> Result<ShareRuleInput> {
        let subject = if let Some(member) = &self.member {
            if member.contains('@') {
                ShareSubject::Member(member.clone())
            } else {
                ShareSubject::member(
                    Uuid::parse_str(member)
                        .map_err(|_| Error::new("INVALID_INPUT", "Invalid member UUID"))?,
                    org,
                )
            }
        } else if let Some(group) = self.group {
            ShareSubject::Group(group)
        } else if let Some(email) = &self.email {
            ShareSubject::Email(email.trim().to_lowercase())
        } else if self.org_members {
            ShareSubject::Org
        } else if self.link {
            ShareSubject::Link
        } else {
            return Err(Error::new("INVALID_INPUT", "Choose a permission audience"));
        };
        let rule = ShareRuleInput {
            subject,
            role: role.number(),
        };
        rule.subject.validate(rule.role)?;
        Ok(rule)
    }
}
#[derive(Subcommand)]
pub enum Permissions {
    /// View the complete rule list and inherited access.
    #[command(visible_alias = "list")]
    Get { target: String },
    /// Set one audience's role; --dry-run previews without changing permissions.
    Set {
        target: String,
        #[command(flatten)]
        audience: Audience,
        #[arg(long, value_enum)]
        role: Role,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove one local rule using its ID from permissions get.
    Remove {
        target: String,
        rule_id: Uuid,
        #[arg(long)]
        dry_run: bool,
    },
    /// Clear this resource's local rules and link role; inherited access remains.
    Clear {
        target: String,
        #[arg(long)]
        dry_run: bool,
    },
}
#[derive(Subcommand)]
pub enum Share {
    /// Get the resource address without granting access.
    Link {
        target: String,
        /// Optional frontend origin for producing an absolute browser URL.
        #[arg(long)]
        web_origin: Option<String>,
    },
    /// Resolve a share token or browser URL under the selected organization.
    Resolve { link: String },
    /// Explicitly accept an invitation into the selected --org.
    Claim {
        #[arg(required_unless_present = "resource", conflicts_with = "resource")]
        link: Option<String>,
        #[arg(long)]
        resource: Option<Uuid>,
    },
}
