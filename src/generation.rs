use crate::{args::Submission, media_args::MediaOptions, print, storage::Store};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;
use virse_api::{
    Client, Error, GenerationReference, MediaKind, ModelMode, NewElement, OutputTarget, Point,
    ReferenceSource, Result, Size, SubmitTask, Task,
};

fn canvas_references(args: &MediaOptions) -> Vec<GenerationReference> {
    args.reference
        .iter()
        .map(|id| GenerationReference {
            source: ReferenceSource::CanvasElement {
                element_id: *id,
                expected_asset_id: None,
                expected_text: None,
            },
        })
        .collect()
}
pub async fn media_request(
    kind: MediaKind,
    prompt: &str,
    args: &MediaOptions,
) -> Result<SubmitTask> {
    let parameters = read_media_parameters(args).await?;
    let references = canvas_references(args);
    let (width, height) = kind.default_size();
    let output = OutputTarget {
        output_index: 0,
        element_id: args.target_element,
        create_element: args.target_element.is_none().then(|| NewElement {
            id: None,
            title: None,
            position: Point {
                x: args.x.unwrap_or(0.0),
                y: args.y.unwrap_or(0.0),
            },
            size: Size {
                width: args.width.unwrap_or(width),
                height: args.height.unwrap_or(height),
            },
        }),
    };
    SubmitTask::media(kind, &args.model, prompt, references, parameters, output)
}
/// 音频的输出槽位数 = count × 目录里该任务的轨数，每个槽位一个占位；转写（outputs.kind
/// = text）的占位由服务端建成文本节点。未指定位置时多个占位自上而下排列。
pub async fn audio_request(
    client: &Client,
    org: Uuid,
    prompt: &Option<String>,
    count: u32,
    args: &MediaOptions,
) -> Result<SubmitTask> {
    let parameters = read_media_parameters(args).await?;
    let mode = parameters
        .as_ref()
        .and_then(|p| p.get("audio_mode"))
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
        .ok_or_else(|| {
            Error::new(
                "INVALID_INPUT",
                "Audio generation requires parameters.audio_mode, e.g. {\"audio_mode\":\"tts\"}",
            )
        })?
        .to_owned();
    let catalog = client
        .model_catalog(org, &[ModelMode::Audio], false)
        .await?;
    let outputs = catalog
        .models
        .iter()
        .find(|m| m.model_key == args.model && m.mode == "audio")
        .and_then(|m| m.audio_outputs(&mode, parameters.as_ref()))
        .ok_or_else(|| {
            Error::new(
                "INVALID_INPUT",
                format!(
                    "Selected model is unavailable or does not support audio_mode {mode}; see model list --mode audio"
                ),
            )
        })?;
    let total = usize::try_from(count)
        .ok()
        .and_then(|n| n.checked_mul(outputs.tracks.len()))
        .filter(|n| *n <= 100)
        .ok_or_else(|| Error::new("INVALID_INPUT", "At most 100 outputs per generation"))?;
    if args.target_element.is_some() && total != 1 {
        return Err(Error::new(
            "INVALID_INPUT",
            format!("--target-element needs a single output; this task produces {total}"),
        ));
    }
    // 转写产物写回文本节点，占位尺寸照文本节点默认值
    let (width, height) = if outputs.is_text() {
        (512.0, 256.0)
    } else {
        MediaKind::Audio.default_size()
    };
    let (width, height) = (args.width.unwrap_or(width), args.height.unwrap_or(height));
    let targets = (0..total)
        .map(|index| OutputTarget {
            output_index: index,
            element_id: args.target_element,
            create_element: args.target_element.is_none().then(|| NewElement {
                id: None,
                title: None,
                position: Point {
                    x: args.x.unwrap_or(0.0),
                    y: args.y.unwrap_or(0.0) + index as f64 * (height + 40.0),
                },
                size: Size { width, height },
            }),
        })
        .collect();
    SubmitTask::audio(
        &args.model,
        prompt.as_deref().unwrap_or(""),
        canvas_references(args),
        parameters,
        count,
        targets,
    )
}
fn parse_parameters(value: &str) -> Result<serde_json::Map<String, Value>> {
    if value.len() > 1024 * 1024 {
        return Err(Error::new("INVALID_INPUT", "Parameters exceed 1 MiB"));
    }
    let value: Value = serde_json::from_str(value)
        .map_err(|_| Error::new("INVALID_INPUT", "Parameters must be a JSON object"))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| Error::new("INVALID_INPUT", "Parameters must be a JSON object"))
}

