mod support;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as ApiPath, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use support::*;
use tokio::sync::Notify;
use uuid::Uuid;
#[derive(Default)]
struct Data {
    folders: HashMap<String, Value>,
    uploads: Vec<(String, Vec<u8>, Value)>,
    receipts: HashMap<String, Value>,
    lose_folder: bool,
    lose_file: bool,
    folder_calls: usize,
    block_upload: bool,
    bad_binding: bool,
}
#[derive(Clone, Default)]
struct StateData {
    data: Arc<Mutex<Data>>,
    started: Arc<Notify>,
    release: Arc<Notify>,
}
fn field(headers: &HeaderMap, body: &[u8], name: &str) -> Vec<u8> {
    let content_type = headers["content-type"].to_str().unwrap();
    let boundary = content_type.split("boundary=").nth(1).unwrap();
    let needle = format!("name=\"{name}\"");
    let start = body
        .windows(needle.len())
        .position(|b| b == needle.as_bytes())
        .unwrap();
    let after = &body[start..];
    let start = after.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
    let data = &after[start..];
    let end = format!("\r\n--{boundary}");
    let end = data
        .windows(end.len())
        .position(|b| b == end.as_bytes())
        .unwrap();
    data[..end].to_vec()
}
async fn app(state: StateData) -> (String, tokio::task::JoinHandle<()>) {
    serve(Router::new().route("/api/v1/resources/query",post(|State(s):State<StateData>,Json(body):Json<Value>|async move{
        let data=s.data.lock().unwrap();let items:Vec<_>=data.folders.values().filter(|v|v["parent_id"]==body["parent_id"]).cloned().collect();envelope(json!({"items":items,"snapshot":"s","next_cursor":null,"truncated":false}))
    })).route("/api/v1/resources/{id}",get(|State(s):State<StateData>,ApiPath(id):ApiPath<String>|async move{let data=s.data.lock().unwrap();
        if let Some(folder)=data.folders.get(&id){return envelope(json!({"resource":folder,"kind":"folder","asset":null,"detail":{}}));}
        let (key,receipt)=data.receipts.iter().find(|(_,value)|value["resource"]["id"]==id).unwrap();
        let bytes=&data.uploads.iter().find(|(k,_,_)|k==key).unwrap().1;
        let checksum=if data.bad_binding{format!("{:x}",Sha256::digest(b"wrong"))}else{format!("{:x}",Sha256::digest(bytes))};
        let mut asset=receipt["asset"].clone();asset["content"]["checksum"]=json!({"algorithm":"sha256","value":checksum});asset["content"]["size_bytes"]=json!(bytes.len());
        envelope(json!({"resource":receipt["resource"],"kind":"asset_image","asset":asset,"detail":{}}))}))
    .route("/versa/folder/create",post(|State(s):State<StateData>,Json(body):Json<Value>|async move{
        let mut data=s.data.lock().unwrap();data.folder_calls+=1;let id=Uuid::new_v4().to_string();let folder=item(&id,body["name"].as_str().unwrap(),"folder",body["parent_id"].as_str());data.folders.insert(id,folder.clone());
        if data.lose_folder{data.lose_folder=false;return (StatusCode::OK,"truncated").into_response();}Json(json!({"status":1,"dentry":folder})).into_response()
    })).route("/api/v1/assets",post(|State(s):State<StateData>,headers:HeaderMap,body:Bytes|async move{
        assert_eq!(headers["x-virse-expected-user"],USER);let key=headers["idempotency-key"].to_str().unwrap().to_owned();let bytes=field(&headers,&body,"file");let options:Value=serde_json::from_slice(&field(&headers,&body,"options")).unwrap();let (result,lost,blocked)={
            let mut data=s.data.lock().unwrap();data.uploads.push((key.clone(),bytes,options));let result=data.receipts.entry(key).or_insert_with(||json!({"asset":{"id":Uuid::new_v4(),"type":"image","status":"ready","content":{"revision":1,"metadata_state":"partial"}},"resource":item(&Uuid::new_v4().to_string(),"file.png","asset_image",None),"skipped":false})).clone();let lost=data.lose_file;data.lose_file=false;(result,lost,data.block_upload)
        };
        if blocked{s.started.notify_one();s.release.notified().await;}
        if lost{(StatusCode::OK,"truncated").into_response()}else{envelope(result).into_response()}
    })).with_state(state)).await
}
fn prepared(out: &std::process::Output) -> String {
    value(out)["batch_id"].as_str().unwrap().into()
}
#[tokio::test]
async fn batch_freezes_bytes_and_retries_only_unknown_file_with_same_key() {
    let state = StateData::default();
    state.data.lock().unwrap().lose_file = true;
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let b = dir.path().join("b.png");
    std::fs::write(&a, b"frozen-a").unwrap();
    std::fs::write(&b, b"frozen-b").unwrap();
    let id = prepared(
        &run(
            dir.path(),
            &base,
            &[
                "resource",
                "import",
                a.to_str().unwrap(),
                b.to_str().unwrap(),
                "--defer",
            ],
        )
        .await,
    );
    std::fs::write(a, b"changed-after-prepare").unwrap();
    let first = run(dir.path(), &base, &["batch", "run", &id]).await;
    assert_eq!(first.status.code(), Some(7));
    let first = json_output(&first);
    assert_eq!(first["items"][0]["status"], "unknown");
    assert_eq!(first["items"][1]["status"], "succeeded");
    let result = value(&run(dir.path(), &base, &["batch", "resume", &id]).await);
    assert_eq!(result["status"], "completed");
    let data = state.data.lock().unwrap();
    assert_eq!(data.uploads.len(), 3);
    assert_eq!(data.uploads[0].1, b"frozen-a");
    assert_eq!(data.uploads[0], data.uploads[2]);
    assert_eq!(data.receipts.len(), 2);
    server.abort();
}
#[tokio::test]
async fn uncertain_folder_is_never_created_twice_and_explicit_binding_unblocks_descendants() {
    let state = StateData::default();
    state.data.lock().unwrap().lose_folder = true;
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir_all(source.join("sub")).unwrap();
    std::fs::write(source.join("sub/a.png"), b"a").unwrap();
    let id = prepared(
        &run(
            dir.path(),
            &base,
            &["resource", "import", source.to_str().unwrap(), "--defer"],
        )
        .await,
    );
    let first = run(dir.path(), &base, &["batch", "run", &id]).await;
    assert_eq!(first.status.code(), Some(7));
    let data = json_output(&first);
    let folder_item = data["items"][0]["id"].as_str().unwrap();
    assert_eq!(data["items"][0]["status"], "unknown");
    assert_eq!(state.data.lock().unwrap().folder_calls, 1);
    assert_eq!(
        run(dir.path(), &base, &["batch", "resume", &id])
            .await
            .status
            .code(),
        Some(7)
    );
    assert_eq!(state.data.lock().unwrap().folder_calls, 1);
    let folder = state
        .data
        .lock()
        .unwrap()
        .folders
        .keys()
        .next()
        .unwrap()
        .clone();
    value(
        &run(
            dir.path(),
            &base,
            &["batch", "bind-folder", &id, folder_item, &folder],
        )
        .await,
    );
    let done = value(&run(dir.path(), &base, &["batch", "resume", &id]).await);
    assert_eq!(done["status"], "completed");
    assert_eq!(state.data.lock().unwrap().folder_calls, 2);
    assert_eq!(state.data.lock().unwrap().uploads.len(), 1);
    server.abort();
}
#[tokio::test]
async fn pause_cancel_and_retry_do_not_resubmit_completed_items() {
    let state = StateData::default();
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("one.png");
    std::fs::write(&source, b"one").unwrap();
    let id = prepared(
        &run(
            dir.path(),
            &base,
            &["resource", "import", source.to_str().unwrap(), "--defer"],
        )
        .await,
    );
    value(&run(dir.path(), &base, &["batch", "pause", &id]).await);
    assert_eq!(
        value(&run(dir.path(), &base, &["batch", "run", &id]).await)["status"],
        "paused"
    );
    assert!(state.data.lock().unwrap().uploads.is_empty());
    assert_eq!(
        value(&run(dir.path(), &base, &["batch", "cancel", &id]).await)["items"][0]["status"],
        "cancelled"
    );
    assert!(state.data.lock().unwrap().uploads.is_empty());
    let done = value(
        &run(
            dir.path(),
            &base,
            &["batch", "retry", &id, "--include-cancelled"],
        )
        .await,
    );
    assert_eq!(done["status"], "completed");
    assert_eq!(state.data.lock().unwrap().uploads.len(), 1);
    assert_eq!(
        value(&run(dir.path(), &base, &["batch", "cancel", &id]).await)["status"],
        "completed"
    );
    assert_eq!(state.data.lock().unwrap().uploads.len(), 1);
    server.abort();
}
#[tokio::test]
async fn cancellation_during_upload_stops_only_later_dispatch() {
    let state = StateData::default();
    state.data.lock().unwrap().block_upload = true;
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.png");
    let b = dir.path().join("b.png");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let id = prepared(
        &run(
            dir.path(),
            &base,
            &[
                "resource",
                "import",
                a.to_str().unwrap(),
                b.to_str().unwrap(),
                "--defer",
            ],
        )
        .await,
    );
    let worker = command(dir.path(), &base, &["batch", "run", &id])
        .spawn()
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), state.started.notified())
        .await
        .unwrap();
    let pending = value(&run(dir.path(), &base, &["batch", "cancel", &id]).await);
    assert_eq!(pending["requested_control"], "cancel");
    assert_eq!(pending["worker_active"], true);
    assert_eq!(pending["items"][0]["status"], "running");
    state.release.notify_one();
    let out = worker.wait_with_output().await.unwrap();
    assert!(out.status.success());
    let done = json_output(&out);
    assert_eq!(done["items"][0]["status"], "succeeded");
    assert_eq!(done["items"][1]["status"], "cancelled");
    assert_eq!(state.data.lock().unwrap().uploads.len(), 1);
    server.abort();
}
#[tokio::test]
async fn batches_reject_account_environment_drift_and_changed_staging() {
    let state = StateData::default();
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("one.png");
    std::fs::write(&source, b"one").unwrap();
    let output = value(
        &run(
            dir.path(),
            &base,
            &["resource", "import", source.to_str().unwrap(), "--defer"],
        )
        .await,
    );
    let id = output["batch_id"].as_str().unwrap();
    let item = output["items"][0]["id"].as_str().unwrap();
    let (other, other_server) = app(StateData::default()).await;
    assert_eq!(
        error(&run(dir.path(), &other, &["batch", "run", id]).await)["error"]["code"],
        "BATCH_IDENTITY_CHANGED"
    );
    let stage = dir
        .path()
        .join("state/batches")
        .join(id)
        .join("files")
        .join(item)
        .join("one.png");
    std::fs::write(stage, b"tampered").unwrap();
    let result = run(dir.path(), &base, &["batch", "run", id]).await;
    assert_eq!(result.status.code(), Some(6));
    assert_eq!(
        json_output(&result)["items"][0]["error"]["code"],
        "BATCH_STATE_ERROR"
    );
    assert!(state.data.lock().unwrap().uploads.is_empty());
    server.abort();
    other_server.abort();
}
#[tokio::test]
async fn directory_skip_does_not_merge_into_existing_tree_and_unsupported_files_are_reported() {
    let state = StateData::default();
    state
        .data
        .lock()
        .unwrap()
        .folders
        .insert(DEST.into(), item(DEST, "source", "folder", None));
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("a.png"), b"a").unwrap();
    std::fs::write(source.join("private.env"), b"ignored").unwrap();
    let result = value(
        &run(
            dir.path(),
            &base,
            &[
                "resource",
                "import",
                source.to_str().unwrap(),
                "--conflict",
                "skip",
            ],
        )
        .await,
    );
    assert_eq!(result["status"], "completed");
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["status"] == "skipped")
    );
    assert_eq!(state.data.lock().unwrap().folder_calls, 0);
    assert!(state.data.lock().unwrap().uploads.is_empty());
    server.abort();
}

