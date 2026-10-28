use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    process::{Output, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};
const CANVAS: &str = "00000000-0000-0000-0000-000000000001";
const ORG: &str = "00000000-0000-0000-0000-000000000002";
const RUN: &str = "00000000-0000-0000-0000-000000000003";
const USER: &str = "00000000-0000-0000-0000-000000000004";
const TOOL: &str = "00000000-0000-0000-0000-000000000005";
const TOKEN: &str = "virse_sk_tool_cli";
#[derive(Default)]
struct Data {
    schemas: Vec<HashMap<String, String>>,
    submits: Vec<(String, Value)>,
    management: Vec<(String, Value)>,
    actions: Vec<String>,
    reads: usize,
    artifact: String,
    lose_response: bool,
    status: String,
}
type Shared = Arc<Mutex<Data>>;
fn state() -> Shared {
    Arc::new(Mutex::new(Data {
        artifact: "artifact-1".into(),
        status: "queued".into(),
        ..Default::default()
    }))
}
fn snapshot(status: &str) -> Value {
    json!({"run_id":RUN,"canvas_id":CANVAS,"kind":"tool","element_id":TOOL,"owner_id":USER,"status":status,"revision":1,"result":{"outputs":[]},"error":null,"actions":{"cancel":status=="queued","resume":false,"interrupt":status=="running"},"tool":{"skill_id":TOOL,"version_id":"artifact-1"}})
}
async fn serve(s: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let app=Router::new()
        .route("/auth/me",get(||async{Json(json!({"id":USER}))}))
        .route(&format!("/v2/skills/{TOOL}/input-schema"),get(|State(s):State<Shared>,Query(q):Query<HashMap<String,String>>,headers:HeaderMap|async move{
            assert_eq!(headers["authorization"],format!("Bearer {TOKEN}")); assert_eq!(q["canvas_id"],CANVAS); assert_eq!(q["org_id"],ORG);
            let mut s=s.lock().unwrap();s.schemas.push(q.clone());
            Json(json!({"skill_id":TOOL,"version_id":q.get("version_id").unwrap_or(&s.artifact),"version":"internal","input_schema":{"version":1,"fields":[]}}))
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/tool-runs"),post(|State(s):State<Shared>,headers:HeaderMap,Json(body):Json<Value>|async move{
            assert_eq!(headers["authorization"],format!("Bearer {TOKEN}")); assert_eq!(headers["x-virse-expected-user"],USER);
            let mut s=s.lock().unwrap();s.submits.push((headers["idempotency-key"].to_str().unwrap().into(),body));
            if s.lose_response && s.submits.len()==1 {return(StatusCode::ACCEPTED,"broken").into_response();}
            (StatusCode::ACCEPTED,Json(snapshot(&s.status))).into_response()
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/tool-runs/{RUN}"),get(|State(s):State<Shared>,Query(q):Query<HashMap<String,String>>|async move{
            assert_eq!(q["org_id"],ORG);let mut s=s.lock().unwrap();s.reads+=1;Json(snapshot(&s.status))
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/tool-runs/{RUN}/{{action}}"),post(|State(s):State<Shared>,AxumPath(action):AxumPath<String>,Query(q):Query<HashMap<String,String>>|async move{
            assert_eq!(q["org_id"],ORG);s.lock().unwrap().actions.push(action.clone());
            match action.as_str(){"cancel"=>(StatusCode::CONFLICT,Json(json!({"detail":"CANVAS_RUN_ALREADY_DISPATCHED"}))),"interrupt"=>(StatusCode::ACCEPTED,Json(snapshot("interrupting"))),_ =>(StatusCode::ACCEPTED,Json(snapshot("queued")))}
        }))
        .route(&format!("/canvas/ws/{CANVAS}"),get(|headers:HeaderMap,ws:WebSocketUpgrade|async move{
            assert_eq!(headers["authorization"],format!("Bearer {TOKEN}"));ws.on_upgrade(|mut socket|async move{
                socket.send(Message::Text(json!({"type":"active_users","users":[]}).to_string().into())).await.unwrap();
                while let Some(Ok(message))=socket.recv().await {if let Message::Text(text)=message {assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["type"],"ping");socket.send(Message::Text(json!({"type":"pong"}).to_string().into())).await.unwrap();}}
            })
        }))
        .route("/skills/{*path}",post(|State(s):State<Shared>,AxumPath(path):AxumPath<String>,Json(body):Json<Value>|async move{
            s.lock().unwrap().management.push((path.clone(),body));
            Json(if path.ends_with("versions"){json!({"versions":[{"id":"release-2","artifactVersionId":"artifact-1","version":"2.0.0","current":true}]})}else{json!({"skills":[],"savedSkillIds":[]})})
        })).with_state(s);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    (
        base,
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    )
}
fn command(base: &str, root: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_virse"));
    c.args(args)
        .env("VIRSE_CONFIG_DIR", root)
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", TOKEN)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}
async fn cli(base: &str, root: &Path, args: &[&str]) -> Output {
    tokio::time::timeout(Duration::from_secs(15), command(base, root, args).output())
        .await
        .unwrap()
        .unwrap()
}
fn value(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn run_args(key: &str) -> Vec<&str> {
    vec![
        "tool",
        "run",
        TOOL,
        "--canvas",
        CANVAS,
        "--org",
        ORG,
        "--instruction",
        "hello",
        "--idempotency-key",
        key,
        "--json",
    ]
}
#[tokio::test]
async fn durable_replay_keeps_artifact_across_processes_and_lost_response() {
    let root = tempfile::tempdir().unwrap();
    let s = state();
    s.lock().unwrap().lose_response = true;
    let (base, server) = serve(s.clone()).await;
    let mut args = run_args("stable-key");
    args.extend(["--version", "2.0.0", "--x", "42", "--width", "500"]);
    assert_eq!(value(&cli(&base, root.path(), &args).await)["run_id"], RUN);
    {
        let mut s = s.lock().unwrap();
        s.artifact = "artifact-2".into();
    }
    assert_eq!(value(&cli(&base, root.path(), &args).await)["run_id"], RUN);
    {
        let s = s.lock().unwrap();
        assert_eq!(s.schemas.len(), 2);
        assert_eq!(s.schemas[0]["version"], "2.0.0");
        assert_eq!(s.schemas[1]["version_id"], "artifact-1");
        assert_eq!(s.submits.len(), 3);
        assert!(
            s.submits
                .iter()
                .all(|(key, body)| key == "stable-key" && body == &s.submits[0].1)
        );
        assert_eq!(s.submits[0].1["tool"]["version_id"], "artifact-1");
        assert_eq!(
            s.submits[0].1["target"]["create_element"]["layout"]["position"]["x"],
            42.0
        );
        assert_eq!(
            s.submits[0].1["target"]["create_element"]["version_id"],
            "artifact-1"
        );
    }
    let mut changed = args.clone();
    let index = changed.iter().position(|s| *s == "hello").unwrap();
    changed[index] = "different";
    let out = cli(&base, root.path(), &changed).await;
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("another tool run"));
    assert_eq!(s.lock().unwrap().submits.len(), 3);
    for file in std::fs::read_dir(root.path()).unwrap() {
        let path = file.unwrap().path();
        if path.extension().is_some_and(|s| s == "json") {
            let bytes = std::fs::read(&path).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains(TOKEN));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
                    0
                );
            }
        }
    }
    server.abort();
}
#[tokio::test]
async fn stdin_inputs_and_existing_target_use_only_v2_contract() {
    let root = tempfile::tempdir().unwrap();
    let s = state();
    let (base, server) = serve(s.clone()).await;
    let mut args = run_args("input-key");
    args.extend([
        "--inputs",
        "-",
        "--target-element",
        TOOL,
        "--version-id",
        "artifact-1",
    ]);
    let mut child = command(&base, root.path(), &args)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"text":"example","image":{"source":{"kind":"canvas_element","element_id":"00000000-0000-0000-0000-000000000004"}}}"#).await.unwrap();
    value(&child.wait_with_output().await.unwrap());
    let s = s.lock().unwrap();
    assert_eq!(s.schemas.len(), 1);
    let b = &s.submits[0].1;
    assert_eq!(b["inputs"]["text"], "example");
    assert_eq!(b["target"]["element_id"], TOOL);
    assert!(b["target"].get("create_element").is_none());
    server.abort();
}
#[tokio::test]
async fn invalid_inputs_and_layout_do_not_submit() {
    let root = tempfile::tempdir().unwrap();
    let s = state();
    let (base, server) = serve(s.clone()).await;
    for extra in [
        vec!["--target-element", TOOL, "--x", "4"],
        vec!["--width", "0"],
        vec!["--x", "NaN"],
        vec!["--version", "2.0.0", "--version-id", "artifact-1"],
    ] {
        let mut args = run_args("bad");
        args.extend(extra);
        assert!(!cli(&base, root.path(), &args).await.status.success());
    }
    let path = root.path().join("inputs.json");
    std::fs::write(&path, "[]").unwrap();
    let mut args = run_args("bad");
    args.extend(["--inputs", path.to_str().unwrap()]);
    assert!(!cli(&base, root.path(), &args).await.status.success());
    assert!(s.lock().unwrap().submits.is_empty());
    assert!(s.lock().unwrap().schemas.is_empty());
    server.abort();
}
#[tokio::test]
async fn commands_preserve_management_context_and_control_truth() {
    let root = tempfile::tempdir().unwrap();
    let s = state();
    let (base, server) = serve(s.clone()).await;
    for view in ["created", "mine", "store"] {
        value(
            &cli(
                &base,
                root.path(),
                &["tool", "list", "--view", view, "--org", ORG, "--json"],
            )
            .await,
        );
    }
    value(
        &cli(
            &base,
            root.path(),
            &["tool", "scope", TOOL, "team", "--org", ORG],
        )
        .await,
    );
    for action in [
        "add", "remove", "enable", "disable", "star", "unstar", "delete",
    ] {
        value(&cli(&base, root.path(), &["tool", action, TOOL, "--org", ORG]).await);
    }
    {
        let s = s.lock().unwrap();
        assert_eq!(s.management.len(), 11);
        assert_eq!(s.management[3].1["accessScope"], "team");
        assert!(s.management.iter().all(|(_, b)| b["organizationId"] == ORG));
    }
    let interrupted = value(
        &cli(
            &base,
            root.path(),
            &[
                "tool",
                "runs",
                "interrupt",
                RUN,
                "--canvas",
                CANVAS,
                "--org",
                ORG,
            ],
        )
        .await,
    );
    assert_eq!(interrupted["status"], "interrupting");
    assert_eq!(interrupted["actions"]["cancel"], false);
    let cancelled = cli(
        &base,
        root.path(),
        &[
            "tool", "runs", "cancel", RUN, "--canvas", CANVAS, "--org", ORG,
        ],
    )
    .await;
    assert!(!cancelled.status.success());
    assert!(String::from_utf8_lossy(&cancelled.stderr).contains("CANVAS_RUN_ALREADY_DISPATCHED"));
    let resumed = value(
        &cli(
            &base,
            root.path(),
            &[
                "tool", "runs", "resume", RUN, "--canvas", CANVAS, "--org", ORG,
            ],
        )
        .await,
    );
    assert_eq!(resumed["status"], "queued");
    server.abort();
}
#[tokio::test]
async fn watching_timeout_and_needs_attention_have_distinct_outcomes() {
    let root = tempfile::tempdir().unwrap();
    let s = state();
    s.lock().unwrap().status = "running".into();
    let (base, server) = serve(s.clone()).await;
    let args = [
        "tool",
        "runs",
        "watch",
        RUN,
        "--canvas",
        CANVAS,
        "--org",
        ORG,
        "--timeout",
        "1",
        "--json",
    ];
    let out = cli(&base, root.path(), &args).await;
    assert_eq!(out.status.code(), Some(8));
    assert!(s.lock().unwrap().actions.is_empty());
    assert_eq!(s.lock().unwrap().reads, 1);
    let event: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(event["type"], "run_snapshot");
    s.lock().unwrap().status = "needs_attention".into();
    let out = cli(&base, root.path(), &args).await;
    assert_eq!(out.status.code(), Some(7));
    let event: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(event["run"]["status"], "needs_attention");
    assert!(s.lock().unwrap().actions.is_empty());
    server.abort();
}
#[cfg(unix)]
#[tokio::test]
async fn ctrl_c_stops_wait_without_any_control_request() {
    let root = tempfile::tempdir().unwrap();
    let s = state();
    s.lock().unwrap().status = "running".into();
    let (base, server) = serve(s.clone()).await;
    let child = command(
        &base,
        root.path(),
        &[
            "tool",
            "runs",
            "watch",
            RUN,
            "--canvas",
            CANVAS,
            "--org",
            ORG,
            "--timeout",
            "30",
            "--json",
        ],
    )
    .spawn()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if s.lock().unwrap().reads > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().unwrap().to_string()])
            .status()
            .await
            .unwrap()
            .success()
    );
    let out = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out.status.code(), Some(130));
    assert!(s.lock().unwrap().actions.is_empty());
    server.abort();
}
#[tokio::test]
async fn unsupported_list_pagination_is_not_silently_ignored() {
    let root = tempfile::tempdir().unwrap();
    let s = state();
    let (base, server) = serve(s.clone()).await;
    for args in [
        vec!["tool", "list", "--view", "mine", "--limit", "20"],
        vec!["tool", "list", "--view", "created", "--limit", "501"],
        vec!["tool", "list", "--view", "mine", "--category", "x"],
    ] {
        assert!(!cli(&base, root.path(), &args).await.status.success());
    }
    assert!(s.lock().unwrap().management.is_empty());
    server.abort();
}

