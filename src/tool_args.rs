use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
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
    pub fn api(self) -> virse_api::ToolScope {
        match self {
            Self::Personal => virse_api::ToolScope::Personal,
            Self::Team => virse_api::ToolScope::Team,
        }
    }
}
#[derive(Clone, Copy, ValueEnum)]
pub enum ToolView {
    Created,
    Mine,
    Store,
}
impl ToolView {
    pub fn api(self) -> virse_api::ToolView {
        match self {
            Self::Created => virse_api::ToolView::Created,
            Self::Mine => virse_api::ToolView::Mine,
            Self::Store => virse_api::ToolView::Store,
        }
    }
}
#[derive(Args, Serialize)]
pub struct ToolVersion {
    #[arg(long, conflicts_with = "version_id")]
    /// Published version label (resolved to an immutable artifact by the backend).
    pub version: Option<String>,
    #[arg(long)]
    /// Immutable artifact ID; this is not a release ID.
    pub version_id: Option<String>,
    #[arg(long, default_value = "zh", value_parser = ["zh", "en"])]
    pub locale: String,
}
#[derive(Args)]
pub struct ToolRunArgs {
    pub id: String,
    #[command(flatten)]
    pub version: ToolVersion,
    #[arg(long, visible_alias = "inputs-file")]
    /// Input object as JSON file, or - to read stdin. May accompany --instruction.
    pub inputs: Option<String>,
    #[arg(long)]
    pub instruction: Option<String>,
    #[arg(long, conflicts_with_all = ["x", "y", "width", "height", "name"])]
    pub target_element: Option<Uuid>,
    #[arg(long, allow_hyphen_values = true)]
    pub x: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub y: Option<f64>,
    #[arg(long)]
    pub width: Option<f64>,
    #[arg(long)]
    pub height: Option<f64>,
    #[arg(long)]
    pub name: Option<String>,
    #[command(flatten)]
    pub submission: crate::args::Submission,
}
#[derive(Subcommand)]
pub enum ToolRuns {
    Get {
        id: Uuid,
    },
    Watch {
        id: Uuid,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
    },
    /// Cancel a queued run. Running runs require interrupt.
    Cancel {
        id: Uuid,
    },
    /// Request interruption; the server may still be finishing work.
    Interrupt {
        id: Uuid,
    },
    /// Dispatch a queued run again, as allowed by the server.
    Resume {
        id: Uuid,
    },
}
#[derive(Subcommand)]
pub enum ToolDraft {
    /// Abandon a known draft revision; the published version remains unchanged.
    Abandon {
        tool_id: String,
        revision_id: String,
    },
}
#[derive(Subcommand)]
pub enum Tool {
    /// Upload a cover image for use in local tool metadata.
    Cover {
        #[command(subcommand)]
        command: crate::tool_extra::Cover,
    },
    /// Create a pinned tool instance without starting a run.
    Place(crate::tool_extra::Place),
    /// Initialize a native authoring project locally (no API credentials needed).
    Init {
        directory: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// Validate a native authoring project locally.
    Validate {
        directory: PathBuf,
    },
    Publish {
        directory: PathBuf,
        #[arg(long, value_enum)]
        scope: Option<ToolScope>,
        #[arg(long)]
        resume: bool,
    },
    Pull {
        tool_id: Uuid,
        #[arg(long = "output")]
        directory: PathBuf,
        #[arg(long)]
        version: Option<String>,
    },
    List {
        #[arg(long, value_enum, default_value = "mine")]
        view: ToolView,
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=500))]
        limit: Option<u16>,
        #[arg(long)]
        offset: Option<u32>,
        #[arg(long, default_value = "zh", value_parser = ["zh", "en"])]
        locale: String,
    },
    Get {
        id: String,
        #[arg(long, default_value = "zh", value_parser = ["zh", "en"])]
        locale: String,
    },
    /// List formal releases and their immutable artifact IDs (author only).
    Versions {
        id: String,
    },
    /// Explicit recovery actions for author drafts.
    Draft {
        #[command(subcommand)]
        command: ToolDraft,
    },
    /// Change visibility of a tool you own.
    Scope {
        id: String,
        #[arg(value_enum)]
        scope: ToolScope,
    },
    Add {
        id: String,
    },
    Remove {
        id: String,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Star {
        id: String,
    },
    Unstar {
        id: String,
    },
    Delete {
        id: String,
    },
    /// Read the executable input form, pinned to an immutable artifact.
    Schema {
        id: String,
        #[command(flatten)]
        version: ToolVersion,
    },
    /// Run on an existing skill node or atomically create one.
    Run(ToolRunArgs),
    /// Query, observe or control persisted V2 tool runs.
    Runs {
        #[command(subcommand)]
        command: ToolRuns,
    },
}
