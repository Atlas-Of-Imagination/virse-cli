use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{io::AsyncWriteExt, process::Command};
const CANVAS: &str = "00000000-0000-0000-0000-000000000001";
const TASK: &str = "00000000-0000-0000-0000-000000000002";
const TOKEN: &str = "virse_sk_cli_test";
fn cmd(dir: &Path, backend: Option<&str>, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_virse"));
    cmd.args(args)
        .env_remove("VIRSE_BACKEND_URL")
        .env_remove("VIRSE_API_TOKEN")
        .env("VIRSE_CONFIG_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(backend) = backend {
        cmd.env("VIRSE_BACKEND_URL", backend);
    }
    cmd
}
async fn run(mut cmd: Command) -> Output {
    tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .unwrap()
        .unwrap()
}
async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, server)
}
fn value(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "invalid output: {} / {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}
fn snapshot(status: &str) -> Value {
    json!({"task_id":TASK,"project_id":CANVAS,"operation":"image_generate","status":status,"revision":1,"outputs":[],"actions":{"cancel":false,"retry":false}})
}

#[tokio::test]
async fn configuration_has_only_environment_backend_override() {
    let dir = tempfile::tempdir().unwrap();
    let set = run(cmd(dir.path(), None, &["config", "set", "canvas", CANVAS])).await;
    assert!(set.status.success());
    let out = run(cmd(
        dir.path(),
        Some("https://api.virse.ai/"),
        &["config", "get", "--json"],
    ))
    .await;
    assert_eq!(value(&out)["canvas"], CANVAS);
    let out = run(cmd(
        dir.path(),
        Some("https://dev4.virse.ai"),
        &["config", "get", "--json"],
    ))
    .await;
    assert!(value(&out)["canvas"].is_null());
    for backend in [
        "",
        "ftp://x",
        "https://user:secret@host",
        "https://host/path",
    ] {
        assert_eq!(
            run(cmd(dir.path(), Some(backend), &["config", "get"]))
                .await
                .status
                .code(),
            Some(2)
        );
    }
    for args in [
        vec!["--backend-url", "https://x", "config", "get"],
        vec!["--backend", "https://x", "config", "get"],
        vec!["config", "set", "backend", "https://x"],
    ] {
        assert_eq!(
            run(cmd(dir.path(), None, &args)).await.status.code(),
            Some(2)
        );
    }
    let invalid = run(cmd(
        dir.path(),
        None,
        &["--json", "--backend-url", "https://x", "config", "get"],
    ))
    .await;
    let error: Value = serde_json::from_slice(&invalid.stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    let help = run(cmd(dir.path(), Some(""), &["--help"])).await;
    assert!(help.status.success());
}

#[tokio::test]
async fn login_persists_private_credentials_isolated_by_backend_and_profile() {
    let app = Router::new().route("/client-compatibility", get(||async{Json(json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"}))}))
        .route(
            "/auth/cli/start",
            post(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"detail":{"code":"CLI_AUTH_NOT_CONFIGURED"}})),
                )
            }),
        )
        .route(
            "/auth/me",
            get(|headers: HeaderMap| async move {
                if headers.get("authorization").unwrap().to_str().unwrap()
                    == format!("Bearer {TOKEN}")
                {
                    (StatusCode::OK, Json(json!({"status":1,"id":"user"})))
                } else {
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"detail":"Invalid token"})),
                    )
                }
            }),
        );
    let (base, server) = serve(app).await;
    let dir = tempfile::tempdir().unwrap();
    let mut command = cmd(
        dir.path(),
        Some(&base),
        &["auth", "login", "--token-stdin", "--json"],
    );
    command.stdin(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{TOKEN}\n").as_bytes())
        .await
        .unwrap();
    let out = child.wait_with_output().await.unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(TOKEN));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path().join("credentials.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert!(
        run(cmd(dir.path(), Some(&base), &["auth", "status"]))
            .await
            .status
            .success()
    );
    assert_eq!(
        run(cmd(
            dir.path(),
            Some(&base),
            &["--profile", "other", "auth", "status"]
        ))
        .await
        .status
        .code(),
        Some(3)
    );
    assert_eq!(
        run(cmd(
            dir.path(),
            Some("http://127.0.0.1:1"),
            &["auth", "status"]
        ))
        .await
        .status
        .code(),
        Some(3)
    );
    let mut override_cmd = cmd(dir.path(), Some(&base), &["auth", "status"]);
    override_cmd.env("VIRSE_API_TOKEN", "virse_sk_wrong");
    assert_eq!(run(override_cmd).await.status.code(), Some(3));
    assert!(
        run(cmd(dir.path(), Some(&base), &["auth", "logout"]))
            .await
            .status
            .success()
    );
    assert_eq!(
        run(cmd(dir.path(), Some(&base), &["auth", "status"]))
            .await
            .status
            .code(),
        Some(3)
    );
    assert_eq!(
        run(cmd(
            dir.path(),
            Some(&base),
            &["auth", "login", "--no-browser"]
        ))
        .await
        .status
        .code(),
        Some(5)
    );
    server.abort();
}

