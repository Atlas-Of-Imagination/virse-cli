mod support;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::*;
#[derive(Default)]
struct Data {
    sent: Vec<Value>,
    stop: usize,
    broken: bool,
    busy: bool,
}
type Shared = Arc<Mutex<Data>>;
async fn server(s: Shared) -> (String, tokio::task::JoinHandle<()>) {
    serve(Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) }))
.route(&format!("/v2/canvas/projects/{CANVAS}/elements/{ONE}"),get(||async{Json(json!({"element":{"id":ONE,"type":"agent","attributes":{"style":{"status":"idle"}},"revisions":{"layout":1,"content":1}}}))}))
.route(&format!("/agent/blocks/{ONE}/messages"),post(|State(s):State<Shared>,h:HeaderMap,Json(b):Json<Value>|async move{assert!(h.contains_key("request-id"));let mut s=s.lock().unwrap();s.sent.push(b);if s.broken{return(StatusCode::BAD_GATEWAY,"broken").into_response();}
if s.busy{return(StatusCode::CONFLICT,Json(json!({"detail":"agent_run_in_progress"}))).into_response();}Json(json!({"accepted":true,"run_id":"r1","agent_turn_id":"t1","agent_block_id":ONE,"session_id":"s1"})).into_response()}))
.route(&format!("/agent/blocks/{ONE}/history"),get(||async{([("content-type","text/event-stream")],"event: progress\ndata: {\"type\":\"history_meta\",\"hasMore\":false}\n\nevent: progress\ndata: {\"type\":\"turn_completed\",\"turnId\":\"t1\"}\n\n")}))
.route(&format!("/agent/blocks/{ONE}/interrupt"),post(|State(s):State<Shared>|async move{s.lock().unwrap().stop+=1;Json(json!({"success":true}))})).with_state(s)).await
}
#[tokio::test]
async fn sends_once_with_reference_and_org() {
    let s = Shared::default();
    let (b, h) = server(s.clone()).await;
    let d = tempfile::tempdir().unwrap();
    value(
        &run(
            d.path(),
            &b,
            &[
                "composer",
                "send",
                ONE,
                "--input",
                "hello",
                "--reference",
                TWO,
            ],
        )
        .await,
    );
    let s = s.lock().unwrap();
    assert_eq!(s.sent.len(), 1);
    assert_eq!(s.sent[0]["organization_id"], ORG);
    assert_eq!(s.sent[0]["references"]["0"]["source_element_id"], TWO);
    assert_eq!(s.stop, 0);
    h.abort();
}
#[tokio::test]
async fn uncertain_send_never_retries() {
    let s = Shared::default();
    s.lock().unwrap().broken = true;
    let (b, h) = server(s.clone()).await;
    let d = tempfile::tempdir().unwrap();
    let o = run(d.path(), &b, &["composer", "send", ONE, "--input", "hello"]).await;
    assert_eq!(o.status.code(), Some(7));
    assert_eq!(s.lock().unwrap().sent.len(), 1);
    h.abort();
}
#[tokio::test]
async fn active_turn_rejection_preserved() {
    let s = Shared::default();
    s.lock().unwrap().busy = true;
    let (b, h) = server(s.clone()).await;
    let d = tempfile::tempdir().unwrap();
    let e = error(&run(d.path(), &b, &["composer", "send", ONE, "--input", "hello"]).await);
    assert_eq!(e["error"]["code"], "agent_run_in_progress");
    assert_eq!(s.lock().unwrap().sent.len(), 1);
    h.abort();
}
#[tokio::test]
async fn history_is_ndjson_and_stop_is_only_request() {
    let s = Shared::default();
    let (b, h) = server(s.clone()).await;
    let d = tempfile::tempdir().unwrap();
    let o = run(d.path(), &b, &["composer", "history", ONE]).await;
    assert!(o.status.success());
    assert_eq!(String::from_utf8(o.stdout).unwrap().lines().count(), 2);
    let v = value(&run(d.path(), &b, &["composer", "stop", ONE]).await);
    assert_eq!(v["type"], "interrupt_requested");
    assert_eq!(s.lock().unwrap().stop, 1);
    h.abort();
}
#[tokio::test]
async fn wait_connects_before_submit_and_filters_other_turns() {
    use axum::extract::ws::{Message, WebSocketUpgrade};
    let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let notified = Arc::new(tokio::sync::Notify::new());
    let ws_flag = connected.clone();
    let ws_notify = notified.clone();
    let send_flag = connected.clone();
    let send_notify = notified.clone();
    let app=Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).route(&format!("/canvas/ws/{CANVAS}"),get(move|ws:WebSocketUpgrade|{let flag=ws_flag.clone();let n=ws_notify.clone();async move{ws.on_upgrade(move|mut sock|async move{flag.store(true,std::sync::atomic::Ordering::SeqCst);n.notified().await;for turn in ["other-turn","t1"] {sock.send(Message::Text(json!({"type":"agent_stream_frame","agent_block_id":ONE,"agent_turn_id":turn,"frame":{"event":"progress","data":{"type":"turn_completed","turnId":turn}}}).to_string().into())).await.unwrap();}})}}))
 .route(&format!("/v2/canvas/projects/{CANVAS}/elements/{ONE}"),get(||async{Json(json!({"element":{"id":ONE,"type":"agent","attributes":{"style":{"run_id":"r1","agent_turn_id":"t1","status":"running"}}}}))}))
 .route(&format!("/agent/blocks/{ONE}/messages"),post(move||{let flag=send_flag.clone();let n=send_notify.clone();async move{assert!(flag.load(std::sync::atomic::Ordering::SeqCst));n.notify_one();Json(json!({"accepted":true,"run_id":"r1","agent_turn_id":"t1","agent_block_id":ONE,"session_id":"s1"}))}}));
    let (b, h) = serve(app).await;
    let d = tempfile::tempdir().unwrap();
    let o = run(
        d.path(),
        &b,
        &[
            "composer",
            "send",
            ONE,
            "--input",
            "hi",
            "--wait",
            "--timeout",
            "5",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(out.contains("t1"));
    assert!(!out.contains("other-turn"));
    h.abort();
}
#[tokio::test]
async fn disconnected_observer_reconciles_history_without_resending() {
    use axum::extract::ws::WebSocketUpgrade;
    let app=Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).route(&format!("/canvas/ws/{CANVAS}"),get(|ws:WebSocketUpgrade|async{ws.on_upgrade(|_sock|async{})}))
 .route(&format!("/v2/canvas/projects/{CANVAS}/elements/{ONE}"),get(||async{Json(json!({"element":{"id":ONE,"type":"agent","attributes":{"style":{"run_id":"r1","agent_turn_id":"t1","status":"running"}}}}))}))
 .route(&format!("/agent/blocks/{ONE}/history"),get(||async{([("content-type","text/event-stream")],"event: progress\ndata: {\"type\":\"turn_completed\",\"turnId\":\"t1\"}\n\n")}));
    let (b, h) = serve(app).await;
    let d = tempfile::tempdir().unwrap();
    let o = run(d.path(), &b, &["composer", "watch", ONE, "--timeout", "5"]).await;
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    h.abort();
}
#[tokio::test]
async fn archived_turn_replays_paginated_history_with_unlabelled_text_frames() {
    use axum::extract::{Query, ws::WebSocketUpgrade};
    use std::collections::HashMap;
    let app=Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).route(&format!("/canvas/ws/{CANVAS}"),get(|ws:WebSocketUpgrade|async{ws.on_upgrade(|_sock|async{tokio::time::sleep(std::time::Duration::from_secs(10)).await;})}))
 .route(&format!("/v2/canvas/projects/{CANVAS}/elements/{ONE}"),get(||async{Json(json!({"element":{"id":ONE,"type":"agent","attributes":{"style":{"run_id":"new","agent_turn_id":"new","status":"completed"}}}}))}))
 .route(&format!("/agent/blocks/{ONE}/history"),get(|Query(q):Query<HashMap<String,String>>|async move{let body=if q.contains_key("before"){"event: progress\ndata: {\"type\":\"turn_started\",\"turnId\":\"old\"}\n\nevent: progress\ndata: {\"type\":\"model_text_delta\",\"delta\":\"restored\"}\n\nevent: progress\ndata: {\"type\":\"turn_completed\",\"turnId\":\"old\"}\n\n"}else{"event: progress\ndata: {\"type\":\"history_meta\",\"hasMore\":true,\"nextCursor\":\"older\"}\n\n"};([("content-type","text/event-stream")],body)}));
    let (b, h) = serve(app).await;
    let d = tempfile::tempdir().unwrap();
    let o = run(
        d.path(),
        &b,
        &["composer", "watch", ONE, "--turn", "old", "--timeout", "5"],
    )
    .await;
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8(o.stdout).unwrap().contains("restored"));
    h.abort();
}
