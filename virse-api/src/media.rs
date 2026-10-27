use crate::{Error, GenerationReference, OutputTarget, Result, SubmitTask};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Model3d,
}
impl MediaKind {
    pub fn operation(self) -> &'static str {
        match self {
            Self::Image => "image_generate",
            Self::Video => "video_generate",
            Self::Model3d => "three_d_generate",
        }
    }
    pub fn default_size(self) -> (f64, f64) {
        match self {
            Self::Image => (1024.0, 1024.0),
            Self::Video => (512.0, 288.0),
            Self::Model3d => (512.0, 512.0),
        }
    }
}
impl SubmitTask {
    /// Build a single-output generation using the shared task/reference protocol.
    /// Model-specific parameter validation remains authoritative on the server.
    pub fn media(
        kind: MediaKind,
        model: &str,
        prompt: &str,
        references: Vec<GenerationReference>,
        parameters: Option<Map<String, Value>>,
        output: OutputTarget,
    ) -> Result<Self> {
        if model.trim().is_empty()
            || (prompt.trim().is_empty() && (kind != MediaKind::Model3d || references.is_empty()))
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "A model and prompt are required; 3D may omit the prompt when image references are provided",
            ));
        }
        if references.len() > 100 {
            return Err(Error::new(
                "INVALID_INPUT",
                "At most 100 references may be supplied; models may impose smaller limits",
            ));
        }
        let mut refs = Map::new();
        for (index, reference) in references.into_iter().enumerate() {
            refs.insert(
                format!("${}", index + 1),
                serde_json::to_value(reference)
                    .map_err(|_| Error::new("INVALID_INPUT", "Invalid reference"))?,
            );
        }
        // Keep the existing image input order stable for persisted CLI request digests.
        let mut input = Map::new();
        input.insert("model".into(), model.into());
        input.insert("user_query".into(), prompt.into());
        match kind {
            MediaKind::Image => {
                input.insert("image_count".into(), 1.into());
            }
            MediaKind::Video => {
                input.insert("video_count".into(), 1.into());
            }
            MediaKind::Model3d => {}
        }
        input.insert("references".into(), Value::Object(refs));
        if let Some(parameters) = parameters {
            input.insert("parameters".into(), Value::Object(parameters));
        }
        let request = Self {
            operation: kind.operation().into(),
            input,
            outputs: vec![output],
        };
        request.validate()?;
        Ok(request)
    }
}
