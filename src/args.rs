use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "virse", version, about = "Virse canvas and generation client")]
pub struct Cli {
    #[arg(long, global = true)]
    /// Emit compact JSON; watch emits one event per line.
    pub json: bool,
    #[arg(long, global = true, default_value = "default")]
    pub profile: String,
    #[arg(long, global = true)]
    pub canvas: Option<Uuid>,
    #[arg(long, global = true)]
    pub org: Option<Uuid>,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand)]
pub enum Command {
    /// Discover personal and team workspaces.
    Space {
        #[command(subcommand)]
        command: crate::discovery::Space,
    },
    /// Read available models and manage personal favorites.
    Model {
        #[command(subcommand)]
        command: crate::discovery::Model,
    },
    /// Read this space's balance, budget and license information.
    Balance,
    /// Create, publish, manage and run intelligent tools.
    Tool {
        #[command(subcommand)]
        command: crate::tool_args::Tool,
    },
    /// Browse and manage resources using paths or IDs.
    Resource {
        #[command(subcommand)]
        command: crate::resource_args::Resource,
    },
    /// Manage API token credentials for the current backend/profile.
    Auth {
        #[command(subcommand)]
        command: Auth,
    },
    /// Read or set default organization and canvas.
    Config {
        #[command(subcommand)]
        command: Config,
    },
    /// Read canvases and their persisted elements.
    Canvas {
        #[command(subcommand)]
        command: Canvas,
    },
    /// Submit and observe asynchronous resource generation.
    Generation {
        #[command(subcommand)]
        command: Generation,
    },
    /// Inspect or download generated assets.
    Asset {
        #[command(subcommand)]
        command: Asset,
    },
}
#[derive(Subcommand)]
pub enum Auth {
    Login {
        #[arg(long)]
        token_stdin: bool,
    },
    Status,
    Logout,
}
#[derive(Clone, ValueEnum)]
pub enum ConfigKey {
    Canvas,
    Org,
}
#[derive(Subcommand)]
pub enum Config {
    Get,
    Set { key: ConfigKey, value: Uuid },
}
#[derive(Subcommand)]
pub enum Canvas {
    /// Create a canvas in an existing resource folder.
    Create(crate::discovery::CreateCanvas),
    /// List the media/document content projection, separate from canvas layout.
    Contents(crate::discovery::Contents),
    /// Place an existing image, video, GLB, Word or PDF asset on the canvas.
    Add(crate::media_args::CanvasAdd),
    List {
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=200))]
        limit: u16,
        #[arg(long)]
        cursor: Option<String>,
    },
    Get {
        id: Uuid,
    },
}
#[derive(Args)]
pub struct Submission {
    #[arg(long)]
    pub idempotency_key: Option<String>,
    #[arg(long)]
    /// Wait for the task; disconnecting does not cancel generation.
    pub wait: bool,
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..))]
    /// Maximum wait duration in seconds.
    pub timeout: u64,
}
#[derive(Subcommand)]
pub enum Generation {
    /// Extend a prepared mask image using a server-authorized asset reference.
    Outpaint(crate::shortcuts::Outpaint),
    /// Generate text into a new or existing text element.
    Text(crate::shortcuts::Text),
    /// Remove an image background using the V2 task pipeline.
    RemoveBg(crate::shortcuts::Process),
    /// Upscale an image using the requested model and scale.
    Upscale(crate::shortcuts::Upscale),
    /// Discover operations and their input JSON Schemas.
    Operations,
    /// Submit operation/input/outputs JSON from a file, or - for stdin.
    Submit {
        #[arg(long)]
        file: String,
        #[command(flatten)]
        options: Submission,
    },
    /// Create an image placeholder and submit single-image generation.
    Image(crate::media_args::PromptMedia),
    /// Create a video placeholder and submit asynchronous video generation.
    Video(crate::media_args::PromptMedia),
    /// Create a 3D placeholder and generate a model from text or image references.
    #[command(name = "3d", visible_alias = "model3d")]
    Model3d(crate::media_args::Model3d),
    Get {
        id: Uuid,
    },
    /// Observe snapshots over WebSocket until completion or timeout.
    Watch {
        id: Uuid,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
    },
    /// Ask the server to cancel a task; running tasks may reject cancellation.
    Cancel {
        id: Uuid,
    },
}
#[derive(Subcommand)]
pub enum Asset {
    /// Save an authorized preview or display representation of an asset.
    Preview {
        id: Uuid,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "preview")]
        representation: crate::discovery::Representation,
    },
    Get {
        id: Uuid,
    },
    Download {
        id: Uuid,
        #[arg(long)]
        output: PathBuf,
    },
}
