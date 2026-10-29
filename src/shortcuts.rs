use clap::Args;
use uuid::Uuid;
use virse_api::{
    GenerationReference, ImageProcess, NewElement, OutputTarget, Point, ReferenceSource, Result,
    Size, SubmitTask,
};

#[derive(Args)]
pub struct Output {
    #[arg(long,conflicts_with_all=["x","y","width","height"])]
    pub target_element: Option<Uuid>,
    #[arg(long, allow_hyphen_values = true)]
    pub x: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub y: Option<f64>,
    #[arg(long)]
    pub width: Option<f64>,
    #[arg(long)]
    pub height: Option<f64>,
}
impl Output {
    pub fn target(&self, default: (f64, f64)) -> OutputTarget {
        OutputTarget {
            output_index: 0,
            element_id: self.target_element,
            create_element: self.target_element.is_none().then(|| NewElement {
                id: None,
                title: None,
                position: Point {
                    x: self.x.unwrap_or(0.0),
                    y: self.y.unwrap_or(0.0),
                },
                size: Size {
                    width: self.width.unwrap_or(default.0),
                    height: self.height.unwrap_or(default.1),
                },
            }),
        }
    }
}
#[derive(Args)]
pub struct Text {
    #[arg(long)]
    pub model: String,
    #[arg(long)]
    pub prompt: String,
    #[arg(long)]
    pub reference: Vec<Uuid>,
    /// Optional backend text mode (validated by the server).
    #[arg(long)]
    pub mode: Option<String>,
    #[command(flatten)]
    pub output: Output,
    #[command(flatten)]
    pub submission: crate::args::Submission,
}
impl Text {
    pub fn request(&self) -> Result<SubmitTask> {
        SubmitTask::text(
            &self.model,
            &self.prompt,
            self.reference
                .iter()
                .map(|id| GenerationReference {
                    source: ReferenceSource::CanvasElement {
                        element_id: *id,
                        expected_asset_id: None,
                        expected_text: None,
                    },
                })
                .collect(),
            self.mode.as_deref(),
            self.output.target((560.0, 320.0)),
        )
    }
}
#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct ImageSource {
    /// Existing image canvas element ID.
    #[arg(long)]
    pub source_element: Option<Uuid>,
    /// Existing image asset ID (not a resource directory entry ID).
    #[arg(long)]
    pub source_asset: Option<Uuid>,
}
impl ImageSource {
    pub fn reference(&self) -> ReferenceSource {
        match self.source_element {
            Some(id) => ReferenceSource::CanvasElement {
                element_id: id,
                expected_asset_id: None,
                expected_text: None,
            },
            None => ReferenceSource::Asset {
                asset_type: "image".into(),
                asset_id: self.source_asset.expect("clap requires an image source"),
            },
        }
    }
}
#[derive(Args)]
pub struct Process {
    #[arg(long)]
    pub model: String,
    #[command(flatten)]
    pub source: ImageSource,
    #[command(flatten)]
    pub output: Output,
    #[command(flatten)]
    pub submission: crate::args::Submission,
}
impl Process {
    pub fn request(&self, operation: ImageProcess) -> Result<SubmitTask> {
        SubmitTask::image_process(
            operation,
            &self.model,
            self.source.reference(),
            self.output.target((1024.0, 1024.0)),
        )
    }
}
#[derive(Args)]
pub struct Upscale {
    #[arg(long,value_parser=clap::value_parser!(u32).range(1..))]
    pub scale: u32,
    #[command(flatten)]
    pub options: Process,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct MaskSource {
    /// Canvas image element containing the prepared expanded mask PNG.
    #[arg(long)]
    pub mask_element: Option<Uuid>,
    /// Uploaded prepared mask image asset ID; use resource upload for local files first.
    #[arg(long)]
    pub mask_asset: Option<Uuid>,
}
#[derive(Args)]
pub struct Outpaint {
    #[arg(long, default_value = "")]
    pub prompt: String,
    #[command(flatten)]
    pub mask: MaskSource,
    #[command(flatten)]
    pub options: crate::media_args::MediaOptions,
}
impl Outpaint {
    pub async fn request(&self) -> Result<SubmitTask> {
        let mask = match self.mask.mask_element {
            Some(id) => ReferenceSource::CanvasElement {
                element_id: id,
                expected_asset_id: None,
                expected_text: None,
            },
            None => ReferenceSource::Asset {
                asset_type: "image".into(),
                asset_id: self.mask.mask_asset.expect("clap requires a mask"),
            },
        };
        let options = &self.options;
        let output = Output {
            target_element: options.target_element,
            x: options.x,
            y: options.y,
            width: options.width,
            height: options.height,
        }
        .target((1024.0, 1024.0));
        let references = options
            .reference
            .iter()
            .map(|id| GenerationReference {
                source: ReferenceSource::CanvasElement {
                    element_id: *id,
                    expected_asset_id: None,
                    expected_text: None,
                },
            })
            .collect();
        SubmitTask::outpaint(
            &options.model,
            &self.prompt,
            mask,
            references,
            crate::generation::read_media_parameters(options).await?,
            output,
        )
    }
}
