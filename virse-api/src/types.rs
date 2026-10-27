use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitTask {
    pub operation: String,
    pub input: Map<String, Value>,
    pub outputs: Vec<OutputTarget>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputTarget {
    pub output_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_element: Option<NewElement>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewElement {
    pub position: Point,
    pub size: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReferenceSource {
    CanvasElement {
        element_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_asset_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_text: Option<String>,
    },
    Asset {
        asset_type: String,
        asset_id: Uuid,
    },
    Text {
        value: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationReference {
    pub source: ReferenceSource,
}

impl SubmitTask {
    pub fn validate(&self) -> Result<()> {
        let invalid = || Error::new("INVALID_INPUT", "Invalid operation or output targets");
        if self.operation.is_empty()
            || self.operation.len() > 64
            || self.outputs.is_empty()
            || self.outputs.len() > 100
        {
            return Err(invalid());
        }
        let mut indices = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for out in &self.outputs {
            if out.output_index >= self.outputs.len()
                || !indices.insert(out.output_index)
                || out.element_id.is_some() == out.create_element.is_some()
            {
                return Err(invalid());
            }
            let id = out
                .element_id
                .or_else(|| out.create_element.as_ref().and_then(|e| e.id));
            if let Some(id) = id
                && !ids.insert(id)
            {
                return Err(invalid());
            }
            if let Some(e) = &out.create_element {
                if ![e.position.x, e.position.y, e.size.width, e.size.height]
                    .iter()
                    .all(|v| v.is_finite())
                    || e.size.width <= 0.0
                    || e.size.height <= 0.0
                {
                    return Err(invalid());
                }
                if e.title.as_ref().is_some_and(|s| s.chars().count() > 255) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub project_id: String,
    pub operation: String,
    pub status: String,
    pub revision: u64,
    pub outputs: Vec<TaskOutput>,
    #[serde(default)]
    pub actions: Actions,
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub error: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Task {
    pub fn finished(&self) -> bool {
        matches!(
            self.status.as_str(),
            "succeeded" | "partially_succeeded" | "failed" | "cancelled"
        )
    }
    pub fn stop_waiting(&self) -> bool {
        self.finished() || self.status == "needs_attention"
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    pub element_id: String,
    pub output_index: usize,
    pub status: String,
    pub asset_id: Option<String>,
    pub error_code: Option<String>,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Actions {
    pub cancel: bool,
    pub retry: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadInfo {
    pub url: String,
    pub filename: String,
    pub delivery: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
