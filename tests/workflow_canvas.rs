mod support;
use axum::{
    Json, Router,
    extract::{Path as ApiPath, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::*;
#[derive(Default)]
struct Data {
    reads: usize,
    writes: Vec<(String, Value)>,
    lost: bool,
    conflict: bool,
    pastes: Vec<Value>,
}
type Shared = Arc<Mutex<Data>>;
fn entity(id: &str) -> Value {
    json!({"id":id,"canvas_id":CANVAS,"type":"text","name":"source","layout":{"position":{"x":10,"y":20},"size":{"width":if id==ONE{10}else{30},"height":if id==ONE{20}else{40}},"rotation":0,"z_index":0},"attributes":{"text":"hello","text_rich":"<p>hello</p>","style":{}},"generation":{"config":null},"revisions":{"name":1,"layout":2,"content":3}})
}
async fn app(state: Shared) -> (String, tokio::task::JoinHandle<()>) {
    serve(Router::new().route(&format!("/v2/canvas/projects/{CANVAS}/elements/{{id}}"),get(|State(s):State<Shared>,ApiPath(id):ApiPath<String>|async move{s.lock().unwrap().reads+=1;Json(json!({"element":entity(&id)}))}))
        .route(&format!("/v2/canvas/projects/{CANVAS}/groups/{GROUP}"),get(||async{Json(json!({"group":{"id":GROUP,"elements":[ONE,TWO],"position_x":5,"position_y":8,"size_width":50,"size_height":60,"title":"G","revisions":{"entity":7}}}))}))
        .route(&format!("/v2/canvas/projects/{CANVAS}/edges"),get(||async{Json(json!({"items":[{"id":DEST,"source_element_id":ONE,"target_element_id":TWO,"input_slot":"reference"}],"total":1}))}))
        .route(&format!("/v2/canvas/projects/{CANVAS}/transactions"),post(|State(s):State<Shared>,headers:HeaderMap,Json(body):Json<Value>|async move{
            assert_eq!(headers["x-virse-expected-user"],USER);let mut state=s.lock().unwrap();state.writes.push((headers["idempotency-key"].to_str().unwrap().into(),body.clone()));
            if state.conflict{return (StatusCode::CONFLICT,Json(json!({"detail":{"code":"CANVAS_REVISION_CONFLICT","revisions":{"layout":9}}}))).into_response();}
            if state.lost&&state.writes.len()==1{return (StatusCode::OK,"truncated").into_response();}
            Json(json!({"mutation_id":"mutation","results":body["operations"]})).into_response()
        }))
        .route(&format!("/canvas/projects/{CANVAS}/paste"),post(|State(s):State<Shared>,Json(body):Json<Value>|async move{s.lock().unwrap().pastes.push(body.clone());Json(json!({"status":1,"elements":body["elements"]}))}))
        .route(&format!("/canvas/projects/{CANVAS}/recently-deleted"),get(||async{Json(json!({"status":1,"items":[{"id":ONE}]}))}))
        .route(&format!("/canvas/projects/{CANVAS}/recently-deleted/{ONE}/restore"),post(|Json(body):Json<Value>|async move{assert_eq!(body["clientMutationId"],"restore-one");Json(json!({"status":1,"restored":true}))}))
        .with_state(state)).await
}
#[tokio::test]
async fn lost_canvas_write_replays_original_revisions_without_rereading() {
    let state = Shared::default();
    state.lock().unwrap().lost = true;
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let patch = dir.path().join("patch.json");
    std::fs::write(&patch, r#"{"layout":{"position":{"x":42,"y":20}}}"#).unwrap();
    let args = [
        "canvas",
        "element",
        "update",
        ONE,
        "--file",
        patch.to_str().unwrap(),
        "--idempotency-key",
        "fixed",
    ];
    let first = run(dir.path(), &base, &args).await;
    assert_eq!(first.status.code(), Some(7));
    assert_eq!(error(&first)["error"]["idempotency_key"], "fixed");
    value(&run(dir.path(), &base, &args).await);
    {
        let data = state.lock().unwrap();
        assert_eq!(data.reads, 1);
        assert_eq!(data.writes.len(), 2);
        assert_eq!(data.writes[0], data.writes[1]);
        assert_eq!(
            data.writes[0].1["operations"][0]["expected_revisions"]["layout"],
            2
        );
    }
    std::fs::write(&patch, r#"{"layout":{"position":{"x":43,"y":20}}}"#).unwrap();
    assert_eq!(run(dir.path(), &base, &args).await.status.code(), Some(2));
    assert_eq!(state.lock().unwrap().writes.len(), 2);
    server.abort();
}
#[tokio::test]
async fn revision_conflict_is_not_forced_or_retried() {
    let state = Shared::default();
    state.lock().unwrap().conflict = true;
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &base,
        &[
            "canvas",
            "element",
            "name",
            ONE,
            "new",
            "--revisions",
            r#"{"name":1}"#,
        ],
    )
    .await;
    assert_eq!(error(&out)["error"]["code"], "CANVAS_REVISION_CONFLICT");
    assert_eq!(state.lock().unwrap().writes.len(), 1);
    assert_eq!(state.lock().unwrap().reads, 0);
    server.abort();
}
#[tokio::test]
async fn group_layout_updates_members_and_bounds_atomically() {
    let state = Shared::default();
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    value(
        &run(
            dir.path(),
            &base,
            &[
                "canvas", "layout", "--group", GROUP, "--mode", "row", "--gap", "5", "--x", "100",
                "--y", "200",
            ],
        )
        .await,
    );
    let data = state.lock().unwrap();
    let ops = data.writes[0].1["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 3);
    assert_eq!(
        ops[0]["value"]["layout"]["position"],
        json!({"x":100.0,"y":200.0})
    );
    assert_eq!(ops[1]["value"]["layout"]["position"]["x"], 115.0);
    assert_eq!(ops[2]["action"], "group.update");
    assert_eq!(ops[2]["value"]["size"], json!({"width":45.0,"height":40.0}));
    assert_eq!(ops[2]["expected_revisions"]["entity"], 7);
    server.abort();
}
#[tokio::test]
async fn copy_remaps_only_selected_relationships_and_reuses_frozen_plan() {
    let state = Shared::default();
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "canvas",
        "copy",
        "--group",
        GROUP,
        "--idempotency-key",
        "copy-group",
    ];
    value(&run(dir.path(), &base, &args).await);
    let reads = state.lock().unwrap().reads;
    value(&run(dir.path(), &base, &args).await);
    let data = state.lock().unwrap();
    assert_eq!(data.reads, reads);
    assert_eq!(data.pastes[0], data.pastes[1]);
    let body = &data.pastes[0];
    let id1 = &body["elements"][0]["element_id"];
    let id2 = &body["elements"][1]["element_id"];
    assert_ne!(id1, ONE);
    assert_eq!(&body["edges"][0]["source_element_id"], id1);
    assert_eq!(&body["edges"][0]["target_element_id"], id2);
    assert_eq!(&body["groups"][0]["elements"][0], id1);
    assert_eq!(body["groups"][0]["position"]["x"], 85.0);
    assert_eq!(body["elements"][0]["text"], "hello");
    server.abort();
}
#[tokio::test]
async fn create_entities_and_restore_use_existing_http_shapes() {
    let state = Shared::default();
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for (kind, body) in [
        ("element", json!({"type":"text","text":"draft"})),
        ("group", json!({"elements":[ONE,TWO],"title":"Group"})),
        (
            "edge",
            json!({"source_element_id":ONE,"target_element_id":TWO,"input_slot":"reference"}),
        ),
    ] {
        let file = dir.path().join(format!("{kind}.json"));
        std::fs::write(&file, body.to_string()).unwrap();
        value(
            &run(
                dir.path(),
                &base,
                &["canvas", kind, "create", "--file", file.to_str().unwrap()],
            )
            .await,
        );
    }
    let actions: Vec<_> = state
        .lock()
        .unwrap()
        .writes
        .iter()
        .map(|(_, b)| b["operations"][0]["action"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        actions,
        vec!["element.create", "group.create", "edge.create"]
    );
    value(
        &run(
            dir.path(),
            &base,
            &[
                "canvas",
                "deleted",
                "restore",
                ONE,
                "--idempotency-key",
                "restore-one",
            ],
        )
        .await,
    );
    server.abort();
}

#[tokio::test]
async fn graph_updates_and_deletion_keep_entity_revision_checks() {
    let state = Shared::default();
    let (base, server) = app(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    value(
        &run(
            dir.path(),
            &base,
            &["canvas", "group", "name", GROUP, "renamed"],
        )
        .await,
    );
    let patch = dir.path().join("edge.json");
    std::fs::write(&patch, r#"{"input_slot":"image"}"#).unwrap();
    value(
        &run(
            dir.path(),
            &base,
            &[
                "canvas",
                "edge",
                "update",
                DEST,
                "--file",
                patch.to_str().unwrap(),
                "--revisions",
                r#"{"entity":3}"#,
            ],
        )
        .await,
    );
    value(
        &run(
            dir.path(),
            &base,
            &[
                "canvas",
                "edge",
                "delete",
                DEST,
                "--revisions",
                r#"{"entity":3}"#,
            ],
        )
        .await,
    );
    let data = state.lock().unwrap();
    assert_eq!(
        data.writes[0].1["operations"][0]["value"]["title"],
        "renamed"
    );
    assert_eq!(
        data.writes[0].1["operations"][0]["expected_revisions"]["entity"],
        7
    );
    assert_eq!(
        data.writes[1].1["operations"][0]["value"],
        json!({"input_slot":"image"})
    );
    assert_eq!(data.writes[2].1["operations"][0]["action"], "edge.delete");
    server.abort();
}
