use axum::{
    Json, Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{CloseFrame, Message},
    },
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    process::{Output, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::process::Command;
use uuid::Uuid;

const ORG: &str = "00000000-0000-0000-0000-000000000001";
const TOKEN: &str = "virse_sk_watch_cli_test";

#[derive(Default)]
struct Mock {
    responses: VecDeque<(u16, Value)>,
    scripts: VecDeque<Vec<Message>>,
    cursors: Vec<Option<String>>,
    sockets: usize,
}
type Shared = Arc<Mutex<Mock>>;
fn page(cursor: &str, bootstrap: bool, items: Value) -> (u16, Value) {
    (
        200,
        json!({"request_id":"test","data":{"items":items,"next_cursor":cursor,"has_more":false,"bootstrap":bootstrap}}),
    )
}
fn ready() -> Message {
    Message::Text(
        json!({"type":"asset_changes_ready","org_id":ORG})
            .to_string()
            .into(),
    )
}
fn revoked() -> Message {
    Message::Text(
        json!({"type":"workspace_access_revoked","clear_cache":true})
            .to_string()
            .into(),
    )
}
fn change(kind: &str) -> Value {
    json!({"event_id":Uuid::new_v4(),"kind":kind,"occurred_at":"2026-10-28T00:00:00Z"})
}
async fn serve(mock: Mock) -> (String, Shared, tokio::task::JoinHandle<()>) {
    let shared = Arc::new(Mutex::new(mock));
    let app = Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) }))
        .route(
            "/api/v1/changes",
            get(
                |State(shared): State<Shared>,
                 Query(query): Query<HashMap<String, String>>,
                 headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                    assert_eq!(query["org_id"], ORG);
                    assert!(!query.contains_key("token"));
                    let mut state = shared.lock().unwrap();
                    state.cursors.push(query.get("cursor").cloned());
                    let (status, body) = state
                        .responses
                        .pop_front()
                        .expect("unexpected request/poll");
                    (StatusCode::from_u16(status).unwrap(), Json(body))
                },
            ),
        )
        .route(
            "/api/v1/changes/ws",
            get(
                |State(shared): State<Shared>,
                 Query(query): Query<HashMap<String, String>>,
                 headers: HeaderMap,
                 ws: WebSocketUpgrade| async move {
                    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                    assert_eq!(query, HashMap::from([("org_id".into(), ORG.into())]));
                    let script = {
                        let mut state = shared.lock().unwrap();
                        state.sockets += 1;
                        state.scripts.pop_front().unwrap_or_default()
                    };
                    ws.on_upgrade(move |mut socket| async move {
                        for message in script {
                            let closing = matches!(message, Message::Close(_));
                            if socket.send(message).await.is_err() || closing {
                                return;
                            }
                        }
                        while socket.recv().await.is_some() {}
                    })
                },
            ),
        )
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, shared, server)
}
fn command(root: &Path, base: &str, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_virse"));
    command
        .args(["--org", ORG, "resource", "watch"])
        .args(args)
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", TOKEN)
        .env("VIRSE_CONFIG_DIR", root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}
async fn run(mut command: Command) -> Output {
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .unwrap()
        .unwrap()
}
fn lines(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn error(output: &Output) -> Value {
    serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"].clone()
}

#[tokio::test]
async fn ndjson_delivers_baseline_and_scope_events_with_their_cursors_then_stops_on_revocation() {
    let (base, shared, server) = serve(Mock {
        responses: VecDeque::from([
            page("baseline", true, json!([])),
            page(
                "delivered",
                false,
                json!([change("created"), change("scope_invalidated")]),
            ),
        ]),
        scripts: VecDeque::from([vec![ready(), revoked()]]),
        ..Mock::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let output = run(command(dir.path(), &base, &["--json"])).await;
    assert_eq!(output.status.code(), Some(4));
    let lines = lines(&output);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["type"], "page");
    assert_eq!(lines[0]["bootstrap"], true);
    assert_eq!(lines[0]["next_cursor"], "baseline");
    assert_eq!(lines[1]["next_cursor"], "delivered");
    assert_eq!(lines[1]["items"][1]["kind"], "scope_invalidated");
    assert!(lines[1]["items"][1]["resource_id"].is_null());
    assert_eq!(error(&output)["code"], "workspace_access_revoked");
    assert_eq!(
        shared.lock().unwrap().cursors,
        vec![None, Some("baseline".into())]
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN));
    server.abort();
}

#[tokio::test]
async fn human_output_explains_baseline_and_scope_invalidation() {
    let (base, _, server) = serve(Mock {
        responses: VecDeque::from([
            page("baseline", true, json!([])),
            page("updated", false, json!([change("scope_invalidated")])),
        ]),
        scripts: VecDeque::from([vec![ready(), revoked()]]),
        ..Mock::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let output = run(command(dir.path(), &base, &[])).await;
    assert_eq!(output.status.code(), Some(4));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Baseline cursor: baseline"));
    assert!(text.contains("no initial resource listing"));
    assert!(text.contains("scope_invalidated"));
    assert!(text.contains("refresh your resource listing"));
    assert!(text.contains("Cursor: updated"));
    server.abort();
}

#[tokio::test]
async fn disabled_service_fails_without_socket_or_polling_and_reset_requires_full_resync() {
    for (status, code, cursor, exit) in [
        (503, "changes_not_enabled", None, 5),
        (409, "cursor_expired", Some("expired"), 1),
        (409, "sync_reset_required", Some("old-era"), 1),
    ] {
        let (base,shared,server)=serve(Mock {
            responses:VecDeque::from([(status,json!({"request_id":"rejected","error":{"code":code,"message":"Rejected","retryable":false}}))]),
            scripts:VecDeque::from([vec![ready()]]),
            ..Mock::default()
        }).await;
        let dir = tempfile::tempdir().unwrap();
        let mut args = vec!["--json"];
        if let Some(cursor) = cursor {
            args.extend(["--cursor", cursor]);
        }
        let output = run(command(dir.path(), &base, &args)).await;
        assert_eq!(output.status.code(), Some(exit));
        assert!(output.stdout.is_empty());
        assert_eq!(error(&output)["code"], code);
        if status == 409 {
            assert!(
                error(&output)["message"]
                    .as_str()
                    .unwrap()
                    .contains("full resource resync")
            );
        }
        assert_eq!(
            shared.lock().unwrap().cursors,
            vec![cursor.map(str::to_owned)]
        );
        assert_eq!(
            shared.lock().unwrap().sockets,
            usize::from(cursor.is_some())
        );
        server.abort();
    }
}

#[tokio::test]
async fn explicit_cursor_reconnect_notices_keep_checkpoint_and_never_bootstrap() {
    let (base, shared, server) = serve(Mock {
        responses: VecDeque::from([
            page("first", false, json!([change("renamed")])),
            page("second", false, json!([change("moved")])),
        ]),
        scripts: VecDeque::from([
            vec![
                ready(),
                Message::Close(Some(CloseFrame {
                    code: 1000,
                    reason: "test".into(),
                })),
            ],
            vec![ready(), revoked()],
        ]),
        ..Mock::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let output = run(command(
        dir.path(),
        &base,
        &["--cursor", "resume", "--json"],
    ))
    .await;
    assert_eq!(output.status.code(), Some(4));
    let lines = lines(&output);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["next_cursor"], "first");
    assert_eq!(lines[1]["type"], "notice");
    assert_eq!(lines[1]["notice"], "reconnecting");
    assert_eq!(lines[1]["cursor"], "first");
    assert_eq!(lines[2]["next_cursor"], "second");
    assert_eq!(
        shared.lock().unwrap().cursors,
        vec![Some("resume".into()), Some("first".into())]
    );
    server.abort();
}

#[tokio::test]
async fn timeout_is_successful_duration_with_no_idle_http_polling_or_terminal_json_mixing() {
    let (base, shared, server) = serve(Mock {
        responses: VecDeque::from([page("resume", false, json!([]))]),
        scripts: VecDeque::from([vec![ready()]]),
        ..Mock::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let output = run(command(
        dir.path(),
        &base,
        &["--cursor", "resume", "--timeout", "1", "--json"],
    ))
    .await;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let lines = lines(&output);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["next_cursor"], "resume");
    assert_eq!(shared.lock().unwrap().cursors, vec![Some("resume".into())]);
    assert_eq!(shared.lock().unwrap().sockets, 1);
    server.abort();
}

#[tokio::test]
async fn rejects_missing_org_empty_cursor_and_zero_duration_before_network() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["--cursor", "", "--json"],
        vec!["--timeout", "0", "--json"],
    ] {
        let output = run(command(dir.path(), "http://127.0.0.1:1", &args)).await;
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(error(&output)["code"], "INVALID_INPUT");
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_virse"));
    cmd.args(["resource", "watch", "--json"])
        .env("VIRSE_BACKEND_URL", "http://127.0.0.1:1")
        .env("VIRSE_CONFIG_DIR", dir.path())
        .env("VIRSE_API_TOKEN", TOKEN)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = run(cmd).await;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        error(&output)["message"]
            .as_str()
            .unwrap()
            .contains("--org")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn ctrl_c_exits_130_after_flushing_the_last_delivered_checkpoint() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let (base, _, server) = serve(Mock {
        responses: VecDeque::from([
            page("baseline", true, json!([])),
            page("baseline", false, json!([])),
        ]),
        scripts: VecDeque::from([vec![ready()]]),
        ..Mock::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mut child = command(dir.path(), &base, &["--json"]).spawn().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["next_cursor"],
        "baseline"
    );
    let status = Command::new("kill")
        .args(["-INT", &child.id().unwrap().to_string()])
        .status()
        .await
        .unwrap();
    assert!(status.success());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .code(),
        Some(130)
    );
    server.abort();
}
