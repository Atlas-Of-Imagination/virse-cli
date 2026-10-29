use crate::{
    generation::SubmissionContext, media_args::MediaOptions, shortcuts::ImageSource,
    storage::digest, workflow_state as state,
};
use clap::Args;
use futures_util::StreamExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;
use virse_api::{
    AssetType, AssetUploadFile, EntityKind, Error, GenerationReference, MAX_IMAGE_MASK_BYTES,
    ModelMode, ReferenceSource, Result, SubmitTask,
};

#[derive(Args)]
pub struct Edit {
    #[arg(long)]
    pub prompt: String,
    #[command(flatten)]
    pub source: ImageSource,
    /// PNG selection: white edits, black preserves. Use the source image's pixel dimensions.
    #[arg(long, conflicts_with = "mask_asset")]
    pub mask_file: Option<PathBuf>,
    /// Download an authorized PNG mask asset, then upload it through the existing mask protocol.
    #[arg(long)]
    pub mask_asset: Option<Uuid>,
    #[command(flatten)]
    pub options: MediaOptions,
}
fn invalid(message: &str) -> Error {
    Error::new("INVALID_INPUT", message)
}
fn uncertain(key: &str) -> Error {
    Error {
        idempotency_key: Some(key.into()),
        ..Error::new(
            "MASK_UPLOAD_UNCERTAIN",
            "Mask upload result is unknown. No generation was submitted. This upload API has no receipt; the same key will not automatically upload again. Check the result before explicitly starting a new attempt with a new key",
        )
    }
}

