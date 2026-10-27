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
#[derive(Args)]
pub struct Image {
    #[arg(long)]
    pub model: String,
    #[arg(long)]
    pub prompt: String,
    #[arg(long)]
    /// Source element ID; repeat to bind $1, $2, ... in prompt order.
    pub reference: Vec<Uuid>,
    #[arg(long, conflicts_with_all = ["x", "y", "width", "height"])]
    /// Bind an existing element instead of creating a new placeholder.
    pub target_element: Option<Uuid>,
    #[arg(long, allow_hyphen_values = true)]
    pub x: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub y: Option<f64>,
    #[arg(long)]
    pub width: Option<f64>,
    #[arg(long)]
    pub height: Option<f64>,
    #[command(flatten)]
    pub submission: Submission,
}
#[derive(Subcommand)]
pub enum Generation {
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
    Image(Image),
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
    Get {
        id: Uuid,
    },
    Download {
        id: Uuid,
        #[arg(long)]
        output: PathBuf,
    },
}