#[derive(Clone, Default)]
struct Calls {
    submissions: Arc<Mutex<Vec<(String, Value)>>>,
    cancels: Arc<AtomicUsize>,
}
async fn submit(
    State(calls): State<Calls>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let mut calls = calls.submissions.lock().unwrap();
    calls.push((headers["idempotency-key"].to_str().unwrap().into(), body));
    if calls.len() == 1 {
        (StatusCode::ACCEPTED, "truncated accepted response").into_response()
    } else {
        (StatusCode::ACCEPTED, Json(snapshot("queued"))).into_response()
    }
}
fn app(calls: Calls, status: &'static str) -> Router {
    Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) }))
        .route(
            "/auth/me",
            get(|| async { Json(json!({"status":1,"id":"user"})) }),
        )
        .route(
            &format!("/v2/canvas/projects/{CANVAS}/generations"),
            post(submit),
        )
        .route(
            &format!("/v2/canvas/projects/{CANVAS}/generations/{TASK}"),
            get(move || async move { Json(snapshot(status)) }),
        )
        .route(
            &format!("/v2/canvas/projects/{CANVAS}/generations/{TASK}/cancel"),
            post(|State(c): State<Calls>| async move {
                c.cancels.fetch_add(1, Ordering::SeqCst);
                Json(snapshot("cancelled"))
            }),
        )
        .with_state(calls)
}
#[tokio::test]
async fn combined_image_retries_same_key_and_records_no_prompt() {
    let calls = Calls::default();
    let (base, server) = serve(app(calls.clone(), "succeeded")).await;
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "generation",
        "image",
        "--canvas",
        CANVAS,
        "--model",
        "test-model",
        "--prompt",
        "private prompt $1",
        "--reference",
        TASK,
        "--wait",
        "--json",
        "--idempotency-key",
        "recover-key",
    ];
    let mut command = cmd(dir.path(), Some(&base), &args);
    command.env("VIRSE_API_TOKEN", TOKEN);
    let out = run(command).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(value(&out)["status"], "succeeded");
    {
        let submitted = calls.submissions.lock().unwrap();
        assert_eq!(submitted.len(), 2);
        assert_eq!(submitted[0], submitted[1]);
        assert_eq!(submitted[0].0, "recover-key");
        assert!(submitted[0].1["outputs"][0].get("create_element").is_some());
        assert_eq!(
            submitted[0].1["input"]["references"]["$1"]["source"]["element_id"],
            TASK
        );
    }
    for file in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(file.unwrap().path()).unwrap();
        let content = String::from_utf8_lossy(&bytes);
        assert!(!content.contains("private prompt"));
        assert!(!content.contains(TOKEN));
    }
    let mut args = args;
    args[7] = "different prompt";
    let mut command = cmd(dir.path(), Some(&base), &args);
    command.env("VIRSE_API_TOKEN", TOKEN);
    assert_eq!(run(command).await.status.code(), Some(2));
    assert_eq!(calls.submissions.lock().unwrap().len(), 2);
    server.abort();
}