pub struct SubmissionContext<'a> {
    pub client: &'a Client,
    pub store: &'a Store,
    pub backend: &'a str,
    pub canvas: Uuid,
    pub json: bool,
}
impl SubmissionContext<'_> {
    pub async fn submit(&self, body: SubmitTask, options: &Submission) -> Result<u8> {
        body.validate()?;
        let key = options
            .idempotency_key
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if key.is_empty() || key.len() > 200 || !key.is_ascii() || key.chars().any(char::is_control)
        {
            return Err(Error::new("INVALID_INPUT", "Invalid idempotency key"));
        }
        let identity = self.client.me().await?;
        let account = identity["id"]
            .as_str()
            .ok_or_else(|| Error::new("INVALID_RESPONSE", "Identity has no user ID"))?;
        let record =
            self.store
                .prepare_submission(self.backend, account, self.canvas, &key, &body)?;
        eprintln!("Submission key: {key}");
        let mut accepted = None;
        for attempt in 0..4 {
            match self.client.submit(self.canvas, &body, &key).await {
                Ok(task) => {
                    accepted = Some(task);
                    break;
                }
                Err(e)
                    if matches!(
                        e.code.as_str(),
                        "GENERATION_TASKS_NOT_ENABLED" | "GENERATION_ADMISSION_PAUSED"
                    ) =>
                {
                    return Err(e);
                }
                Err(e) if e.retryable || e.code == "INVALID_RESPONSE" => {
                    if attempt == 3 {
                        return Err(Error {
                            retryable: true,
                            idempotency_key: Some(key.clone().into_boxed_str()),
                            ..Error::new(
                                "SUBMISSION_UNCERTAIN",
                                format!(
                                    "Acceptance unknown. Resubmit the same input with --idempotency-key {key}; do not generate a new key"
                                ),
                            )
                        });
                    }
                    tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                }
                Err(e) => return Err(e),
            }
        }
        let task =
            accepted.ok_or_else(|| Error::new("SUBMISSION_UNCERTAIN", "No accepted task"))?;
        if let Err(e) = self.store.accepted(&record, &task.task_id) {
            eprintln!(
                "{}; accepted task {} remains available",
                e.code, task.task_id
            );
        }
        if !options.wait {
            print(&task, self.json)?;
            return Ok(0);
        }
        eprintln!("Accepted task: {} (canvas {})", task.task_id, self.canvas);
        if task.stop_waiting() {
            print(&task, self.json)?;
            return Ok(task_exit(&task));
        }
        let id = Uuid::parse_str(&task.task_id)
            .map_err(|_| Error::new("INVALID_RESPONSE", "Invalid task ID"))?;
        wait(
            self.client,
            self.canvas,
            id,
            options.timeout,
            self.json,
            false,
        )
        .await
    }
}

pub fn task_exit(task: &Task) -> u8 {
    match task.status.as_str() {
        "succeeded" => 0,
        "failed" | "partially_succeeded" | "cancelled" => 6,
        "needs_attention" => 7,
        _ => 1,
    }
}

pub async fn wait(
    client: &Client,
    canvas: Uuid,
    id: Uuid,
    timeout: u64,
    json: bool,
    events: bool,
) -> Result<u8> {
    let observe = async {
        let stream = client.watch(canvas, id);
        futures_util::pin_mut!(stream);
        while let Some(result) = stream.next().await {
            let task = result?;
            if events {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string(&json!({"type":"task_snapshot","task":task}))
                            .map_err(|_| Error::new("OUTPUT_ERROR", "Cannot encode output"))?
                    );
                } else {
                    print(&task, false)?;
                }
            } else if task.stop_waiting() {
                print(&task, json)?;
            } else {
                eprintln!("Task {}: {}", task.task_id, task.status);
            }
            if task.stop_waiting() {
                return Ok(task_exit(&task));
            }
        }
        Err(Error::new(
            "OBSERVATION_UNAVAILABLE",
            "Observation ended without a result",
        ))
    };
    let result = tokio::select! {
        result = observe => result,
        _ = tokio::time::sleep(Duration::from_secs(timeout)) => Err(Error::new("WAIT_TIMEOUT", format!("Stopped waiting; task {id} on canvas {canvas} was not cancelled"))),
        _ = tokio::signal::ctrl_c() => Err(Error::new("INTERRUPTED", format!("Stopped waiting; task {id} on canvas {canvas} was not cancelled"))),
    };
    result.map_err(|mut error| {
        error.task_id = Some(id.to_string().into_boxed_str());
        error
    })
}

pub async fn read_request(file: &str) -> Result<SubmitTask> {
    let bytes = if file == "-" {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(10 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(crate::storage::io_error)?;
        bytes
    } else {
        tokio::fs::read(file)
            .await
            .map_err(crate::storage::io_error)?
    };
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(Error::new("INVALID_INPUT", "Request exceeds 10 MiB"));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("INVALID_INPUT", "Request must be valid JSON"))?;
    let request: SubmitTask = serde_json::from_value(value)
        .map_err(|_| Error::new("INVALID_INPUT", "Expected operation, input and outputs"))?;
    request.validate()?;
    Ok(request)
}

pub async fn read_media_parameters(
    args: &MediaOptions,
) -> Result<Option<serde_json::Map<String, Value>>> {
    Ok(match (&args.parameters, &args.parameters_file) {
        (Some(value), None) => Some(parse_parameters(value)?),
        (None, Some(path)) => {
            if tokio::fs::metadata(path)
                .await
                .map_err(crate::storage::io_error)?
                .len()
                > 1024 * 1024
            {
                return Err(Error::new("INVALID_INPUT", "Parameters exceed 1 MiB"));
            }
            let value = tokio::fs::read_to_string(path)
                .await
                .map_err(crate::storage::io_error)?;
            Some(parse_parameters(&value)?)
        }
        (None, None) => None,
        _ => {
            return Err(Error::new(
                "INVALID_INPUT",
                "Choose parameters or parameters-file",
            ));
        }
    })
}
