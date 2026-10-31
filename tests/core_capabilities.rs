use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as ApiPath, State},
    http::{HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, patch, post},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Output, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::process::Command;
const ORG: &str = "00000000-0000-0000-0000-000000000001";
const USER: &str = "00000000-0000-0000-0000-000000000002";
const ONE: &str = "00000000-0000-0000-0000-000000000003";
const TWO: &str = "00000000-0000-0000-0000-000000000004";
const DEST: &str = "00000000-0000-0000-0000-000000000005";
const CANVAS_RES: &str = "00000000-0000-0000-0000-000000000006";
const CANVAS: &str = "00000000-0000-0000-0000-000000000007";
const ASSET: &str = "00000000-0000-0000-0000-000000000008";
const TASK: &str = "00000000-0000-0000-0000-000000000009";
#[derive(Default)]
struct Data {
    calls: Vec<(String, Value)>,
    mutations: Vec<(String, Value)>,
    broken_create: bool,
    partial: bool,
    renamed: bool,
    preview_denied: bool,
    favorite_broken: bool,
}
type Shared = Arc<Mutex<Data>>;
fn item(id: &str, name: &str, kind: &str) -> Value {
    json!({"id":id,"name":name,"kind":kind,"parent_id":null,"target_id":if kind=="canvas"{Some(CANVAS)}else{None}})
}
fn value(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn error(out: &Output) -> Value {
    assert!(!out.status.success());
    serde_json::from_str(String::from_utf8_lossy(&out.stderr).lines().last().unwrap()).unwrap()
}
async fn run(root: &Path, base: &str, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_virse"));
    command
        .args(args)
        .arg("--json")
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", "virse_sk_core_test")
        .env("VIRSE_CONFIG_DIR", root)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .unwrap()
        .unwrap()
}
async fn record(
    State(state): State<Shared>,
    uri: Uri,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let path = uri.path();
    let mut state = state.lock().unwrap();
    state.calls.push((path.into(), body.clone()));
    match path{
        "/get_organization_by_user"=>Json(json!({"status":1,"organizations":[{"id":ORG,"name":"Team","org_type":"team","my_role":"admin","relation_type":"has_owner"}]})).into_response(),
        "/generation/models/catalog"=>{
            assert_eq!(body["organization_id"],ORG);assert_eq!(body["modes"],json!(["image","text","video","3d"]));
            Json(json!({"schema_version":2,"catalog_version":"c","models":[
                {"model_key":"image-model","mode":"image","display_name":"Image","user_preferences":{"is_favorite":false}},
                {"model_key":"3d-model","mode":"3d","display_name":"3D","user_preferences":{"is_favorite":true},"capabilities":{"triangle_limit":1000}}
            ],"defaults":{"3d":"3d-model"}})).into_response()
        }
        "/billing/balance"=>{assert_eq!(body["organization_id"],ORG);Json(json!({"status":1,"org_id":ORG,"total_balance":"15.00000001","member_budget":{"credit_remaining":null}})).into_response()},
        "/versa/canvas/create"=>{
            if state.broken_create{return (StatusCode::OK,"broken response").into_response();}
            assert_eq!(body["parent_id"],DEST);assert_eq!(body["name"],"New");
            Json(json!({"status":1,"canvas_space_id":CANVAS,"dentry":item(CANVAS_RES,"New","canvas")})).into_response()
        }
        "/versa/canvas/contents"=>{
            assert_eq!(body["contract_version"],2);assert_eq!(body["dentry_id"],CANVAS_RES);
            Json(json!({"status":1,"canvas_id":CANVAS,"items":[{"id":"content","name":"File"}],"total":4,"offset":body["offset"],"limit":body["limit"],"has_more":true})).into_response()
        }
        "/api/v1/resources/query"=>{
            let items=vec![item(ONE,if state.renamed{"moved-one"}else{"one"},"folder"),item(TWO,"two","folder"),item(DEST,"dest","folder"),item(CANVAS_RES,"canvas","canvas")];
            Json(json!({"request_id":"q","data":{"items":items,"snapshot":"s","next_cursor":null,"truncated":false}})).into_response()
        }
        _=>StatusCode::NOT_FOUND.into_response()
    }
}
async fn serve(state: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let app=Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).route("/auth/me",get(||async{Json(json!({"status":1,"id":USER}))}))
        .route("/get_organization_by_user",post(record)).route("/generation/models/catalog",post(record)).route("/billing/balance",post(record))
        .route("/versa/canvas/create",post(record)).route("/versa/canvas/contents",post(record)).route("/api/v1/resources/query",post(record))
        .route("/generation/models/{key}/favorite",patch(|State(s):State<Shared>,ApiPath(key):ApiPath<String>,Json(body):Json<Value>|async move{
            let mut state=s.lock().unwrap();state.calls.push(("favorite".into(),body.clone()));
            if state.favorite_broken{return (StatusCode::OK,"broken").into_response();}
            Json(json!({"model_key":key,"is_favorite":body["is_favorite"]})).into_response()
        }))
        .route("/api/v1/resources/{id}",get(|ApiPath(id):ApiPath<String>,State(s):State<Shared>|async move{
            s.lock().unwrap().calls.push(("resource".into(),json!(id)));
            Json(json!({"request_id":"detail","data":{"resource":item(&id,"test",if id==CANVAS_RES{"canvas"}else{"asset_image"}),"kind":"asset_image","asset":{"id":ASSET,"content":{"revision":1}},"detail":{}}}))
        }))
        .route("/api/v1/resources/batch",post(|State(s):State<Shared>,headers:HeaderMap,Json(body):Json<Value>|async move{
            let mut state=s.lock().unwrap();assert_eq!(headers["x-virse-expected-user"],USER);state.mutations.push((headers["idempotency-key"].to_str().unwrap().into(),body.clone()));state.renamed=true;
            let items:Vec<Value>=body["ids"].as_array().unwrap().iter().map(|id|if state.partial && id==TWO{json!({"id":id,"outcome":"failed","error":{"code":"no_permission","message":"denied"}})}else{json!({"id":id,"outcome":"succeeded","data":{"applied":true}})}).collect();
            Json(json!({"request_id":"batch","data":{"items":items}}))
        }))
        .route(&format!("/api/v1/assets/{ASSET}/presentation"),get(|State(s):State<Shared>,uri:Uri|async move{
            let mut state=s.lock().unwrap();state.calls.push(("presentation".into(),json!(uri.to_string())));
            if state.preview_denied{return (StatusCode::FORBIDDEN,Json(json!({"request_id":"deny","error":{"code":"forbidden","message":"denied","retryable":false}}))).into_response();}
            Json(json!({"request_id":"p","data":{"url":"/preview?representation=preview&source_version=one&object_version=2","filename":"preview.jpg","delivery":"authenticated_stream","content":{"revision":1,"size_bytes":5,"metadata_state":"partial"}}})).into_response()
        }))
        .route("/preview",get(|headers:HeaderMap,uri:Uri|async move{
            assert_eq!(headers["authorization"],"Bearer virse_sk_core_test");assert!(uri.query().unwrap().contains("source_version=one"));Bytes::from_static(b"bytes")
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/generations"),post(|State(s):State<Shared>,headers:HeaderMap,Json(body):Json<Value>|async move{
            s.lock().unwrap().calls.push(("generation".into(),body.clone()));assert!(headers.contains_key("idempotency-key"));
            Json(json!({"task_id":TASK,"project_id":CANVAS,"operation":body["operation"],"status":"queued","revision":1,"outputs":[]}))
        })).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    (
        base,
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    )
}
#[tokio::test]
async fn discovers_spaces_models_favorites_and_exact_balance_without_changing_defaults() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        value(&run(dir.path(), &base, &["space", "list"]).await)["organizations"][0]["my_role"],
        "admin"
    );
    assert!(!dir.path().join("config.json").exists());
    let models = value(
        &run(
            dir.path(),
            &base,
            &["--org", ORG, "model", "list", "--favorites"],
        )
        .await,
    );
    assert_eq!(models["models"].as_array().unwrap().len(), 1);
    assert_eq!(models["models"][0]["model_key"], "3d-model");
    assert_eq!(
        value(
            &run(
                dir.path(),
                &base,
                &["--org", ORG, "model", "get", "3d-model"]
            )
            .await
        )["models"][0]["capabilities"]["triangle_limit"],
        1000
    );
    assert_eq!(
        value(&run(dir.path(), &base, &["--org", ORG, "balance"]).await)["total_balance"],
        "15.00000001"
    );
    assert_eq!(
        value(
            &run(
                dir.path(),
                &base,
                &["--org", ORG, "model", "favorite", "3d-model"]
            )
            .await
        )["is_favorite"],
        true
    );
    assert_eq!(
        value(
            &run(
                dir.path(),
                &base,
                &["--org", ORG, "model", "unfavorite", "3d-model"]
            )
            .await
        )["is_favorite"],
        false
    );
    state.lock().unwrap().favorite_broken = true;
    let out = run(
        dir.path(),
        &base,
        &["--org", ORG, "model", "favorite", "3d-model"],
    )
    .await;
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(error(&out)["error"]["code"], "MODEL_PREFERENCE_UNCERTAIN");
    assert_eq!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(p, _)| p == "favorite")
            .count(),
        3
    );
    server.abort();
}
#[tokio::test]
async fn canvas_creation_resolves_parent_and_unknown_result_is_not_retried() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &base,
        &["--org", ORG, "canvas", "create", "New", "--parent", "/dest"],
    )
    .await;
    assert_eq!(value(&out)["canvas_space_id"], CANVAS);
    state.lock().unwrap().broken_create = true;
    let out = run(
        dir.path(),
        &base,
        &["--org", ORG, "canvas", "create", "New", "--parent", "/dest"],
    )
    .await;
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(error(&out)["error"]["code"], "CANVAS_CREATION_UNCERTAIN");
    assert_eq!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(p, _)| p == "/versa/canvas/create")
            .count(),
        2
    );
    server.abort();
}
#[tokio::test]
async fn canvas_contents_preserve_filters_offset_and_resource_identity() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &base,
        &[
            "--org",
            ORG,
            "canvas",
            "contents",
            "/canvas",
            "--kind",
            "document",
            "--document-type",
            "pdf",
            "--offset",
            "2",
            "--limit",
            "1",
            "--sort",
            "size",
            "--order",
            "desc",
        ],
    )
    .await;
    assert!(value(&out)["has_more"].as_bool().unwrap());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--offset 3"));
    let data = state.lock().unwrap();
    let (_, body) = data
        .calls
        .iter()
        .find(|(p, _)| p == "/versa/canvas/contents")
        .unwrap();
    assert_eq!(body["sort_by"], "size_bytes");
    assert_eq!(body["sort_order"], "desc");
    assert_eq!(body["document_types"], json!(["pdf"]));
    server.abort();
}
#[tokio::test]
async fn resource_filters_and_pagination_are_forwarded_without_affecting_path_resolution() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    value(
        &run(
            dir.path(),
            &base,
            &[
                "--org",
                ORG,
                "resource",
                "ls",
                "/",
                "--sort",
                "modified-at",
                "--order",
                "desc",
                "--created-by-me",
                "--access",
                "edit",
                "--access",
                "manage",
                "--cursor",
                "same-snapshot",
            ],
        )
        .await,
    );
    let data = state.lock().unwrap();
    let body = &data.calls.last().unwrap().1;
    assert_eq!(body["sort"], "modified_at");
    assert_eq!(body["direction"], "desc");
    assert_eq!(body["created_by_me"], true);
    assert_eq!(body["access_levels"], json!([3, 4]));
    assert_eq!(body["cursor"], "same-snapshot");
    server.abort();
}
#[tokio::test]
async fn batch_move_recovery_freezes_all_paths_and_partial_copy_returns_each_result() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "--org",
        ORG,
        "resource",
        "move",
        "/one",
        "/two",
        "/dest",
        "--idempotency-key",
        "move-many",
    ];
    value(&run(dir.path(), &base, &args).await);
    let calls = state.lock().unwrap().calls.len();
    value(&run(dir.path(), &base, &args).await);
    {
        let data = state.lock().unwrap();
        assert_eq!(data.calls.len(), calls);
        assert_eq!(data.mutations[0], data.mutations[1]);
        assert_eq!(data.mutations[0].1["ids"], json!([ONE, TWO]));
        assert_eq!(data.mutations[0].1["destination"]["parent_id"], DEST);
    }
    state.lock().unwrap().partial = true;
    let out = run(
        dir.path(),
        &base,
        &["--org", ORG, "resource", "copy", ONE, TWO, "/dest"],
    )
    .await;
    assert_eq!(out.status.code(), Some(6));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["items"][0]["outcome"], "succeeded");
    assert_eq!(result["items"][1]["outcome"], "failed");
    server.abort();
}
#[tokio::test]
async fn preview_saves_real_bytes_and_permission_failure_does_not_fall_back_to_original() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preview.jpg");
    let out = run(
        dir.path(),
        &base,
        &[
            "--org",
            ORG,
            "resource",
            "preview",
            ONE,
            "--output",
            path.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(value(&out)["bytes"], 5);
    assert_eq!(std::fs::read(&path).unwrap(), b"bytes");
    state.lock().unwrap().preview_denied = true;
    let denied = dir.path().join("denied.jpg");
    let out = run(
        dir.path(),
        &base,
        &[
            "--org",
            ORG,
            "resource",
            "preview",
            ONE,
            "--output",
            denied.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(4));
    assert!(!denied.exists());
    server.abort();
}
#[tokio::test]
async fn generation_shortcuts_use_task_submission_and_source_contract() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    value(
        &run(
            dir.path(),
            &base,
            &[
                "--canvas",
                CANVAS,
                "generation",
                "text",
                "--model",
                "text-model",
                "--prompt",
                "hello",
                "--reference",
                ONE,
                "--target-element",
                TWO,
            ],
        )
        .await,
    );
    value(
        &run(
            dir.path(),
            &base,
            &[
                "--canvas",
                CANVAS,
                "generation",
                "remove-bg",
                "--model",
                "remove-model",
                "--source-asset",
                ASSET,
            ],
        )
        .await,
    );
    value(
        &run(
            dir.path(),
            &base,
            &[
                "--canvas",
                CANVAS,
                "generation",
                "upscale",
                "--model",
                "upscale-model",
                "--source-element",
                ONE,
                "--scale",
                "4",
            ],
        )
        .await,
    );
    value(
        &run(
            dir.path(),
            &base,
            &[
                "--canvas",
                CANVAS,
                "generation",
                "outpaint",
                "--model",
                "outpaint-model",
                "--mask-asset",
                ASSET,
                "--prompt",
                "extend",
                "--reference",
                ONE,
                "--parameters",
                r#"{"resolution":"2K"}"#,
            ],
        )
        .await,
    );
    let data = state.lock().unwrap();
    let calls: Vec<_> = data
        .calls
        .iter()
        .filter(|(p, _)| p == "generation")
        .map(|(_, b)| b)
        .collect();
    assert_eq!(calls[0]["operation"], "text_generate");
    assert_eq!(calls[0]["outputs"][0]["element_id"], TWO);
    assert!(calls[0]["input"].get("image_count").is_none());
    assert_eq!(calls[1]["input"]["source"]["asset_id"], ASSET);
    assert_eq!(calls[2]["input"]["source"]["element_id"], ONE);
    assert_eq!(calls[2]["input"]["upscale_times"], 4);
    assert_eq!(calls[3]["input"]["mask_source"]["asset_id"], ASSET);
    assert!(calls[3]["input"].get("mask_image_filepath").is_none());
    assert_eq!(calls[3]["input"]["parameters"]["resolution"], "2K");
    server.abort();
}

#[tokio::test]
async fn asset_preview_keeps_explicit_canvas_context_and_never_uses_resource_endpoint() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("asset-preview.jpg");
    value(
        &run(
            dir.path(),
            &base,
            &[
                "--org",
                ORG,
                "--canvas",
                CANVAS,
                "asset",
                "preview",
                ASSET,
                "--representation",
                "content",
                "--output",
                output.to_str().unwrap(),
            ],
        )
        .await,
    );
    assert_eq!(std::fs::read(output).unwrap(), b"bytes");
    let data = state.lock().unwrap();
    assert!(!data.calls.iter().any(|(p, _)| p == "resource"));
    let url = data
        .calls
        .iter()
        .find(|(p, _)| p == "presentation")
        .unwrap()
        .1
        .as_str()
        .unwrap();
    assert!(url.contains(&format!("canvas_space_id={CANVAS}")));
    assert!(url.contains("representation=content"));
    assert!(!url.contains("dentry_id"));
    server.abort();
}

#[tokio::test]
async fn oversized_batch_is_rejected_before_network_calls() {
    let dir = tempfile::tempdir().unwrap();
    let mut args = vec!["--org", ORG, "resource", "copy"];
    args.extend(std::iter::repeat_n(ONE, 201));
    args.push("/");
    let out = run(dir.path(), "http://127.0.0.1:9", &args).await;
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error(&out)["error"]["code"], "INVALID_INPUT");
}