#[tokio::test]
async fn wait_exit_codes_and_timeout_do_not_cancel_tasks() {
    for (status, code) in [
        ("succeeded", 0),
        ("failed", 6),
        ("partially_succeeded", 6),
        ("cancelled", 6),
        ("needs_attention", 7),
        ("running", 8),
    ] {
        let calls = Calls::default();
        let (base, server) = serve(app(calls.clone(), status)).await;
        let dir = tempfile::tempdir().unwrap();
        let mut command = cmd(
            dir.path(),
            Some(&base),
            &[
                "generation",
                "watch",
                TASK,
                "--canvas",
                CANVAS,
                "--timeout",
                "1",
                "--json",
            ],
        );
        command.env("VIRSE_API_TOKEN", TOKEN);
        let out = run(command).await;
        assert_eq!(
            out.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        if code != 8 {
            assert_eq!(value(&out)["task"]["status"], status);
        }
        assert_eq!(calls.cancels.load(Ordering::SeqCst), 0);
        let mut command = cmd(
            dir.path(),
            Some(&base),
            &["generation", "get", TASK, "--canvas", CANVAS],
        );
        command.env("VIRSE_API_TOKEN", TOKEN);
        assert!(run(command).await.status.success());
        server.abort();
    }
}

#[tokio::test]
async fn asset_download_is_atomic_and_never_overwrites() {
    let app=Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).route(&format!("/api/v1/assets/{TASK}/download"),get(||async {Json(json!({"request_id":"r","data":{"url":"/bytes","filename":"image.png","delivery":"authenticated_stream"}}))}))
        .route("/bytes",get(|request:Request<Body>|async move {assert_eq!(request.headers()["authorization"],format!("Bearer {TOKEN}"));"actual image bytes"}));
    let (base, server) = serve(app).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.png");
    let args = [
        "asset",
        "download",
        TASK,
        "--canvas",
        CANVAS,
        "--org",
        CANVAS,
        "--output",
        path.to_str().unwrap(),
        "--json",
    ];
    let mut command = cmd(dir.path(), Some(&base), &args);
    command.env("VIRSE_API_TOKEN", TOKEN);
    let out = run(command).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"actual image bytes");
    assert_eq!(value(&out)["bytes"], 18);
    let mut command = cmd(dir.path(), Some(&base), &args);
    command.env("VIRSE_API_TOKEN", TOKEN);
    assert!(!run(command).await.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), b"actual image bytes");
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn ctrl_c_only_stops_observation() {
    let calls = Calls::default();
    let (base, server) = serve(app(calls.clone(), "running")).await;
    let dir = tempfile::tempdir().unwrap();
    let mut command = cmd(
        dir.path(),
        Some(&base),
        &["generation", "watch", TASK, "--canvas", CANVAS],
    );
    command.env("VIRSE_API_TOKEN", TOKEN);
    let child = command.spawn().unwrap();
    let pid = child.id().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let signal = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .await
        .unwrap();
    assert!(signal.success());
    let out = child.wait_with_output().await.unwrap();
    assert_eq!(out.status.code(), Some(130));
    assert_eq!(calls.cancels.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn uncertain_submission_returns_key_after_bounded_retries() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let app = Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) }))
        .route(
            "/auth/me",
            get(|| async { Json(json!({"status":1,"id":"user"})) }),
        )
        .route(
            &format!("/v2/canvas/projects/{CANVAS}/generations"),
            post(move |headers: HeaderMap| {
                let count = count.clone();
                async move {
                    assert_eq!(headers["idempotency-key"], "uncertain-key");
                    count.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::ACCEPTED, "truncated")
                }
            }),
        );
    let (base, server) = serve(app).await;
    let dir = tempfile::tempdir().unwrap();
    let mut command = cmd(
        dir.path(),
        Some(&base),
        &[
            "generation",
            "image",
            "--canvas",
            CANVAS,
            "--model",
            "test",
            "--prompt",
            "test",
            "--idempotency-key",
            "uncertain-key",
            "--json",
        ],
    );
    command.env("VIRSE_API_TOKEN", TOKEN);
    let out = run(command).await;
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert!(out.stdout.is_empty());
    let lines = String::from_utf8_lossy(&out.stderr);
    let error: Value = serde_json::from_str(lines.lines().last().unwrap()).unwrap();
    assert_eq!(error["error"]["idempotency_key"], "uncertain-key");
    server.abort();
}

#[tokio::test]
async fn generic_stdin_submission_binds_existing_element() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let app = Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) }))
        .route(
            "/auth/me",
            get(|| async { Json(json!({"status":1,"id":"user"})) }),
        )
        .route(
            &format!("/v2/canvas/projects/{CANVAS}/generations"),
            post(move |Json(body): Json<Value>| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["operation"], "text_generate");
                    assert_eq!(body["outputs"][0]["element_id"], CANVAS);
                    assert!(body["outputs"][0].get("create_element").is_none());
                    (StatusCode::ACCEPTED, Json(snapshot("queued")))
                }
            }),
        );
    let (base, server) = serve(app).await;
    let dir = tempfile::tempdir().unwrap();
    let mut command = cmd(
        dir.path(),
        Some(&base),
        &[
            "generation",
            "submit",
            "--canvas",
            CANVAS,
            "--file",
            "-",
            "--json",
        ],
    );
    command.env("VIRSE_API_TOKEN", TOKEN).stdin(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(json!({"operation":"text_generate","input":{"user_query":"hello"},"outputs":[{"output_index":0,"element_id":CANVAS}]}).to_string().as_bytes()).await.unwrap();
    let out = child.wait_with_output().await.unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(value(&out)["status"], "queued");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}
