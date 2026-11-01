use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Output, Stdio},
    sync::{Arc, Mutex},
};
use tokio::process::Command;
const CANVAS: &str = "00000000-0000-0000-0000-000000000001";
const ORG: &str = "00000000-0000-0000-0000-000000000002";
const USER: &str = "00000000-0000-0000-0000-000000000003";
const SOURCE: &str = "00000000-0000-0000-0000-000000000004";
const ELEMENT: &str = "00000000-0000-0000-0000-000000000005";
const TASK: &str = "00000000-0000-0000-0000-000000000006";
const OTHER: &str = "00000000-0000-0000-0000-000000000007";
#[derive(Default)]
struct Data {
    requests: Vec<(String, String, Value)>,
    asset_kind: String,
    document_type: Option<String>,
    status: String,
    source_reads: usize,
    source_removed: bool,
    lose_response: bool,
    disabled: bool,
}
type Shared = Arc<Mutex<Data>>;
fn state() -> Shared {
    Arc::new(Mutex::new(Data {
        asset_kind: "image".into(),
        status: "ready".into(),
        ..Default::default()
    }))
}
fn metadata(s: &Data) -> Value {
    json!({"id":SOURCE,"type":s.asset_kind,"document_type":s.document_type,"status":s.status})
}
fn task(status: &str) -> Value {
    json!({"task_id":TASK,"project_id":CANVAS,"operation":"test","status":status,"revision":1,"outputs":[],"actions":{"cancel":false,"retry":false}})
}
async fn assets(State(s): State<Shared>) -> Json<Value> {
    let mut s = s.lock().unwrap();
    s.source_reads += 1;
    Json(json!({"request_id":"read","data":metadata(&s)}))
}
async fn resource(State(s): State<Shared>) -> axum::response::Response {
    let mut s = s.lock().unwrap();
    s.source_reads += 1;
    if s.source_removed {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(json!({"request_id":"read","data":{"resource":{"id":SOURCE,"parent_id":null,"name":"reference.png","kind":"asset_image"},"kind":"asset_image","asset":metadata(&s),"detail":{}}})).into_response()
}
async fn generations(
    State(s): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    s.lock().unwrap().requests.push((
        "generation".into(),
        headers["idempotency-key"].to_str().unwrap().into(),
        body,
    ));
    Json(task("queued"))
}
async fn create(
    State(s): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    assert_eq!(headers["x-virse-expected-user"], USER);
    let mut s = s.lock().unwrap();
    s.requests.push((
        "create".into(),
        headers["idempotency-key"].to_str().unwrap().into(),
        body.clone(),
    ));
    if s.disabled {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"detail":"CANVAS_HTTP_COMMANDS_NOT_ENABLED"})),
        )
            .into_response();
    }
    if s.lose_response && s.requests.len() == 1 {
        return (StatusCode::CREATED, "truncated").into_response();
    }
    // The backend may bind a canvas-owned asset with a different ID.
    (StatusCode::CREATED,Json(json!({"mutation_id":OTHER,"element":{"id":ELEMENT,"canvas_id":CANVAS,"type":body["type"],"content_ref":{"kind":"asset","id":OTHER},"layout":{"position":{"x":800,"y":0},"size":{"width":512,"height":512}},"revisions":{"content":1}}}))).into_response()
}
async fn serve(s: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let app=Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).route("/auth/me",get(||async{Json(json!({"status":1,"id":USER}))}))
        .route(&format!("/api/v1/assets/{SOURCE}"),get(assets))
        .route(&format!("/api/v1/resources/{SOURCE}"),get(resource))
        .route("/api/v1/resources/query",post(|State(s):State<Shared>|async move{
            let mut s=s.lock().unwrap();s.source_reads+=1;
            Json(json!({"request_id":"read","data":{"items":if s.source_removed {vec![]}else{vec![json!({"id":SOURCE,"parent_id":null,"name":"reference.png","kind":"asset_image"})]},"next_cursor":null,"snapshot":"s","truncated":false}}))
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/generations"),post(generations))
        .route(&format!("/v2/canvas/projects/{CANVAS}/generations/{TASK}"),get(||async{Json(task("succeeded"))}))
        .route(&format!("/v2/canvas/projects/{CANVAS}/elements"),post(create)).with_state(s);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, handle)
}
async fn run(root: &Path, base: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_virse"));
    cmd.args(["--canvas", CANVAS, "--org", ORG])
        .args(args)
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", "virse_sk_media_test")
        .env("VIRSE_CONFIG_DIR", root)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output())
        .await
        .unwrap()
        .unwrap()
}
fn output(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[tokio::test]
async fn media_commands_share_references_placement_parameters_and_waiting() {
    let s = state();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for (command, operation, default_width, default_height) in [
        ("image", "image_generate", 1024.0, 1024.0),
        ("video", "video_generate", 512.0, 288.0),
        ("3d", "three_d_generate", 512.0, 512.0),
    ] {
        let out = run(
            dir.path(),
            &base,
            &[
                "generation",
                command,
                "--model",
                "test-model",
                "--prompt",
                "use $1 and $2",
                "--reference",
                SOURCE,
                "--reference",
                OTHER,
                "--parameters",
                r#"{"resolution":"test","duration":2}"#,
                "--wait",
                "--json",
            ],
        )
        .await;
        assert_eq!(output(&out)["status"], "succeeded");
        let s = s.lock().unwrap();
        let body = &s.requests.last().unwrap().2;
        assert_eq!(body["operation"], operation);
        assert_eq!(body["input"]["model"], "test-model");
        assert_eq!(
            body["input"]["references"]["$1"]["source"]["element_id"],
            SOURCE
        );
        assert_eq!(
            body["input"]["references"]["$2"]["source"]["element_id"],
            OTHER
        );
        assert_eq!(body["input"]["parameters"]["duration"], 2);
        assert_eq!(
            body["outputs"][0]["create_element"]["size"]["width"],
            default_width
        );
        assert_eq!(
            body["outputs"][0]["create_element"]["size"]["height"],
            default_height
        );
        assert_eq!(body["outputs"].as_array().unwrap().len(), 1);
    }
    server.abort();
}
#[tokio::test]
async fn image_protocol_stays_compatible_and_target_binding_does_not_create_a_placeholder() {
    let s = state();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    output(
        &run(
            dir.path(),
            &base,
            &[
                "generation",
                "image",
                "--model",
                "test",
                "--prompt",
                "test",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(
        s.lock().unwrap().requests[0].2,
        json!({"operation":"image_generate","input":{"model":"test","user_query":"test","image_count":1,"references":{}},"outputs":[{"output_index":0,"create_element":{"position":{"x":0.0,"y":0.0},"size":{"width":1024.0,"height":1024.0}}}]})
    );
    let file = dir.path().join("params.json");
    std::fs::write(&file, r#"{"duration":2,"generate_audio":false}"#).unwrap();
    output(
        &run(
            dir.path(),
            &base,
            &[
                "generation",
                "video",
                "--model",
                "test",
                "--prompt",
                "test",
                "--target-element",
                ELEMENT,
                "--parameters-file",
                file.to_str().unwrap(),
                "--json",
            ],
        )
        .await,
    );
    let s = s.lock().unwrap();
    assert_eq!(
        s.requests[1].2["outputs"],
        json!([{"output_index":0,"element_id":ELEMENT}])
    );
    assert_eq!(
        s.requests[1].2["input"]["parameters"]["generate_audio"],
        false
    );
    server.abort();
}
#[tokio::test]
async fn three_d_supports_image_only_input_and_invalid_args_do_not_submit() {
    let s = state();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    output(
        &run(
            dir.path(),
            &base,
            &[
                "generation",
                "3d",
                "--model",
                "test",
                "--reference",
                SOURCE,
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(s.lock().unwrap().requests[0].2["input"]["user_query"], "");
    for args in [
        vec!["generation", "3d", "--model", "test"],
        vec!["generation", "video", "--model", "test"],
        vec![
            "generation",
            "video",
            "--model",
            "test",
            "--prompt",
            "test",
            "--parameters",
            "[]",
        ],
        vec![
            "generation",
            "video",
            "--model",
            "test",
            "--prompt",
            "test",
            "--target-element",
            ELEMENT,
            "--x",
            "10",
        ],
        vec![
            "generation",
            "3d",
            "--model",
            "test",
            "--prompt",
            "test",
            "--parameters",
            "{}",
            "--parameters-file",
            "ignored.json",
        ],
    ] {
        assert_eq!(run(dir.path(), &base, &args).await.status.code(), Some(2));
    }
    assert_eq!(s.lock().unwrap().requests.len(), 1);
    server.abort();
}
#[tokio::test]
async fn placing_all_asset_types_uses_server_layout_without_generating() {
    let s = state();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for (kind, subtype, element_type) in [
        ("image", None, "image"),
        ("video", None, "video"),
        ("model3d", None, "3d"),
        ("document", Some("word"), "document"),
        ("document", Some("pdf"), "document"),
    ] {
        {
            let mut s = s.lock().unwrap();
            s.asset_kind = kind.into();
            s.document_type = subtype.map(str::to_owned);
        }
        let out = run(
            dir.path(),
            &base,
            &["canvas", "add", "--asset", SOURCE, "--json"],
        )
        .await;
        assert_eq!(output(&out)["element"]["type"], element_type);
        let s = s.lock().unwrap();
        let request = s.requests.last().unwrap();
        assert_eq!(request.0, "create");
        assert_eq!(
            request.2,
            json!({"org_id":ORG,"type":element_type,"content_ref":{"kind":"asset","id":SOURCE},"layout":{}})
        );
    }
    server.abort();
}
#[tokio::test]
async fn placement_replay_keeps_resolved_source_even_after_path_disappears() {
    let s = state();
    s.lock().unwrap().lose_response = true;
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "canvas",
        "add",
        "--resource",
        "/reference.png",
        "--x",
        "100",
        "--y",
        "200",
        "--width",
        "300",
        "--height",
        "400",
        "--idempotency-key",
        "placement-key",
        "--json",
    ];
    assert_eq!(
        output(&run(dir.path(), &base, &args).await)["element"]["id"],
        ELEMENT
    );
    let reads = {
        let mut s = s.lock().unwrap();
        s.source_removed = true;
        s.source_reads
    };
    assert_eq!(
        output(&run(dir.path(), &base, &args).await)["element"]["id"],
        ELEMENT
    );
    {
        let s = s.lock().unwrap();
        assert_eq!(s.source_reads, reads);
        assert_eq!(s.requests.len(), 3);
        assert!(s.requests.windows(2).all(|a| a[0] == a[1]));
        assert_eq!(s.requests[0].2["name"], "reference.png");
        assert_eq!(s.requests[0].2["layout"]["position"]["x"], 100.0);
    }
    let out = run(
        dir.path(),
        &base,
        &[
            "canvas",
            "add",
            "--asset",
            SOURCE,
            "--idempotency-key",
            "placement-key",
            "--json",
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(s.lock().unwrap().requests.len(), 3);
    server.abort();
}
#[tokio::test]
async fn invalid_sources_and_disabled_api_do_not_fallback_or_create_blanks() {
    let s = state();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for (kind, status) in [
        ("text", "ready"),
        ("image", "processing"),
        ("video", "failed"),
    ] {
        {
            let mut s = s.lock().unwrap();
            s.asset_kind = kind.into();
            s.status = status.into();
        }
        assert!(
            !run(
                dir.path(),
                &base,
                &["canvas", "add", "--asset", SOURCE, "--json"]
            )
            .await
            .status
            .success()
        );
    }
    assert!(s.lock().unwrap().requests.is_empty());
    for args in [
        vec!["canvas", "add"],
        vec![
            "canvas",
            "add",
            "--asset",
            SOURCE,
            "--resource",
            "/reference.png",
        ],
        vec!["canvas", "add", "--asset", SOURCE, "--width", "200"],
    ] {
        assert_eq!(run(dir.path(), &base, &args).await.status.code(), Some(2));
    }
    {
        let mut s = s.lock().unwrap();
        s.asset_kind = "image".into();
        s.status = "ready".into();
        s.disabled = true;
    }
    let out = run(
        dir.path(),
        &base,
        &["canvas", "add", "--asset", SOURCE, "--json"],
    )
    .await;
    assert_eq!(out.status.code(), Some(5));
    let error: Value = serde_json::from_slice(
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .last()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(error["error"]["code"], "CANVAS_HTTP_COMMANDS_NOT_ENABLED");
    assert_eq!(s.lock().unwrap().requests.len(), 1);
    server.abort();
}