async fn mask_asset(ctx: &SubmissionContext<'_>, org: Uuid, id: Uuid) -> Result<Vec<u8>> {
    let access = ctx
        .client
        .resolve_asset_context(Some(org), Some(ctx.canvas))
        .await?;
    let asset = ctx.client.asset_detail(id, &access).await?;
    if asset.asset_type != AssetType::Image {
        return Err(invalid("Mask asset must be an image"));
    }
    let info = ctx.client.download_info_in(id, &access).await?;
    info.check_revision(&json!(asset))?;
    let response = ctx.client.download(&info).await?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_IMAGE_MASK_BYTES as u64)
    {
        return Err(invalid("Mask exceeds 50 MiB"));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| Error::new("DOWNLOAD_INCOMPLETE", "Mask download interrupted"))?;
        if bytes.len() + chunk.len() > MAX_IMAGE_MASK_BYTES {
            return Err(invalid("Mask exceeds 50 MiB"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if asset
        .content
        .size_bytes
        .is_some_and(|n| n != bytes.len() as u64)
    {
        return Err(Error::new(
            "DOWNLOAD_INCOMPLETE",
            "Mask byte size does not match metadata",
        ));
    }
    if let Some(checksum) = &asset.content.checksum
        && checksum.algorithm == "sha256"
        && checksum.value != format!("{:x}", Sha256::digest(&bytes))
    {
        return Err(Error::new(
            "DOWNLOAD_INCOMPLETE",
            "Mask checksum does not match metadata",
        ));
    }
    Ok(bytes)
}

pub async fn run(args: &Edit, ctx: &SubmissionContext<'_>, org: Uuid) -> Result<u8> {
    let options = &args.options;
    let parameters = crate::generation::read_media_parameters(options).await?;
    let key = state::key(&options.submission.idempotency_key)?;
    // 本地文件只读取一次；该次上传与摘要始终来自同一份字节。
    let local_mask = args
        .mask_file
        .as_ref()
        .map(|p| crate::resource_upload::read_file(p, MAX_IMAGE_MASK_BYTES as u64, false))
        .transpose()?;
    let mask_hash = local_mask
        .as_ref()
        .map(|b| format!("{:x}", Sha256::digest(b)));
    let masked = args.mask_file.is_some() || args.mask_asset.is_some();
    let output = crate::shortcuts::Output {
        target_element: options.target_element,
        x: options.x,
        y: options.y,
        width: options.width,
        height: options.height,
    }
    .target((1024.0, 1024.0));
    let refs: Vec<_> = options
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
    let mut body = SubmitTask::image_edit(
        &options.model,
        &args.prompt,
        args.source.reference(),
        refs.clone(),
        parameters.clone(),
        None,
        output,
    )?;
    body.input.insert("organization_id".into(), json!(org));
    let user = state::user(ctx.client).await?;
    let scope = json!([ctx.backend, user, org, ctx.canvas, "image-edit", key]);
    let name = format!("image-edit-{}.json", digest(&scope.to_string()));
    let intent = json!({"request":body, "mask_file":args.mask_file,"mask_asset":args.mask_asset,"mask_sha256":mask_hash});
    let _guard = state::exclusive(ctx.store, &scope)?;
    let mut record: Value = ctx.store.read(&name)?;
    if !record.is_null() {
        if record["intent"] != intent {
            return Err(invalid(
                "This key belongs to different edit inputs or mask bytes",
            ));
        }
        if record["phase"] == "rejected" {
            return Err(Error {
                status: record["status"]
                    .as_u64()
                    .and_then(|s| u16::try_from(s).ok()),
                idempotency_key: Some(key.into()),
                ..Error::new(
                    "MASK_UPLOAD_REJECTED",
                    "Mask upload was rejected; correct the inputs or permissions and use a new key",
                )
            });
        }
        if record["phase"] != "ready" {
            return Err(uncertain(&key));
        }
        body = serde_json::from_value(record["request"].clone())
            .map_err(|_| invalid("Invalid saved image edit"))?;
    } else {
        let scenario = if masked { "inpaint" } else { "image-to-image" };
        let catalog = ctx
            .client
            .model_catalog(org, &[ModelMode::Image], false)
            .await?;
        if !catalog.models.iter().any(|m| {
            m.model_key == options.model
                && m.mode == "image"
                && m.extra
                    .get("supported_scenarios")
                    .and_then(Value::as_array)
                    .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(scenario)))
        }) {
            return Err(invalid(
                "Selected model is unavailable or does not support this image edit scenario",
            ));
        }
        let mut source = args.source.reference();
        if let Some(id) = args.source.source_element {
            let node = ctx
                .client
                .canvas_entity(ctx.canvas, org, EntityKind::Element, id)
                .await?;
            if node["type"] != "image" {
                return Err(invalid("Source element must be an image"));
            }
            let asset = node
                .pointer("/content_ref/id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| invalid("Source image has no ready asset"))?;
            source = ReferenceSource::CanvasElement {
                element_id: id,
                expected_asset_id: Some(asset),
                expected_text: None,
            };
            if let Some(out) = body.outputs[0].create_element.as_mut() {
                let number = |path| {
                    node.pointer(path)
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| invalid("Source image has invalid layout"))
                };
                let width = number("/layout/size/width")?;
                let height = number("/layout/size/height")?;
                out.position.x = options
                    .x
                    .unwrap_or(number("/layout/position/x")? + width + 40.0);
                out.position.y = options.y.unwrap_or(number("/layout/position/y")?);
                out.size.width = options.width.unwrap_or(width);
                out.size.height = options.height.unwrap_or(height);
            }
        }
        body = SubmitTask::image_edit(
            &options.model,
            &args.prompt,
            source,
            refs,
            parameters,
            None,
            body.outputs.remove(0),
        )?;
        body.input.insert("organization_id".into(), json!(org));
        // 上传前校验整个生成请求；不会因布局/参数无效先留下中间文件。
        body.validate()?;
        if masked {
            let bytes = match local_mask {
                Some(bytes) => bytes,
                None => mask_asset(ctx, org, args.mask_asset.expect("mask source present")).await?,
            };
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(invalid("Mask must be a PNG"));
            }
            record = json!({"intent":intent,"phase":"uploading"});
            ctx.store.save(&name, &record)?;
            eprintln!("Edit key: {key}");
            let mask = match ctx
                .client
                .upload_image_mask(AssetUploadFile::new("mask.png", bytes))
                .await
            {
                Ok(mask) => mask,
                Err(mut e)
                    if !e.retryable
                        && e.status
                            .is_some_and(|s| (400..500).contains(&s) && s != 408)
                        && e.code != "INVALID_RESPONSE" =>
                {
                    ctx.store.save(
                        &name,
                        &json!({"intent":intent,"phase":"rejected","status":e.status}),
                    )?;
                    e.idempotency_key = Some(key.into());
                    return Err(e);
                }
                Err(e) => {
                    return Err(Error {
                        details: Some(Box::new(json!({"cause":e.code,"status":e.status}))),
                        ..uncertain(&key)
                    });
                }
            };
            // 上传响应固定后才提交任务；请求丢失/重放不再上传蒙版。
            body = body.with_image_mask(&mask)?;
        }
        ctx.store.save(
            &name,
            &json!({"intent":intent,"phase":"ready","request":body}),
        )?;
    }
    let submission = crate::args::Submission {
        idempotency_key: Some(key),
        wait: options.submission.wait,
        timeout: options.submission.timeout,
    };
    ctx.submit(body, &submission).await
}
