//! Private partial files with identity/version checkpoints; never append a 200 response.
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use tokio::io::AsyncWriteExt;
use virse_api::{Client, DownloadInfo, Error, Result};
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    identity: Value,
    version: Value,
    bytes: u64,
    prefix_hash: String,
    etag: Option<String>,
    total: Option<u64>,
}
fn fail(code: &str, message: &str) -> Error {
    let mut error = Error::new(code, message);
    if code == "DOWNLOAD_INCOMPLETE" {
        error.retryable = true;
    }
    error
}
fn paths(output: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let name = output
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| fail("INVALID_INPUT", "Output must have a UTF-8 filename"))?;
    Ok((
        output.with_file_name(format!("{name}.virse-part")),
        output.with_file_name(format!("{name}.virse-part.json")),
        output.with_file_name(format!("{name}.virse-part.lock")),
    ))
}
fn checkpoint(path: &Path, value: &Checkpoint) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(crate::storage::io_error)?;
    serde_json::to_writer(&mut temp, value)
        .map_err(|_| fail("LOCAL_IO_ERROR", "Cannot save download checkpoint"))?;
    temp.flush().map_err(crate::storage::io_error)?;
    temp.as_file()
        .sync_all()
        .map_err(crate::storage::io_error)?;
    temp.persist(path)
        .map_err(|e| crate::storage::io_error(e.error))?;
    Ok(())
}
fn range(value: &str) -> Result<(u64, u64, u64)> {
    let body = value
        .strip_prefix("bytes ")
        .ok_or_else(|| fail("INVALID_RESPONSE", "Invalid Content-Range"))?;
    let (bounds, total) = body
        .split_once('/')
        .ok_or_else(|| fail("INVALID_RESPONSE", "Invalid Content-Range"))?;
    let (start, end) = bounds
        .split_once('-')
        .ok_or_else(|| fail("INVALID_RESPONSE", "Invalid Content-Range"))?;
    let numbers = [start, end, total].map(|s| {
        s.parse::<u64>()
            .map_err(|_| fail("INVALID_RESPONSE", "Invalid range numbers"))
    });
    let [start, end, total] = numbers;
    let (start, end, total) = (start?, end?, total?);
    if start > end || end >= total {
        return Err(fail("INVALID_RESPONSE", "Inconsistent Content-Range"));
    }
    Ok((start, end, total))
}
fn hash(file: &mut File, len: u64) -> Result<Sha256> {
    file.seek(SeekFrom::Start(0))
        .map_err(crate::storage::io_error)?;
    let mut remaining = len;
    let mut bytes = [0; 65536];
    let mut digest = Sha256::new();
    while remaining > 0 {
        let n = file
            .read(&mut bytes[..remaining.min(65536) as usize])
            .map_err(crate::storage::io_error)?;
        if n == 0 {
            return Err(fail(
                "DOWNLOAD_PARTIAL_CHANGED",
                "Partial file is shorter than checkpoint",
            ));
        }
        digest.update(&bytes[..n]);
        remaining -= n as u64;
    }
    Ok(digest)
}
fn verify(info: &DownloadInfo, size: u64, digest: &Sha256, total: Option<u64>) -> Result<()> {
    let content = info.content()?;
    if total.is_some_and(|n| n != size)
        || content
            .as_ref()
            .and_then(|c| c.size_bytes)
            .is_some_and(|n| n != size)
    {
        return Err(fail(
            "DOWNLOAD_INCOMPLETE",
            "Partial download retained; run again with --resume",
        ));
    }
    if let Some(sum) = content.and_then(|c| c.checksum)
        && (sum.algorithm != "sha256"
            || sum.value.len() != 64
            || !sum
                .value
                .eq_ignore_ascii_case(&format!("{:x}", digest.clone().finalize())))
    {
        return Err(fail(
            "DOWNLOAD_INTEGRITY_FAILED",
            "Downloaded bytes fail integrity validation; partial file retained",
        ));
    }
    Ok(())
}
fn publish(part: &Path, meta: &Path, output: &Path) -> Result<()> {
    fs::hard_link(part, output).map_err(crate::storage::io_error)?;
    fs::remove_file(part).map_err(crate::storage::io_error)?;
    fs::remove_file(meta).map_err(crate::storage::io_error)?;
    Ok(())
}
pub async fn save(
    client: &Client,
    info: &DownloadInfo,
    output: &Path,
    identity: &Value,
) -> Result<u64> {
    if output.try_exists().map_err(crate::storage::io_error)? {
        return Err(fail("OUTPUT_EXISTS", "Choose a new output path"));
    }
    let content = info.content()?.ok_or_else(|| {
        fail(
            "RESUME_UNSUPPORTED",
            "Server did not provide versioned content metadata",
        )
    })?;
    let source_version = info.source_version().ok_or_else(|| {
        fail(
            "RESUME_UNSUPPORTED",
            "Server did not provide a source version",
        )
    })?;
    let version = json!({"source":source_version,"object":info.object_version(),"revision":content.revision,"checksum":content.checksum});
    let (part, meta, lock) = paths(output)?;
    let guard = crate::resource_upload::open_file(&lock, true, true)?;
    guard
        .try_lock()
        .map_err(|_| fail("DOWNLOAD_BUSY", "Another process is writing this download"))?;
    let exists = part.try_exists().map_err(crate::storage::io_error)?;
    let mut state = if meta.try_exists().map_err(crate::storage::io_error)? {
        let bytes = crate::resource_upload::read_file(&meta, 65536, true)?;
        let value: Checkpoint = serde_json::from_slice(&bytes)
            .map_err(|_| fail("DOWNLOAD_PARTIAL_CHANGED", "Invalid checkpoint"))?;
        if value.identity != *identity || value.version != version {
            return Err(fail(
                "content_changed",
                "Partial file belongs to another identity or content version; choose a new output path",
            ));
        }
        if !exists {
            return Err(fail(
                "DOWNLOAD_PARTIAL_CHANGED",
                "Checkpoint has no partial file",
            ));
        }
        value
    } else {
        if exists {
            return Err(fail(
                "DOWNLOAD_PARTIAL_CHANGED",
                "Partial file has no checkpoint; choose another output path",
            ));
        }
        Checkpoint {
            identity: identity.clone(),
            version,
            bytes: 0,
            prefix_hash: format!("{:x}", Sha256::new().finalize()),
            etag: None,
            total: content.size_bytes,
        }
    };
    let mut file = crate::resource_upload::open_file(&part, true, true)?;
    let mut digest = hash(&mut file, state.bytes)?;
    if format!("{:x}", digest.clone().finalize()) != state.prefix_hash {
        return Err(fail(
            "DOWNLOAD_PARTIAL_CHANGED",
            "Partial file checksum changed",
        ));
    }
    file.set_len(state.bytes)
        .map_err(crate::storage::io_error)?;
    file.seek(SeekFrom::End(0))
        .map_err(crate::storage::io_error)?;
    if state.bytes > 0
        && state.total == Some(state.bytes)
        && (info.object_version().is_some() || content.checksum.is_some())
    {
        verify(info, state.bytes, &digest, state.total)?;
        file.sync_all().map_err(crate::storage::io_error)?;
        drop(file);
        publish(&part, &meta, output)?;
        return Ok(state.bytes);
    }
    checkpoint(&meta, &state)?;
    if state.bytes > 0
        && info.object_version().is_none()
        && content.checksum.is_none()
        && state.etag.is_none()
    {
        return Err(fail(
            "RESUME_UNSUPPORTED",
            "No stable object version, checksum or strong ETag; choose a fresh output path",
        ));
    }
    let offset = if state.total == Some(state.bytes) {
        0
    } else {
        state.bytes
    };
    let response = client
        .download_range(
            info,
            if offset > 0 { Some(offset) } else { None },
            state.etag.as_deref(),
        )
        .await?;
    let status = response.status().as_u16();
    let response_length = response.content_length();
    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .filter(|s| s.starts_with('"') && s.ends_with('"'))
        .map(str::to_owned);
    let expected = if status == 206 {
        let header = response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| fail("INVALID_RESPONSE", "206 response lacks Content-Range"))?;
        let (start, end, total) = range(header)?;
        if start != offset
            || response_length.is_some_and(|n| n != end - start + 1)
            || state.total.is_some_and(|n| n != total)
        {
            return Err(fail(
                "INVALID_RESPONSE",
                "Range does not match the partial file",
            ));
        }
        if state.etag.is_some() && etag.is_some() && state.etag != etag {
            return Err(fail(
                "content_changed",
                "Range ETag changed; refusing to append",
            ));
        }
        state.total = Some(total);
        Some(end - start + 1)
    } else if status == 200 {
        file.set_len(0).map_err(crate::storage::io_error)?;
        file.seek(SeekFrom::Start(0))
            .map_err(crate::storage::io_error)?;
        state.bytes = 0;
        digest = Sha256::new();
        state.total = content.size_bytes.or(response_length);
        response_length
    } else {
        return Err(fail("INVALID_RESPONSE", "Expected HTTP 200 or 206"));
    };
    state.etag = if status == 200 {
        etag
    } else {
        etag.or(state.etag)
    };
    state.prefix_hash = format!("{:x}", digest.clone().finalize());
    checkpoint(&meta, &state)?;
    eprintln!("Downloading from byte {}", state.bytes);
    let mut file = tokio::fs::File::from_std(file);
    let mut stream = response.bytes_stream();
    let mut received = 0_u64;
    loop {
        let next = tokio::select! {next=stream.next()=>next,_=tokio::signal::ctrl_c()=>{
            file.sync_all().await.map_err(crate::storage::io_error)?;state.prefix_hash=format!("{:x}",digest.clone().finalize());checkpoint(&meta,&state)?;return Err(fail("INTERRUPTED","Download paused; use --resume with the same output"));
        }};
        let Some(chunk) = next else {
            break;
        };
        let chunk = match chunk {
            Ok(value) => value,
            Err(_) => {
                file.sync_all().await.map_err(crate::storage::io_error)?;
                state.prefix_hash = format!("{:x}", digest.clone().finalize());
                checkpoint(&meta, &state)?;
                return Err(fail(
                    "DOWNLOAD_INCOMPLETE",
                    "Transfer interrupted; partial bytes retained for --resume",
                ));
            }
        };
        if expected.is_some_and(|n| received + chunk.len() as u64 > n) {
            return Err(fail("INVALID_RESPONSE", "Response exceeds declared range"));
        }
        file.write_all(&chunk)
            .await
            .map_err(crate::storage::io_error)?;
        digest.update(&chunk);
        state.bytes += chunk.len() as u64;
        received += chunk.len() as u64;
        file.sync_all().await.map_err(crate::storage::io_error)?;
        state.prefix_hash = format!("{:x}", digest.clone().finalize());
        checkpoint(&meta, &state)?;
    }
    if expected.is_some_and(|n| n != received) {
        return Err(fail(
            "DOWNLOAD_INCOMPLETE",
            "Short response; partial retained",
        ));
    }
    verify(info, state.bytes, &digest, state.total)?;
    file.sync_all().await.map_err(crate::storage::io_error)?;
    drop(file);
    publish(&part, &meta, output)?;
    Ok(state.bytes)
}