#[tokio::test]
async fn forgetting_a_terminal_batch_removes_only_local_snapshots() {
    let state = StateData::default();
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("one.png");
    std::fs::write(&source, b"one").unwrap();
    let id = prepared(
        &run(
            dir.path(),
            &base,
            &["resource", "import", source.to_str().unwrap(), "--defer"],
        )
        .await,
    );
    assert_eq!(
        error(&run(dir.path(), &base, &["batch", "forget", &id, "--yes"]).await)["error"]["code"],
        "BATCH_STATE_ERROR"
    );
    value(&run(dir.path(), &base, &["batch", "run", &id]).await);
    let count = state.data.lock().unwrap().uploads.len();
    let removed = value(&run(dir.path(), &base, &["batch", "forget", &id, "--yes"]).await);
    assert_eq!(removed["remote_resources_changed"], false);
    assert!(!dir.path().join("state/batches").join(&id).exists());
    assert_eq!(std::fs::read(source).unwrap(), b"one");
    assert_eq!(state.data.lock().unwrap().uploads.len(), count);
    server.abort();
}

#[tokio::test]
async fn explicit_file_binding_requires_server_checksum_and_does_not_reupload() {
    let state = StateData::default();
    state.data.lock().unwrap().lose_file = true;
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("one.png");
    std::fs::write(&source, b"one").unwrap();
    let id = prepared(
        &run(
            dir.path(),
            &base,
            &["resource", "import", source.to_str().unwrap(), "--defer"],
        )
        .await,
    );
    let first = run(dir.path(), &base, &["batch", "run", &id]).await;
    assert_eq!(first.status.code(), Some(7));
    let result = json_output(&first);
    let item = result["items"][0]["id"].as_str().unwrap();
    let resource = state.data.lock().unwrap().receipts.values().next().unwrap()["resource"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    state.data.lock().unwrap().bad_binding = true;
    let args = ["batch", "bind-file", &id, item, &resource];
    assert_eq!(
        error(&run(dir.path(), &base, &args).await)["error"]["code"],
        "BATCH_STATE_ERROR"
    );
    state.data.lock().unwrap().bad_binding = false;
    value(&run(dir.path(), &base, &args).await);
    assert_eq!(
        value(&run(dir.path(), &base, &["batch", "resume", &id]).await)["status"],
        "completed"
    );
    assert_eq!(state.data.lock().unwrap().uploads.len(), 1);
    server.abort();
}
