use axum::{Json, Router, routing::get};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::process::Stdio;
use tokio::process::Command;

#[tokio::test]
async fn download_checks_checksum_and_size_before_publishing_file() {
    let id = "00000000-0000-0000-0000-000000000001";
    for (size, checksum, success) in [
        (5, format!("{:x}", Sha256::digest(b"bytes")), true),
        (5, format!("{:x}", Sha256::digest(b"wrong")), false),
        (7, format!("{:x}", Sha256::digest(b"bytes")), false),
    ] {
        let app = Router::new()
            .route(&format!("/api/v1/assets/{id}/download"),get(move || {
                let checksum = checksum.clone();
                async move { Json(json!({"request_id":"download","data":{
                    "url":"/bytes?source_version=v1","filename":"a.png","delivery":"authenticated_stream",
                    "content":{"revision":1,"size_bytes":size,"metadata_state":"complete", "checksum":{"algorithm":"sha256","value":checksum}}
                }})) }
            }))
            .route("/bytes",get(|| async { "bytes" }));
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", socket.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(socket, app).await.unwrap();
        });
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("download.png");
        let output = Command::new(env!("CARGO_BIN_EXE_virse"))
            .args([
                "--org", id, "--canvas", id, "asset", "download", id, "--output",
            ])
            .arg(&target)
            .arg("--json")
            .env("VIRSE_BACKEND_URL", base)
            .env("VIRSE_API_TOKEN", "virse_sk_download_test")
            .env("VIRSE_CONFIG_DIR", temp.path().join("config"))
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(target.exists(), success);
        if !success {
            let error: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(error["error"]["code"], "DOWNLOAD_INTEGRITY_FAILED");
        }
        server.abort();
    }
}
