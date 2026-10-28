use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use virse_api::{Client, DownloadInfo, Error, Result};

pub async fn save(
    client: &Client,
    id: Uuid,
    org: Uuid,
    canvas: Uuid,
    output: &Path,
) -> Result<u64> {
    if output.try_exists().map_err(crate::storage::io_error)? {
        return Err(Error::new(
            "OUTPUT_EXISTS",
            "Output already exists; choose another path",
        ));
    }
    let info = client.download_info(id, org, canvas).await?;
    save_info(client, &info, output).await
}

pub async fn save_info(client: &Client, info: &DownloadInfo, output: &Path) -> Result<u64> {
    if output.try_exists().map_err(crate::storage::io_error)? {
        return Err(Error::new(
            "OUTPUT_EXISTS",
            "Output already exists; choose another path",
        ));
    }
    let content = info.content()?;
    let checksum = content
        .as_ref()
        .and_then(|content| content.checksum.as_ref());
    if checksum.is_some_and(|checksum| {
        checksum.algorithm != "sha256"
            || checksum.value.len() != 64
            || !checksum.value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(Error::new(
            "INVALID_RESPONSE",
            "Invalid download checksum metadata",
        ));
    }
    let response = client.download(info).await?;
    let expected = response.content_length();
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = tempfile::NamedTempFile::new_in(parent).map_err(crate::storage::io_error)?;
    let mut file = tokio::fs::File::from_std(
        temporary
            .as_file()
            .try_clone()
            .map_err(crate::storage::io_error)?,
    );
    let mut stream = response.bytes_stream();
    let mut size = 0u64;
    let mut digest = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            Error::new(
                "DOWNLOAD_FAILED",
                "Download interrupted; no output file was published",
            )
        })?;
        file.write_all(&chunk)
            .await
            .map_err(crate::storage::io_error)?;
        size += chunk.len() as u64;
        if checksum.is_some() {
            digest.update(&chunk);
        }
    }
    if expected.is_some_and(|n| n != size) {
        return Err(Error::new("DOWNLOAD_FAILED", "Incomplete download"));
    }
    if content
        .as_ref()
        .and_then(|content| content.size_bytes)
        .is_some_and(|n| n != size)
        || checksum.is_some_and(|checksum| {
            !checksum
                .value
                .eq_ignore_ascii_case(&format!("{:x}", digest.finalize()))
        })
    {
        return Err(Error::new(
            "DOWNLOAD_INTEGRITY_FAILED",
            "Downloaded bytes do not match the asset metadata; no output file was published",
        ));
    }
    file.sync_all().await.map_err(crate::storage::io_error)?;
    drop(file);
    temporary
        .persist_noclobber(output)
        .map_err(|e| crate::storage::io_error(e.error))?;
    Ok(size)
}
