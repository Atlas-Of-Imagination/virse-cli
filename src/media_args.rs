use crate::args::Submission;
use clap::Args;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Args)]
pub struct MediaOptions {
    #[arg(long)]
    pub model: String,
    /// Source element IDs, mapped to $1, $2, ... in the supplied order.
    #[arg(long)]
    pub reference: Vec<Uuid>,
    /// Model-specific JSON object, e.g. {"duration":2,"resolution":"720p"}.
    #[arg(long, conflicts_with = "parameters_file")]
    pub parameters: Option<String>,
    /// Read model-specific parameters from a JSON file.
    #[arg(long)]
    pub parameters_file: Option<PathBuf>,
    /// Generate into an existing element of the matching type.
    #[arg(long,conflicts_with_all=["x","y","width","height"])]
    pub target_element: Option<Uuid>,
    #[arg(long, allow_hyphen_values = true)]
    pub x: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub y: Option<f64>,
    /// Canvas display width, not the model's output resolution.
    #[arg(long)]
    pub width: Option<f64>,
    #[arg(long)]
    pub height: Option<f64>,
    #[command(flatten)]
    pub submission: Submission,
}
#[derive(Args)]
pub struct PromptMedia {
    #[arg(long)]
    pub prompt: String,
    #[command(flatten)]
    pub options: MediaOptions,
}
#[derive(Args)]
pub struct Model3d {
    /// Optional for image-to-3D; required without references.
    #[arg(long)]
    pub prompt: Option<String>,
    #[command(flatten)]
    pub options: MediaOptions,
}
#[derive(Args)]
pub struct CanvasAdd {
    /// Existing resource path or @UUID; automatically determines the element type.
    #[arg(long, required_unless_present = "asset", conflicts_with = "asset")]
    pub resource: Option<String>,
    /// Existing asset ID; use --resource for resource-library paths.
    #[arg(long)]
    pub asset: Option<Uuid>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pub x: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub y: Option<f64>,
    #[arg(long, requires = "height")]
    pub width: Option<f64>,
    #[arg(long, requires = "width")]
    pub height: Option<f64>,
    #[arg(long)]
    pub idempotency_key: Option<String>,
}
