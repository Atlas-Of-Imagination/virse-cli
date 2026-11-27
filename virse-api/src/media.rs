use crate::{CatalogModel, Error, GenerationReference, OutputTarget, Result, SubmitTask};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Model3d,
    Audio,
}
impl MediaKind {
    pub fn operation(self) -> &'static str {
        match self {
            Self::Image => "image_generate",
            Self::Video => "video_generate",
            Self::Model3d => "three_d_generate",
            Self::Audio => "audio_generate",
        }
    }
    pub fn default_size(self) -> (f64, f64) {
        match self {
            Self::Image => (1024.0, 1024.0),
            Self::Video => (512.0, 288.0),
            Self::Model3d => (512.0, 512.0),
            // 与后端画布命令的 audio 节点默认尺寸一致
            Self::Audio => (1024.0, 320.0),
        }
    }
}
/// 目录轨道缺省时的单轨名（与后端 AUDIO_DEFAULT_TRACKS 一致）。
const AUDIO_DEFAULT_TRACKS: [&str; 1] = ["mix"];
/// 音频模型某个任务模式的输出（目录 generation_capabilities.audio_modes[mode].outputs）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOutputs {
    /// `audio`，或 `text`（转写：占位与产物是画布文本节点）。
    pub kind: String,
    /// 一次生成的输出轨道，顺序即输出槽位内的轨道序号。
    pub tracks: Vec<String>,
}
impl AudioOutputs {
    pub fn is_text(&self) -> bool {
        self.kind == "text"
    }
}
impl CatalogModel {
    /// 与后端 get_audio_output_tracks 同算法：tracks_by 所指参数取到声明过的值时用那一组
    /// 轨道，否则用默认 tracks；轨道为空按单轨 mix 算。不是音频模型或目录没有该任务时返回 None。
    pub fn audio_outputs(
        &self,
        audio_mode: &str,
        parameters: Option<&Map<String, Value>>,
    ) -> Option<AudioOutputs> {
        if self.mode != "audio" {
            return None;
        }
        let spec = self
            .extra
            .get("generation_capabilities")?
            .get("audio_modes")?
            .get(audio_mode)?
            .as_object()?;
        let outputs = spec.get("outputs").and_then(Value::as_object);
        let names = |value: Option<&Value>| -> Vec<String> {
            value
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let tracks_by = outputs
            .and_then(|o| o.get("tracks_by"))
            .and_then(Value::as_object);
        let chosen = tracks_by.and_then(|by| {
            let param = by.get("param")?.as_str()?;
            let value = parameters?.get(param)?.as_str()?;
            let listed = by.get("values")?.get(value)?;
            Some(names(Some(listed)))
        });
        let mut tracks = chosen.unwrap_or_else(|| names(outputs.and_then(|o| o.get("tracks"))));
        if tracks.is_empty() {
            tracks = AUDIO_DEFAULT_TRACKS
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
        }
        let kind = outputs
            .and_then(|o| o.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("audio")
            .to_owned();
        Some(AudioOutputs { kind, tracks })
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
        Self::media_outputs(kind, model, prompt, references, parameters, 1, vec![output])
    }
    /// 音频生成：输出槽位数须等于 audio_count × 该任务的轨数（见 CatalogModel::audio_outputs），
    /// 否则服务端以 GENERATION_OUTPUT_COUNT_MISMATCH 拒绝。
    pub fn audio(
        model: &str,
        prompt: &str,
        references: Vec<GenerationReference>,
        parameters: Option<Map<String, Value>>,
        audio_count: u32,
        outputs: Vec<OutputTarget>,
    ) -> Result<Self> {
        Self::media_outputs(
            MediaKind::Audio,
            model,
            prompt,
            references,
            parameters,
            audio_count,
            outputs,
        )
    }
    fn media_outputs(
        kind: MediaKind,
        model: &str,
        prompt: &str,
        references: Vec<GenerationReference>,
        parameters: Option<Map<String, Value>>,
        count: u32,
        outputs: Vec<OutputTarget>,
    ) -> Result<Self> {
        if model.trim().is_empty()
            || (prompt.trim().is_empty()
                && kind != MediaKind::Audio
                && (kind != MediaKind::Model3d || references.is_empty()))
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "A model and prompt are required; 3D may omit the prompt when image references are provided, audio may omit it for tasks without text",
            ));
        }
        if kind == MediaKind::Audio {
            let mode = parameters
                .as_ref()
                .and_then(|p| p.get("audio_mode"))
                .and_then(Value::as_str);
            if mode.is_none_or(|m| m.trim().is_empty()) {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "Audio generation requires parameters.audio_mode, e.g. {\"audio_mode\":\"tts\"}",
                ));
            }
            if count == 0 {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "Audio count must be at least 1",
                ));
            }
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
            MediaKind::Audio => {
                input.insert("audio_count".into(), count.into());
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
            outputs,
        };
        request.validate()?;
        Ok(request)
    }
}
