mod support;
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path as ApiPath, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use support::*;
#[derive(Default)]
struct Data {
    calls: Vec<HeaderMap>,
    mode: String,
    changed: bool,
    fail_first: bool,
    unversioned: bool,
}
type Shared = Arc<Mutex<Data>>;
fn info(asset: &str, revision: u64, hostile_name: bool) -> Value {
    json!({"url":format!("/bytes/{asset}?source_version=v{revision}&object_version={revision}"),"filename":if hostile_name{"../../escape.png"}else{"file.png"},"delivery":"authenticated_stream","source_version":format!("v{revision}"),"object_version":revision.to_string(),"content":{"revision":revision,"size_bytes":8,"checksum":{"algorithm":"sha256","value":format!("{:x}",Sha256::digest(b"abcdefgh"))},"metadata_state":"complete"}})
}
async fn app(state: Shared) -> (String, tokio::task::JoinHandle<()>) {
    serve(Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).route("/api/v1/assets/resolve", axum::routing::post(|Json(body):Json<Value>| async move { envelope(json!({"items": body["asset_ids"].as_array().unwrap().iter().map(|id| json!({"asset_id":id,"original":{"delivery":"authenticated_stream"}})).collect::<Vec<_>>()})) })).route("/api/v1/assets/{id}/download",get(|State(s):State<Shared>,ApiPath(id):ApiPath<String>|async move{
        if id==DEST{return (StatusCode::FORBIDDEN,Json(json!({"request_id":"no","error":{"code":"forbidden","message":"no download"}}))).into_response();}
        let data=s.lock().unwrap();let mut value=info(&id,if data.changed{2}else{1},true);
        if data.unversioned {value["delivery"]=json!("external_url");value["url"]=json!(format!("/bytes/{id}"));value["object_version"]=Value::Null;value["content"]["checksum"]=Value::Null;value["content"]["metadata_state"]=json!("partial");}
        envelope(value).into_response()
    })).route("/bytes/{id}",get(|State(s):State<Shared>,headers:HeaderMap|async move{
        if let Some(auth)=headers.get("authorization"){assert_eq!(auth,"Bearer virse_sk_workflow_test");}
        let (first,mode,n)={let mut data=s.lock().unwrap();data.calls.push(headers.clone());(data.fail_first&&data.calls.len()==1,data.mode.clone(),data.calls.len())};
        if first{
            let stream=futures_util::stream::unfold(0,|state|async move{match state{0=>Some((Ok::<_,std::io::Error>(Bytes::from_static(b"abc")),1)),1=>{tokio::time::sleep(std::time::Duration::from_millis(60)).await;Some((Err(std::io::Error::other("connection cut")),2))},_=>None}});
            return (StatusCode::OK,[("content-length","8"),("etag","\"one\"")],Body::from_stream(stream)).into_response();
        }
        if mode=="drop-validator"&&n==2 {
            let stream=futures_util::stream::unfold(0,|state|async move{match state{0=>Some((Ok::<_,std::io::Error>(Bytes::from_static(b"abc")),1)),1=>{tokio::time::sleep(std::time::Duration::from_millis(60)).await;Some((Err(std::io::Error::other("connection cut")),2))},_=>None}});
            return (StatusCode::OK,[("content-length","8")],Body::from_stream(stream)).into_response();
        }
        if headers.contains_key("range")&&mode!="full"{
            assert_eq!(headers["range"],"bytes=3-");assert_eq!(headers["if-range"],"\"one\"");
            return (StatusCode::PARTIAL_CONTENT,[("content-range",if mode=="bad"{"bytes 0-4/8"}else{"bytes 3-7/8"}),("etag","\"one\"")],"defgh").into_response();
        }
        (StatusCode::OK,[("etag","\"one\"")],"abcdefgh").into_response()
    })).route("/api/v1/resources/{id}",get(|ApiPath(id):ApiPath<String>|async move{
        envelope(json!({"resource":item(&id,"asset","asset_image",None),"kind":"asset_image","asset":{"id":if id==ONE{ASSET}else{DEST},"content":{"revision":1}},"detail":{}}))
    })).route(&format!("/v2/canvas/projects/{CANVAS}/groups/{GROUP}"),get(||async{Json(json!({"group":{"id":GROUP,"elements":[ONE,TWO],"revisions":{"entity":1}}}))}))
    .route(&format!("/v2/canvas/projects/{CANVAS}/elements/{{id}}"),get(|ApiPath(id):ApiPath<String>|async move{Json(json!({"element":{"id":id,"type":if id==ONE{"image"}else{"text"},"name":"entry","content_ref":if id==ONE{json!({"kind":"asset","id":ASSET})}else{Value::Null},"revisions":{"content":1}}}))}))
    .with_state(state)).await
}
async fn interrupt(root: &std::path::Path, base: &str) -> std::path::PathBuf {
    let path = root.join("download.bin");
    let out = run(
        root,
        base,
        &[
            "asset",
            "download",
            ASSET,
            "--output",
            path.to_str().unwrap(),
            "--resume",
        ],
    )
    .await;
    assert_eq!(
        out.status.code(),
        Some(5),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(error(&out)["error"]["code"], "DOWNLOAD_INCOMPLETE");
    assert!(!path.exists());
    let checkpoint: Value =
        serde_json::from_slice(&std::fs::read(root.join("download.bin.virse-part.json")).unwrap())
            .unwrap();
    assert_eq!(checkpoint["bytes"], 3);
    path
}
#[tokio::test]
async fn download_resumes_exact_range_and_publishes_only_after_checksum() {
    let data = Shared::default();
    data.lock().unwrap().fail_first = true;
    let (base, server) = app(data.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = interrupt(dir.path(), &base).await;
    value(
        &run(
            dir.path(),
            &base,
            &[
                "asset",
                "download",
                ASSET,
                "--output",
                path.to_str().unwrap(),
                "--resume",
            ],
        )
        .await,
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"abcdefgh");
    assert!(!dir.path().join("download.bin.virse-part").exists());
    assert!(!dir.path().join("download.bin.virse-part.json").exists());
    assert_eq!(data.lock().unwrap().calls.len(), 2);
    server.abort();
}
#[tokio::test]
async fn full_200_on_range_request_restarts_instead_of_appending() {
    let data = Shared::default();
    {
        let mut d = data.lock().unwrap();
        d.fail_first = true;
        d.mode = "full".into();
    }
    let (base, server) = app(data).await;
    let dir = tempfile::tempdir().unwrap();
    let path = interrupt(dir.path(), &base).await;
    value(
        &run(
            dir.path(),
            &base,
            &[
                "asset",
                "download",
                ASSET,
                "--output",
                path.to_str().unwrap(),
                "--resume",
            ],
        )
        .await,
    );
    assert_eq!(std::fs::read(path).unwrap(), b"abcdefgh");
    server.abort();
}
#[tokio::test]
async fn changed_version_and_corrupt_prefix_never_mix_bytes() {
    let data = Shared::default();
    data.lock().unwrap().fail_first = true;
    let (base, server) = app(data.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = interrupt(dir.path(), &base).await;
    data.lock().unwrap().changed = true;
    let args = [
        "asset",
        "download",
        ASSET,
        "--output",
        path.to_str().unwrap(),
        "--resume",
    ];
    assert_eq!(
        error(&run(dir.path(), &base, &args).await)["error"]["code"],
        "content_changed"
    );
    assert_eq!(data.lock().unwrap().calls.len(), 1);
    data.lock().unwrap().changed = false;
    std::fs::write(dir.path().join("download.bin.virse-part"), b"bad").unwrap();
    assert_eq!(
        error(&run(dir.path(), &base, &args).await)["error"]["code"],
        "DOWNLOAD_PARTIAL_CHANGED"
    );
    assert_eq!(data.lock().unwrap().calls.len(), 1);
    assert!(!path.exists());
    server.abort();
}
#[tokio::test]
async fn wrong_content_range_is_rejected_before_append() {
    let data = Shared::default();
    {
        let mut d = data.lock().unwrap();
        d.fail_first = true;
        d.mode = "bad".into();
    }
    let (base, server) = app(data).await;
    let dir = tempfile::tempdir().unwrap();
    let path = interrupt(dir.path(), &base).await;
    assert_eq!(
        error(
            &run(
                dir.path(),
                &base,
                &[
                    "asset",
                    "download",
                    ASSET,
                    "--output",
                    path.to_str().unwrap(),
                    "--resume"
                ]
            )
            .await
        )["error"]["code"],
        "INVALID_RESPONSE"
    );
    assert_eq!(
        std::fs::read(dir.path().join("download.bin.virse-part")).unwrap(),
        b"abc"
    );
    assert!(!path.exists());
    server.abort();
}
#[tokio::test]
async fn partial_resource_zip_has_safe_names_and_per_file_failures() {
    let data = Shared::default();
    let (base, server) = app(data).await;
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("selection.zip");
    let out = run(
        dir.path(),
        &base,
        &[
            "resource",
            "pack",
            ONE,
            TWO,
            "--output",
            output.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(6));
    let result = json_output(&out);
    assert_eq!(result["manifest"]["files"], 1);
    assert_eq!(result["manifest"]["failed"], 1);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    assert!(archive.by_name("001-escape.png").is_ok());
    let manifest: Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest["items"][1]["error"]["code"], "forbidden");
    assert!(!manifest.to_string().contains("Bearer"));
    assert!(!manifest.to_string().contains("/bytes/"));
    server.abort();
}
#[tokio::test]
async fn group_pack_includes_media_and_reports_inline_content_as_skipped() {
    let data = Shared::default();
    let (base, server) = app(data).await;
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("group.zip");
    let result = value(
        &run(
            dir.path(),
            &base,
            &[
                "canvas",
                "pack",
                "--group",
                GROUP,
                "--output",
                output.to_str().unwrap(),
            ],
        )
        .await,
    );
    assert_eq!(result["manifest"]["files"], 1);
    assert_eq!(result["manifest"]["skipped"], 1);
    assert_eq!(result["manifest"]["items"][0]["asset_id"], ASSET);
    server.abort();
}

#[tokio::test]
async fn restarted_unversioned_download_does_not_reuse_a_disappeared_etag() {
    let data = Shared::default();
    {
        let mut d = data.lock().unwrap();
        d.fail_first = true;
        d.unversioned = true;
        d.mode = "drop-validator".into();
    }
    let (base, server) = app(data.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = interrupt(dir.path(), &base).await;
    let args = [
        "asset",
        "download",
        ASSET,
        "--output",
        path.to_str().unwrap(),
        "--resume",
    ];
    assert_eq!(run(dir.path(), &base, &args).await.status.code(), Some(5));
    let checkpoint: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("download.bin.virse-part.json")).unwrap(),
    )
    .unwrap();
    assert!(checkpoint["etag"].is_null());
    assert_eq!(
        error(&run(dir.path(), &base, &args).await)["error"]["code"],
        "RESUME_UNSUPPORTED"
    );
    assert_eq!(data.lock().unwrap().calls.len(), 2);
    assert!(
        data.lock()
            .unwrap()
            .calls
            .iter()
            .all(|h| !h.contains_key("authorization"))
    );
    assert!(!path.exists());
    server.abort();
}
