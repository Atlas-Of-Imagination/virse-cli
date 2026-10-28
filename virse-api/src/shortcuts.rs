//! Small typed builders over the existing V2 generation submission protocol.
use crate::{Error, GenerationReference, OutputTarget, ReferenceSource, Result, SubmitTask};
use serde_json::{Map, json};

#[derive(Debug, Clone, Copy)]
pub enum ImageProcess {
    RemoveBackground,
    Upscale { times: u32 },
}
fn model_input(model: &str) -> Result<Map<String, serde_json::Value>> {
    if model.trim().is_empty() {
        return Err(Error::new("INVALID_INPUT", "A model key is required"));
    }
    Ok(Map::from_iter([("model".into(), model.into())]))
}
impl SubmitTask {
    pub fn text(
        model: &str,
        prompt: &str,
        references: Vec<GenerationReference>,
        mode: Option<&str>,
        output: OutputTarget,
    ) -> Result<Self> {
        if prompt.trim().is_empty()
            || references.len() > 100
            || mode.is_some_and(|s| s.trim().is_empty())
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Provide a prompt and at most 100 references",
            ));
        }
        let mut input = model_input(model)?;
        input.insert("user_query".into(), prompt.into());
        let refs = references
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                Ok((
                    format!("${}", i + 1),
                    serde_json::to_value(r).map_err(|_| Error::protocol())?,
                ))
            })
            .collect::<Result<Map<String, _>>>()?;
        input.insert("references".into(), refs.into());
        if let Some(mode) = mode {
            input.insert("mode".into(), mode.into());
        }
        let request = Self {
            operation: "text_generate".into(),
            input,
            outputs: vec![output],
        };
        request.validate()?;
        Ok(request)
    }
    pub fn image_process(
        operation: ImageProcess,
        model: &str,
        source: ReferenceSource,
        output: OutputTarget,
    ) -> Result<Self> {
        if matches!(&source, ReferenceSource::Text { .. })
            || matches!(&source,ReferenceSource::Asset{asset_type,..} if asset_type!="image")
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Image processing requires an image source",
            ));
        }
        let mut input = model_input(model)?;
        input.insert(
            "source".into(),
            serde_json::to_value(source).map_err(|_| Error::protocol())?,
        );
        let operation = match operation {
            ImageProcess::RemoveBackground => "image_remove_bg",
            ImageProcess::Upscale { times } => {
                if times == 0 {
                    return Err(Error::new("INVALID_INPUT", "Scale must be positive"));
                }
                input.insert("upscale_times".into(), json!(times));
                "image_upscale"
            }
        };
        let request = Self {
            operation: operation.into(),
            input,
            outputs: vec![output],
        };
        request.validate()?;
        Ok(request)
    }
}

impl SubmitTask {
    /// Outpaint with an uploaded image mask. The backend resolves its protected object key.
    pub fn outpaint(
        model: &str,
        prompt: &str,
        mask: ReferenceSource,
        references: Vec<GenerationReference>,
        parameters: Option<Map<String, serde_json::Value>>,
        output: OutputTarget,
    ) -> Result<Self> {
        if references.len() > 100
            || matches!(&mask, ReferenceSource::Text { .. })
            || matches!(&mask,ReferenceSource::Asset{asset_type,..} if asset_type!="image")
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Outpaint requires an image mask and at most 100 references",
            ));
        }
        let mut input = model_input(model)?;
        input.insert("user_query".into(), prompt.into());
        input.insert("image_count".into(), 1.into());
        input.insert(
            "mask_source".into(),
            serde_json::to_value(mask).map_err(|_| Error::protocol())?,
        );
        let refs = references
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                Ok((
                    format!("${}", i + 1),
                    serde_json::to_value(r).map_err(|_| Error::protocol())?,
                ))
            })
            .collect::<Result<Map<String, _>>>()?;
        input.insert("references".into(), refs.into());
        if let Some(parameters) = parameters {
            input.insert("parameters".into(), parameters.into());
        }
        let request = Self {
            operation: "image_outpaint".into(),
            input,
            outputs: vec![output],
        };
        request.validate()?;
        Ok(request)
    }
}
