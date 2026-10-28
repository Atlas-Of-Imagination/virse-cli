use crate::{
    canvas_edit::{Context, selected_limit},
    print,
    resource_paths::Resolver,
    storage::Store,
};
use serde_json::{Value, json};
use std::{fs::File, io::Write, path::Path};
use uuid::Uuid;
use virse_api::{AccessContext, Client, DownloadInfo, EntityKind, Error, Result};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

struct Entry {
    label: String,
    resource: Option<Uuid>,
    asset: Option<Uuid>,
    element: Option<Uuid>,
    error: Option<Error>,
    skipped: bool,
}
fn io<E>(_: E) -> Error {
    Error::new("LOCAL_IO_ERROR", "ZIP file operation failed")
}
fn name(filename: &str, index: usize) -> String {
    let leaf = filename.rsplit(['/', '\\']).next().unwrap_or("file");
    let safe: String = leaf
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '\0'))
        .collect();
    let safe = safe.trim_matches('.');
    format!(
        "{:03}-{}",
        index + 1,
        if safe.is_empty() { "file" } else { safe }
    )
}
pub async fn pack_resources(
    client: &Client,
    _store: &Store,
    backend: &str,
    org: Uuid,
    targets: &[String],
    output: &Path,
    compact: bool,
) -> Result<u8> {
    if targets.is_empty() || targets.len() > 200 {
        return Err(Error::new("INVALID_INPUT", "Choose 1..200 resource files"));
    }
    let mut resolver = Resolver::new(client, org);
    let mut entries = vec![];
    for target in targets {
        entries.push(match resolver.item(target).await {
            Ok(item) => Entry {
                label: target.clone(),
                resource: Some(item.id),
                asset: None,
                element: None,
                error: None,
                skipped: false,
            },
            Err(error) => Entry {
                label: target.clone(),
                resource: None,
                asset: None,
                element: None,
                error: Some(error),
                skipped: false,
            },
        });
    }
    pack(client, backend, org, None, entries, output, compact).await
}
pub async fn pack_canvas(
    ctx: &Context<'_>,
    ids: &[Uuid],
    group: Option<Uuid>,
    output: &Path,
) -> Result<u8> {
    let (ids, _) = selected_limit(ctx, ids, group, 200).await?;
    let mut entries = vec![];
    for id in ids {
        let mut entry = Entry {
            label: id.to_string(),
            resource: None,
            asset: None,
            element: Some(id),
            error: None,
            skipped: false,
        };
        match ctx
            .client
            .canvas_entity(ctx.canvas, ctx.org, EntityKind::Element, id)
            .await
        {
            Ok(node) => {
                entry.label = node["name"].as_str().unwrap_or("canvas-media").into();
                entry.asset = node["content_ref"]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok());
                if node["content_ref"]["kind"] != "asset"
                    || !matches!(
                        node["type"].as_str(),
                        Some("image" | "video" | "3d" | "document")
                    )
                {
                    entry.skipped = true;
                    entry.asset = None;
                }
            }
            Err(error) => entry.error = Some(error),
        }
        entries.push(entry);
    }
    pack(
        ctx.client,
        ctx.backend,
        ctx.org,
        Some(ctx.canvas),
        entries,
        output,
        ctx.compact,
    )
    .await
}
async fn descriptor(
    client: &Client,
    org: Uuid,
    canvas: Option<Uuid>,
    entry: &Entry,
) -> Result<DownloadInfo> {
    if let Some(id) = entry.resource {
        return client.resource_download_info(org, id).await;
    }
    let asset = entry
        .asset
        .ok_or_else(|| Error::new("NOT_DOWNLOADABLE", "Selected entry has no byte asset"))?;
    client
        .download_info_in(
            asset,
            &AccessContext {
                org_id: org,
                dentry_id: None,
                canvas_space_id: canvas,
            },
        )
        .await
}
async fn pack(
    client: &Client,
    backend: &str,
    org: Uuid,
    canvas: Option<Uuid>,
    entries: Vec<Entry>,
    output: &Path,
    compact: bool,
) -> Result<u8> {
    if output.try_exists().map_err(crate::storage::io_error)? {
        return Err(Error::new("OUTPUT_EXISTS", "Choose a new ZIP output path"));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    let folder = tempfile::tempdir_in(parent).map_err(io)?;
    let mut zip = ZipWriter::new(temp);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    let mut results = vec![];
    let mut files = 0;
    for (index, entry) in entries.into_iter().enumerate() {
        let mut item = json!({"source":entry.label,"resource_id":entry.resource,"asset_id":entry.asset,"element_id":entry.element});
        if let Some(error) = entry.error {
            item["outcome"] = json!("failed");
            item["error"] = json!(error);
            results.push(item);
            continue;
        }
        if entry.skipped {
            item["outcome"] = json!("skipped");
            item["reason"] = json!("Inline/non-media content is not a file export");
            results.push(item);
            continue;
        }
        eprintln!("Downloading archive item {}", index + 1);
        let result = async {
            let info = descriptor(client, org, canvas, &entry).await?;
            let local = folder.path().join(index.to_string());
            let bytes = crate::download::save_info(client, &info, &local).await?;
            Ok::<_, Error>((info, local, bytes))
        }
        .await;
        match result {
            Ok((info, local, bytes)) => {
                let filename = name(&info.filename, index);
                zip.start_file(&filename, options).map_err(io)?;
                std::io::copy(&mut File::open(&local).map_err(io)?, &mut zip).map_err(io)?;
                std::fs::remove_file(local).map_err(io)?;
                item["outcome"] = json!("succeeded");
                item["filename"] = json!(filename);
                item["bytes"] = json!(bytes);
                item["content"] = info.extra.get("content").cloned().unwrap_or(Value::Null);
                item["source_version"] = json!(info.source_version());
                item["object_version"] = json!(info.object_version());
                files += 1;
            }
            Err(error) => {
                item["outcome"] = json!("failed");
                item["error"] = json!(error);
            }
        }
        results.push(item);
    }
    let failed = results.iter().filter(|v| v["outcome"] == "failed").count();
    let skipped = results.iter().filter(|v| v["outcome"] == "skipped").count();
    let manifest = json!({"backend":backend,"org_id":org,"canvas_id":canvas,"files":files,"failed":failed,"skipped":skipped,"items":results});
    zip.start_file("manifest.json", options).map_err(io)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(io)?)
        .map_err(io)?;
    let zip = zip.finish().map_err(io)?;
    zip.as_file().sync_all().map_err(io)?;
    zip.persist_noclobber(output).map_err(io)?;
    print(&json!({"path":output,"manifest":manifest}), compact)?;
    Ok(if failed == 0 && files > 0 { 0 } else { 6 })
}