#[tokio::test]
async fn offline_authoring_dispatch_does_not_resolve_credentials_or_backend() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("authoring");
    let mut init = Command::new(env!("CARGO_BIN_EXE_virse"));
    let output = init
        .args([
            "tool",
            "init",
            project.to_str().unwrap(),
            "--name",
            "Offline tool",
            "--json",
        ])
        .env("VIRSE_BACKEND_URL", "this is deliberately not a URL")
        .env("VIRSE_CONFIG_DIR", "")
        .env_remove("VIRSE_API_TOKEN")
        .output()
        .await
        .unwrap();
    value(&output);
    let output = Command::new(env!("CARGO_BIN_EXE_virse"))
        .args(["tool", "validate", project.to_str().unwrap(), "--json"])
        .env("VIRSE_BACKEND_URL", "")
        .env("VIRSE_CONFIG_DIR", "")
        .env_remove("VIRSE_API_TOKEN")
        .output()
        .await
        .unwrap();
    value(&output);
}

#[tokio::test]
async fn draft_abandon_is_explicit_single_attempt_and_preserves_current_release() {
    #[derive(Default)]
    struct DraftState {
        posts: Vec<(String, Value)>,
        current_release: String,
    }
    for outcome in ["success", "lost_response", "forbidden", "unavailable"] {
        let root = tempfile::tempdir().unwrap();
        let state = Arc::new(Mutex::new(DraftState {
            current_release: "published-release".into(),
            ..Default::default()
        }));
        let app = Router::new()
            .route(
                "/skills/{*path}",
                post(
                    move |State(state): State<Arc<Mutex<DraftState>>>,
                          AxumPath(path): AxumPath<String>,
                          headers: HeaderMap,
                          Json(body): Json<Value>| async move {
                        assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                        assert!(!headers.contains_key("idempotency-key"));
                        let mut state = state.lock().unwrap();
                        state.posts.push((path.clone(), body));
                        if path != format!("{TOOL}/draft/abandon") {
                            state.current_release = "unexpected-write".into();
                            return StatusCode::NOT_FOUND.into_response();
                        }
                        match outcome {
                            "lost_response" => (StatusCode::OK, "truncated").into_response(),
                            "forbidden" => (
                                StatusCode::FORBIDDEN,
                                Json(json!({"code":"SKILL_NOT_OWNER","message":"not owner"})),
                            )
                                .into_response(),
                            "unavailable" => (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(json!({"code":"SYSTEM_ERROR","message":"unavailable"})),
                            )
                                .into_response(),
                            _ => Json(json!({"success":true,"revisionId":RUN})).into_response(),
                        }
                    },
                ),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let output = cli(
            &base,
            root.path(),
            &[
                "tool", "draft", "abandon", TOOL, RUN, "--org", ORG, "--json",
            ],
        )
        .await;
        match outcome {
            "success" => assert_eq!(value(&output), json!({"success":true,"revisionId":RUN})),
            "forbidden" => assert_eq!(output.status.code(), Some(4)),
            _ => {
                assert_eq!(output.status.code(), Some(7));
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("TOOL_MUTATION_UNCERTAIN")
                );
                assert!(output.stdout.is_empty());
            }
        }
        let state = state.lock().unwrap();
        assert_eq!(state.current_release, "published-release");
        assert_eq!(
            state.posts,
            vec![(
                format!("{TOOL}/draft/abandon"),
                json!({"skillId":TOOL,"revisionId":RUN,"organizationId":ORG})
            )]
        );
        server.abort();
    }
}
