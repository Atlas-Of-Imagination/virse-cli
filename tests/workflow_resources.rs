mod support;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, patch, post, put},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use support::*;
#[derive(Default)]
struct Data {
    calls: Vec<(String, Value)>,
    empty: usize,
    fail_empty: bool,
    fail_comment: bool,
    schema: usize,
}
type Shared = Arc<Mutex<Data>>;
async fn app(state: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let app=Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) }))
        .route("/api/v1/resources/{id}",get(|State(s):State<Shared>|async move{s.lock().unwrap().calls.push(("resource-read".into(),json!({})));envelope(json!({"resource":item(ONE,"file","folder",None),"kind":"folder","asset":null,"detail":{}}))}))
        .route("/api/v1/resources/query",post(||async{envelope(json!({"items":[],"snapshot":"s","next_cursor":null,"truncated":false}))}))
        .route("/versa/mark",post(record)).route("/versa/reorder",post(record)).route("/versa/shared_with_me/remove",post(record)).route("/versa/quick_access/list",post(record))
        .route("/versa/canvas/save_to_drive",post(|State(s):State<Shared>,headers:HeaderMap,Json(body):Json<Value>|async move{assert_eq!(headers["x-virse-expected-user"],USER);assert!(headers.contains_key("idempotency-key"));s.lock().unwrap().calls.push(("save".into(),body));Json(json!({"status":1,"dentry":{"id":DEST}}))}))
        .route("/api/v1/resources/batch",post(|State(s):State<Shared>,Json(body):Json<Value>|async move{assert_eq!(body["op"],"purge");s.lock().unwrap().calls.push(("purge".into(),body.clone()));let items:Vec<_>=body["ids"].as_array().unwrap().iter().map(|id|json!({"id":id,"outcome":"succeeded","data":{},"error":null})).collect();envelope(json!({"items":items}))}))
        .route("/versa/trash/empty",post(|State(s):State<Shared>,headers:HeaderMap,Json(body):Json<Value>|async move{
            assert_eq!(body["org_id"],ORG);assert_eq!(headers["x-virse-expected-user"],USER);let mut data=s.lock().unwrap();data.calls.push(("empty".into(),json!({"key":headers["idempotency-key"].to_str().unwrap()})));data.empty+=1;
            if data.fail_empty&&data.empty==1{return Json(json!({"status":1,"purged":[ONE],"failed":[{"id":TWO,"code":"forbidden"}],"total":2,"truncated":true}));}
            if data.empty==1{Json(json!({"status":1,"purged":[ONE],"failed":[],"total":1,"truncated":true}))}else{Json(json!({"status":1,"purged":[],"failed":[],"total":0,"truncated":false}))}
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/generations"),get(|Query(query):Query<HashMap<String,String>>|async move{assert_eq!(query["status"],"failed");assert_eq!(query["element_id"],ONE);assert_eq!(query["offset"],"4");Json(json!({"tasks":[],"offset":4,"limit":10}))}))
        .route(&format!("/v2/canvas/projects/{CANVAS}/generations/{ONE}/retry"),post(|headers:HeaderMap|async move{assert_eq!(headers["idempotency-key"],"retry-one");assert_eq!(headers["content-length"],"0");Json(json!({"task_id":TWO,"project_id":CANVAS,"operation":"image_generate","status":"queued","revision":1,"outputs":[],"retry_of":ONE}))}))
        .route(&format!("/canvas/projects/{CANVAS}/discussions"),post(record).get(||async{Json(json!({"status":1,"discussions":[]}))}))
        .route(&format!("/canvas/projects/{CANVAS}/discussions/{ONE}"),patch(record).delete(record).get(||async{Json(json!({"status":1,"discussion":{"id":ONE}}))}))
        .route(&format!("/canvas/projects/{CANVAS}/discussions/{ONE}/read"),post(record))
        .route(&format!("/canvas/projects/{CANVAS}/discussions/{ONE}/comments"),post(record))
        .route(&format!("/canvas/projects/{CANVAS}/discussions/{ONE}/comments/{TWO}"),put(record).delete(record))
        .route("/v2/skills/test-tool/input-schema",get(|State(s):State<Shared>|async move{let mut data=s.lock().unwrap();data.schema+=1;Json(json!({"skill_id":"test-tool","version_id":"immutable-v1","version":"1.0.0","input_schema":null}))}))
        .route(&format!("/v2/canvas/projects/{CANVAS}/transactions"),post(|State(s):State<Shared>,Json(body):Json<Value>|async move{s.lock().unwrap().calls.push(("transaction".into(),body.clone()));Json(json!({"mutation_id":"placed","results":body["operations"]}))}))
        .route("/skills/cover/upload",post(|headers:HeaderMap,body:axum::body::Bytes|async move{assert_eq!(headers["authorization"],"Bearer virse_sk_workflow_test");let text=String::from_utf8_lossy(&body);assert!(text.contains("name=\"cover\""));assert!(text.contains("cover.png"));assert!(body.ends_with(b"\r\n"));Json(json!({"coverUrl":"https://images.invalid/cover.png"}))}))
        .with_state(state);
    serve(app).await
}
async fn record(
    State(s): State<Shared>,
    uri: Uri,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> axum::response::Response {
    if uri.path().ends_with("/read") {
        assert_eq!(headers["content-length"], "0");
    }
    let body = body.map(|Json(v)| v).unwrap_or(json!({}));
    let mut data = s.lock().unwrap();
    data.calls.push((uri.path().into(), body));
    if data.fail_comment && uri.path().ends_with("/comments") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"code":"unavailable","message":"later"}})),
        )
            .into_response();
    }
    Json(json!({"status":1,"ok":true})).into_response()
}
#[tokio::test]
async fn resource_marks_order_hide_and_save_use_existing_contracts() {
    let s = Shared::default();
    let (base, server) = app(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for cmd in ["pin", "unpin", "star", "unstar", "hide-shared"] {
        value(&run(dir.path(), &base, &["resource", cmd, ONE]).await);
    }
    value(&run(dir.path(), &base, &["resource", "reorder", "/", ONE]).await);
    let args = [
        "resource",
        "save-canvas",
        "--element",
        ONE,
        "--element",
        TWO,
        "--parent",
        "/",
        "--idempotency-key",
        "save-two",
    ];
    value(&run(dir.path(), &base, &args).await);
    let reads = s
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(p, _)| p == "resource-read")
        .count();
    value(&run(dir.path(), &base, &args).await);
    let data = s.lock().unwrap();
    assert_eq!(
        data.calls
            .iter()
            .filter(|(p, _)| p == "resource-read")
            .count(),
        reads
    );
    let marks: Vec<_> = data
        .calls
        .iter()
        .filter(|(p, _)| p == "/versa/mark")
        .map(|(_, b)| (b["mark_type"].clone(), b["on"].clone()))
        .collect();
    assert_eq!(
        marks,
        vec![
            (json!("pinned"), json!(true)),
            (json!("pinned"), json!(false)),
            (json!("quick_access"), json!(true)),
            (json!("quick_access"), json!(false))
        ]
    );
    let saved: Vec<_> = data
        .calls
        .iter()
        .filter(|(p, _)| p == "save")
        .map(|(_, b)| b)
        .collect();
    assert_eq!(saved.len(), 4);
    assert_eq!(saved[0], saved[2]);
    assert_eq!(saved[0]["canvas_space_id"], CANVAS);
    server.abort();
}
#[tokio::test]
async fn purge_requires_explicit_confirmation_and_empty_trash_drains_pages() {
    let s = Shared::default();
    let (base, server) = app(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        run(dir.path(), &base, &["resource", "purge", ONE])
            .await
            .status
            .code(),
        Some(2)
    );
    assert!(s.lock().unwrap().calls.is_empty());
    value(
        &run(
            dir.path(),
            &base,
            &[
                "resource",
                "purge",
                ONE,
                "--yes",
                "--idempotency-key",
                "purge-one",
            ],
        )
        .await,
    );
    let args = [
        "resource",
        "empty-trash",
        "--yes",
        "--idempotency-key",
        "empty-all",
        "--max-batches",
        "1",
    ];
    let first = run(dir.path(), &base, &args).await;
    assert_eq!(first.status.code(), Some(6));
    assert_eq!(json_output(&first)["progress"]["operation_complete"], false);
    let second = value(&run(dir.path(), &base, &args).await);
    assert_eq!(second["progress"]["operation_complete"], true);
    assert_eq!(second["progress"]["pages"].as_array().unwrap().len(), 2);
    let data = s.lock().unwrap();
    let keys: Vec<_> = data
        .calls
        .iter()
        .filter(|(p, _)| p == "empty")
        .map(|(_, b)| &b["key"])
        .collect();
    assert_ne!(keys[0], keys[1]);
    server.abort();
}
#[tokio::test]
async fn empty_trash_stops_on_failures_and_explicit_resume_reconciles_remaining_candidates() {
    let s = Shared::default();
    s.lock().unwrap().fail_empty = true;
    let (base, server) = app(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "resource",
        "empty-trash",
        "--yes",
        "--idempotency-key",
        "partial",
    ];
    let first = run(dir.path(), &base, &args).await;
    assert_eq!(first.status.code(), Some(6));
    assert_eq!(s.lock().unwrap().empty, 1);
    assert_eq!(json_output(&first)["progress"]["next_batch"], 1);
    assert_eq!(
        value(&run(dir.path(), &base, &args).await)["progress"]["operation_complete"],
        true
    );
    server.abort();
}
#[tokio::test]
async fn task_filters_and_failed_output_retry_are_server_actions() {
    let s = Shared::default();
    let (base, server) = app(s).await;
    let dir = tempfile::tempdir().unwrap();
    value(
        &run(
            dir.path(),
            &base,
            &[
                "generation",
                "list",
                "--status",
                "failed",
                "--element",
                ONE,
                "--offset",
                "4",
                "--limit",
                "10",
            ],
        )
        .await,
    );
    let result = value(
        &run(
            dir.path(),
            &base,
            &["generation", "retry", ONE, "--idempotency-key", "retry-one"],
        )
        .await,
    );
    assert_eq!(result["retry_of"], ONE);
    assert_eq!(result["task_id"], TWO);
    server.abort();
}
#[tokio::test]
async fn discussions_support_real_anchor_and_reply_lifecycle_without_blind_retries() {
    let s = Shared::default();
    let (base, server) = app(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec![
            "canvas",
            "discussion",
            "create",
            "--content",
            "first",
            "--element",
            TWO,
        ],
        vec!["canvas", "discussion", "reply", ONE, "--content", "reply"],
        vec![
            "canvas",
            "discussion",
            "edit",
            ONE,
            TWO,
            "--content",
            "edited",
        ],
        vec!["canvas", "discussion", "resolve", ONE],
        vec!["canvas", "discussion", "resolve", ONE, "--reopen"],
        vec!["canvas", "discussion", "anchor", ONE, "--detach"],
        vec!["canvas", "discussion", "move", ONE, "--x", "5", "--y", "6"],
        vec!["canvas", "discussion", "read", ONE],
        vec!["canvas", "discussion", "delete", ONE, "--comment", TWO],
        vec!["canvas", "discussion", "delete", ONE],
    ] {
        value(&run(dir.path(), &base, &args).await);
    }
    {
        let data = s.lock().unwrap();
        assert_eq!(data.calls[0].1["anchor_id"], TWO);
        assert_eq!(data.calls[0].1["anchor_type"], "element");
        assert_eq!(data.calls[3].1["resolved"], true);
        assert_eq!(data.calls[4].1["resolved"], false);
        assert!(data.calls[5].1.get("anchor").unwrap().is_null());
    }
    s.lock().unwrap().fail_comment = true;
    let before = s.lock().unwrap().calls.len();
    let out = run(
        dir.path(),
        &base,
        &[
            "canvas",
            "discussion",
            "reply",
            ONE,
            "--content",
            "uncertain",
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(s.lock().unwrap().calls.len(), before + 1);
    server.abort();
}
#[tokio::test]
async fn cover_upload_and_standalone_tool_instance_do_not_start_a_run() {
    let s = Shared::default();
    let (base, server) = app(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cover.png");
    std::fs::write(&file, b"\x89PNG\r\n\x1a\ncover").unwrap();
    assert_eq!(
        value(
            &run(
                dir.path(),
                &base,
                &["tool", "cover", "upload", file.to_str().unwrap()]
            )
            .await
        )["coverUrl"],
        "https://images.invalid/cover.png"
    );
    let args = [
        "tool",
        "place",
        "test-tool",
        "--idempotency-key",
        "place-only",
    ];
    assert_eq!(
        value(&run(dir.path(), &base, &args).await)["started"],
        false
    );
    let reads = s.lock().unwrap().schema;
    value(&run(dir.path(), &base, &args).await);
    let data = s.lock().unwrap();
    assert_eq!(data.schema, reads);
    let calls: Vec<_> = data
        .calls
        .iter()
        .filter(|(p, _)| p == "transaction")
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1, calls[1].1);
    assert_eq!(calls[0].1["operations"][0]["value"]["type"], "skill");
    assert_eq!(
        calls[0].1["operations"][0]["value"]["version_id"],
        "immutable-v1"
    );
    server.abort();
}
