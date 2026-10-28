use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, ValueEnum)]
pub enum View {
    All,
    Mine,
    TeamShared,
    External,
    Shared,
}
impl View {
    pub fn wire(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Mine => "mine",
            Self::TeamShared => "team_shared",
            Self::External => "external",
            Self::Shared => "shared",
        }
    }
}
#[derive(Clone, ValueEnum)]
pub enum Kind {
    Folder,
    Canvas,
    Image,
    Video,
    Model3d,
    Text,
    Word,
    Pdf,
    Document,
    Aesthetic,
    Module,
    Shortcut,
}
impl Kind {
    pub fn wire(&self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Canvas => "canvas",
            Self::Image => "asset_image",
            Self::Video => "asset_video",
            Self::Model3d => "asset_3d",
            Self::Text => "asset_text",
            Self::Word => "asset_word",
            Self::Pdf => "asset_pdf",
            Self::Document => "document",
            Self::Aesthetic => "aesthetic",
            Self::Module => "module",
            Self::Shortcut => "shortcut",
        }
    }
}
#[derive(Args)]
pub struct Listing {
    /// Absolute resource path or @UUID; defaults to the current organization's root.
    #[arg(default_value = "/")]
    pub path: String,
    #[arg(long)]
    pub search: Option<String>,
    #[arg(long)]
    pub recursive: bool,
    #[arg(long, value_enum)]
    pub kind: Vec<Kind>,
    #[arg(long, value_enum, default_value = "all")]
    pub view: View,
    #[arg(long, default_value_t=100, value_parser=clap::value_parser!(u16).range(1..=200))]
    pub limit: u16,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long, value_enum, default_value = "name")]
    pub sort: crate::discovery::Sort,
    #[arg(long, value_enum, default_value = "asc")]
    pub order: crate::discovery::Order,
    #[arg(long)]
    pub created_by_me: bool,
    #[arg(long, value_enum)]
    pub access: Vec<crate::discovery::Access>,
}
#[derive(Args)]
pub struct MutationOptions {
    /// Reuse this key with the same command to recover an uncertain operation.
    #[arg(long)]
    pub idempotency_key: Option<String>,
}
#[derive(Subcommand)]
pub enum Resource {
    /// Save the authorized preview/display representation; does not change download rights.
    Preview {
        target: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "preview")]
        representation: crate::discovery::Representation,
    },
    /// Upload one local file into an existing resource folder.
    Upload(crate::resource_upload::UploadArgs),
    /// Replace a file's content using its last-read server revision.
    Replace(crate::resource_upload::ReplaceArgs),
    /// Observe organization changes using WebSocket hints and resumable cursors.
    Watch(crate::resource_watch::WatchArgs),
    /// Inspect and manage resource access rules.
    Permissions {
        #[command(subcommand)]
        command: crate::share_args::Permissions,
    },
    /// Get or resolve a resource address and accept invitations.
    Share {
        #[command(subcommand)]
        command: crate::share_args::Share,
    },
    /// List a folder's children, with optional search and cursor pagination.
    #[command(visible_alias = "list")]
    Ls(Listing),
    /// Display the visible folder hierarchy; canvases are files, not directories.
    Tree {
        #[arg(default_value = "/")]
        path: String,
        #[arg(long,default_value_t=3,value_parser=clap::value_parser!(u16).range(1..=64))]
        depth: u16,
        #[arg(long,default_value_t=2000,value_parser=clap::value_parser!(u32).range(1..=100_000))]
        max_items: u32,
        /// Include stable resource IDs alongside names.
        #[arg(long)]
        ids: bool,
    },
    /// Search recursively within a folder (or the organization root).
    Search {
        query: String,
        #[command(flatten)]
        options: Listing,
    },
    /// Inspect a resource by path or @UUID.
    Get { target: String },
    /// Download a resource file; no canvas or asset ID is required.
    Download {
        target: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Create one folder; the parent must already exist.
    Mkdir {
        path: String,
        /// Parent path or @UUID; with this option the positional argument is a name.
        #[arg(long)]
        parent: Option<String>,
    },
    /// Rename a resource without changing its parent.
    Rename {
        target: String,
        name: String,
        #[command(flatten)]
        options: MutationOptions,
    },
    /// Move into an existing folder, or / for the root.
    Move {
        #[arg(required=true,num_args=1..)]
        sources: Vec<String>,
        destination: String,
        /// Explicitly accept a change in who can access the moved resource.
        #[arg(long)]
        confirm_access_change: bool,
        #[command(flatten)]
        options: MutationOptions,
    },
    /// Copy into an existing folder, or / for the root.
    Copy {
        #[arg(required=true,num_args=1..)]
        sources: Vec<String>,
        destination: String,
        #[command(flatten)]
        options: MutationOptions,
    },
    /// Move resources (including folder contents) to the recycle bin.
    #[command(visible_alias = "rm")]
    Delete {
        #[arg(required=true,num_args=1..)]
        targets: Vec<String>,
        #[command(flatten)]
        options: MutationOptions,
    },
    /// List visible entries in the recycle bin.
    Trash {
        #[arg(long)]
        scope: Option<String>,
    },
    /// Restore entries using IDs from `resource trash`.
    Restore {
        #[arg(required=true,num_args=1..)]
        ids: Vec<String>,
        #[command(flatten)]
        options: MutationOptions,
    },
}
