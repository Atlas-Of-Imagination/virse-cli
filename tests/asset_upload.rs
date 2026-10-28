//! Process-level recovery tests: each invocation must recover from disk, not
//! from memory in the CLI process. The mock commits before losing its response.
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as ApiPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::{Arc, Mutex},
};
use tokio::process::Command;

const ORG: &str = "00000000-0000-0000-0000-000000000001";
const FOLDER: &str = "00000000-0000-0000-0000-000000000002";
const RESOURCE: &str = "00000000-0000-0000-0000-000000000003";
const ASSET: &str = "00000000-0000-0000-0000-000000000004";
const USER: &str = "00000000-0000-0000-0000-000000000005";
const ORIGINAL: &[u8] = b"original file bytes\x00\xff\r\n";

#[derive(Clone, Debug, PartialEq)]
struct Write {
    key: String,
    filename: String,
    bytes: Vec<u8>,
    options: Value,
    replace: bool,
}

struct ServerState {
    queries: usize,
    reads: usize,
    writes: Vec<Write>,
    completed: HashMap<String, Value>,
    lost_responses: usize,
    error: Option<&'static str>,
    renamed: bool,
    source_to_change: Option<PathBuf>,
    revision: u64,
    model: bool,
    state_revision: Option<u64>,
    shortcut: bool,
    skipped: bool,
}
impl Default for ServerState {
    fn default() -> Self {
        Self {
            queries: 0,
            reads: 0,
            writes: vec![],
            completed: HashMap::new(),
            lost_responses: 0,
            error: None,
            renamed: false,
            source_to_change: None,
            revision: 1,
            model: false,
            state_revision: Some(0),
            shortcut: false,
            skipped: false,
        }
    }
}
type Shared = Arc<Mutex<ServerState>>;

fn asset(state: &ServerState) -> Value {
    let mut attributes = json!({});
    if let Some(revision) = state.state_revision {
        attributes["main_view_revision"] = json!(revision);
    }
    json!({"id":ASSET,"type":if state.model {"model3d"} else {"image"},"status":"ready",
        "content":{"revision":state.revision,"mime_type":"image/png","size_bytes":ORIGINAL.len(),"checksum":null,"metadata_state":"ready"},
        "capabilities":{"read":true,"replace_content":true},"attributes":attributes})
}
fn folder(state: &ServerState) -> Value {
    json!({"id":FOLDER,"parent_id":null,"name":if state.renamed {"renamed"} else {"folder"},"kind":"folder"})
}
fn resource(state: &ServerState) -> Value {
    json!({"id":RESOURCE,"parent_id":FOLDER,"name":if state.renamed {"renamed.png"} else {"photo.png"},
        "kind":if state.shortcut {"shortcut"} else {"asset_image"},
        "asset_id":if state.shortcut {None} else {Some(ASSET)}})
}
async fn query(State(shared): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let mut state = shared.lock().unwrap();
    state.queries += 1;
    assert_eq!(body["context"]["org_id"], ORG);
    let items = if body["parent_id"] == FOLDER {
        vec![resource(&state)]
    } else {
        vec![folder(&state)]
    };
    Json(
        json!({"request_id":"query","data":{"items":items,"next_cursor":null,"snapshot":"s","truncated":false}}),
    )
}
async fn detail(ApiPath(id): ApiPath<String>, State(shared): State<Shared>) -> Json<Value> {
    let mut state = shared.lock().unwrap();
    state.reads += 1;
    let (resource, asset) = if id == FOLDER {
        (folder(&state), Value::Null)
    } else {
        assert_eq!(id, RESOURCE);
        (resource(&state), asset(&state))
    };
    Json(
        json!({"request_id":"detail","data":{"resource":resource,"asset":asset,"kind":"asset_image","detail":{}}}),
    )
}

fn position(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|s| s == needle)
        .expect("multipart delimiter")
}
fn multipart(headers: &HeaderMap, body: &[u8], replace: bool) -> Write {
    assert_eq!(headers["authorization"], "Bearer virse_sk_upload_mock");
    assert_eq!(headers["x-virse-expected-user"], USER);
    // The public endpoint defaults an omitted source header to api.
    assert_eq!(
        headers
            .get("x-virse-write-source")
            .map(|h| h.to_str().unwrap())
            .unwrap_or("api"),
        "api"
    );
    let content_type = headers["content-type"].to_str().unwrap();
    let boundary = content_type
        .split("boundary=")
        .nth(1)
        .unwrap()
        .trim_matches('"');
    let delimiter = format!("\r\n--{boundary}");
    let mut remaining = &body[format!("--{boundary}\r\n").len()..];
    let mut filename = None;
    let mut bytes = None;
    let mut options = None;
    loop {
        let header_end = position(remaining, b"\r\n\r\n");
        let part_headers = std::str::from_utf8(&remaining[..header_end]).unwrap();
        remaining = &remaining[header_end + 4..];
        let end = position(remaining, delimiter.as_bytes());
        let data = &remaining[..end];
        if part_headers.contains("name=\"file\"") {
            filename = Some(
                part_headers
                    .split("filename=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
                    .to_owned(),
            );
            bytes = Some(data.to_vec());
        } else if part_headers.contains("name=\"options\"") {
            options = Some(serde_json::from_slice(data).unwrap());
        } else {
            panic!("Unexpected multipart field");
        }
        remaining = &remaining[end + delimiter.len()..];
        if remaining.starts_with(b"--") {
            break;
        }
        assert!(remaining.starts_with(b"\r\n"));
        remaining = &remaining[2..];
    }
    Write {
        key: headers["idempotency-key"].to_str().unwrap().into(),
        filename: filename.unwrap(),
        bytes: bytes.unwrap(),
        options: options.unwrap(),
        replace,
    }
}
fn failure(code: &str) -> Response {
    (StatusCode::CONFLICT, Json(json!({"request_id":"rejected","error":{"code":code,"message":"mock conflict","retryable":true,
        "details":{"current_revision":9,"completed_revision":2}}}))).into_response()
}
fn write(shared: Shared, headers: HeaderMap, body: Bytes, replace: bool) -> Response {
    let request = multipart(&headers, &body, replace);
    let mut state = shared.lock().unwrap();
    state.writes.push(request.clone());
    if let Some(code) = state.error {
        return failure(code);
    }
    let response = if let Some(value) = state.completed.get(&request.key) {
        // Replay after a later edit must not pretend the original content is current.
        let completed_revision = if replace {
            &value["content"]["revision"]
        } else {
            &value["asset"]["content"]["revision"]
        };
        if completed_revision.as_u64() != Some(state.revision) {
            return failure("content_changed");
        }
        let first = state.writes.iter().find(|w| w.key == request.key).unwrap();
        assert_eq!(
            first, &request,
            "same-key replay changed bytes, filename, target, options or method"
        );
        value.clone()
    } else {
        if replace {
            assert_eq!(
                request.options["context"],
                json!({"org_id":ORG,"dentry_id":RESOURCE})
            );
            assert_eq!(request.options["expected_revision"], state.revision);
            state.revision += 1;
        } else {
            assert_eq!(
                request.options["destination"],
                json!({"org_id":ORG,"kind":"drive","parent_id":FOLDER})
            );
            assert_eq!(request.options["retention"], "persistent");
        }
        state.renamed = true;
        if let Some(path) = state.source_to_change.take() {
            std::fs::write(path, b"edited while request is pending").unwrap();
        }
        let response = if replace {
            asset(&state)
        } else {
            json!({"asset":asset(&state),"resource":resource(&state),"skipped":state.skipped})
        };
        state
            .completed
            .insert(request.key.clone(), response.clone());
        response
    };
    if state.lost_responses > 0 {
        state.lost_responses -= 1;
        return (StatusCode::OK, "response lost after durable commit").into_response();
    }
    (
        if replace {
            StatusCode::OK
        } else {
            StatusCode::CREATED
        },
        Json(json!({"request_id":"write","data":response})),
    )
        .into_response()
}
async fn upload(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    write(state, headers, body, false)
}
async fn replace(
    ApiPath(id): ApiPath<String>,
    State(state): State<Shared>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    assert_eq!(id, ASSET);
    write(state, headers, body, true)
}
async fn serve(state: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route(
            "/auth/me",
            get(|| async { Json(json!({"status":1,"id":USER})) }),
        )
        .route("/api/v1/resources/query", post(query))
        .route("/api/v1/resources/{id}", get(detail))
        .route("/api/v1/assets", post(upload))
        .route("/api/v1/assets/{id}/content", put(replace))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, server)
}
async fn run(root: &Path, base: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_virse"));
    cmd.args(["--json", "--org", ORG, "resource"])
        .args(args)
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", "virse_sk_upload_mock")
        .env("VIRSE_CONFIG_DIR", root.join("state"))
        .stdin(Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .unwrap()
        .unwrap()
}
fn success(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn error(out: &Output, code: &str) -> Value {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "failure must not print success JSON");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let value: Value = stderr
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
        .unwrap_or_else(|| panic!("missing JSON error: {stderr}"));
    assert_eq!(value["error"]["code"], code, "{stderr}");
    value["error"].clone()
}
fn source(root: &Path) -> PathBuf {
    let file = root.join("original.png");
    std::fs::write(&file, ORIGINAL).unwrap();
    file
}
fn journal_file(root: &Path, extension: &str) -> PathBuf {
    std::fs::read_dir(root.join("state/asset-writes"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == extension))
        .unwrap()
}

#[tokio::test]
async fn response_loss_retries_frozen_bytes_even_when_path_and_local_file_change() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let state = Shared::default();
    {
        let mut s = state.lock().unwrap();
        s.lost_responses = 1;
        s.source_to_change = Some(source.clone());
    }
    let (base, server) = serve(state.clone()).await;
    let args = [
        "upload",
        source.to_str().unwrap(),
        "/folder",
        "--idempotency-key",
        "lost-once",
    ];
    let result = success(&run(dir.path(), &base, &args).await);
    assert_eq!(result["revision"], 1);
    assert_eq!(result["idempotency_key"], "lost-once");
    {
        let s = state.lock().unwrap();
        assert_eq!(s.queries, 1);
        assert_eq!(s.writes.len(), 2);
        assert_eq!(s.completed.len(), 1);
        assert_eq!(s.writes[0].bytes, ORIGINAL);
        assert_eq!(s.writes[0].filename, "original.png");
    }
    error(&run(dir.path(), &base, &args).await, "ASSET_SOURCE_CHANGED");
    std::fs::remove_file(&source).unwrap();
    let mut resume = args.to_vec();
    resume.push("--resume");
    success(&run(dir.path(), &base, &resume).await);
    assert_eq!(state.lock().unwrap().queries, 1);
    assert_eq!(state.lock().unwrap().writes.len(), 3);
    server.abort();
}

#[tokio::test]
async fn unknown_outcome_survives_process_exit_and_deleted_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let state = Shared::default();
    state.lock().unwrap().lost_responses = 3;
    let (base, server) = serve(state.clone()).await;
    let args = [
        "upload",
        source.to_str().unwrap(),
        "/folder",
        "--idempotency-key",
        "durable",
    ];
    let out = run(dir.path(), &base, &args).await;
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(
        error(&out, "ASSET_WRITE_UNCERTAIN")["idempotency_key"],
        "durable"
    );
    let receipt_path = journal_file(dir.path(), "json");
    let receipt: Value = serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(receipt["identity"]["org"], ORG);
    assert_eq!(receipt["identity"]["user"], USER);
    assert_eq!(receipt["filename"], "original.png");
    assert_eq!(
        std::fs::read(journal_file(dir.path(), "bytes")).unwrap(),
        ORIGINAL
    );
    assert!(receipt.get("binding").is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            receipt_path,
            journal_file(dir.path(), "bytes"),
            journal_file(dir.path(), "lock"),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(
            std::fs::metadata(dir.path().join("state/asset-writes"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let changed = [
        "upload",
        source.to_str().unwrap(),
        "/renamed",
        "--idempotency-key",
        "durable",
    ];
    error(
        &run(dir.path(), &base, &changed).await,
        "ASSET_INTENT_CHANGED",
    );
    let mut changed = args.to_vec();
    changed.extend(["--conflict", "skip"]);
    error(
        &run(dir.path(), &base, &changed).await,
        "ASSET_INTENT_CHANGED",
    );
    std::fs::remove_file(&source).unwrap();
    let mut resume = args.to_vec();
    resume.push("--resume");
    success(&run(dir.path(), &base, &resume).await);
    assert_eq!(state.lock().unwrap().queries, 1);
    assert_eq!(state.lock().unwrap().writes.len(), 4);
    server.abort();
}

#[tokio::test]
async fn replacement_requires_user_revision_and_resolves_shortcut_asset() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let state = Shared::default();
    state.lock().unwrap().shortcut = true;
    let (base, server) = serve(state.clone()).await;
    let missing = run(
        dir.path(),
        &base,
        &["replace", "/folder/photo.png", source.to_str().unwrap()],
    )
    .await;
    assert_eq!(missing.status.code(), Some(2));
    error(&missing, "INVALID_INPUT");
    assert_eq!(state.lock().unwrap().queries, 0);
    let args = [
        "replace",
        "/folder/photo.png",
        source.to_str().unwrap(),
        "--expected-revision",
        "2",
        "--idempotency-key",
        "stale",
    ];
    error(&run(dir.path(), &base, &args).await, "revision_conflict");
    assert!(state.lock().unwrap().writes.is_empty());
    assert!(
        !dir.path()
            .join("state/asset-writes")
            .read_dir()
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "json"))
    );
    let args = [
        "replace",
        "/folder/photo.png",
        source.to_str().unwrap(),
        "--expected-revision",
        "1",
        "--idempotency-key",
        "replace-shortcut",
    ];
    let value = success(&run(dir.path(), &base, &args).await);
    assert_eq!(value["revision"], 2);
    assert_eq!(value["asset"]["id"], ASSET);
    assert_eq!(
        state.lock().unwrap().writes[0].options["context"]["dentry_id"],
        RESOURCE
    );
    server.abort();
}

#[tokio::test]
async fn replacement_recovery_never_rereads_paths_or_guesses_a_new_revision() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let state = Shared::default();
    state.lock().unwrap().lost_responses = 3;
    let (base, server) = serve(state.clone()).await;
    let args = [
        "replace",
        "/folder/photo.png",
        source.to_str().unwrap(),
        "--expected-revision",
        "1",
        "--idempotency-key",
        "replace-lost",
    ];
    let out = run(dir.path(), &base, &args).await;
    assert_eq!(out.status.code(), Some(7));
    error(&out, "ASSET_WRITE_UNCERTAIN");
    let counts = {
        let s = state.lock().unwrap();
        (s.queries, s.reads)
    };
    let value = success(&run(dir.path(), &base, &args).await);
    assert_eq!(value["revision"], 2);
    assert_eq!(
        {
            let s = state.lock().unwrap();
            (s.queries, s.reads)
        },
        counts
    );
    state.lock().unwrap().revision = 9;
    let out = run(dir.path(), &base, &args).await;
    let err = error(&out, "content_changed");
    assert_eq!(err["details"]["completed_revision"], 2);
    assert_ne!(out.status.code(), Some(7));
    assert_eq!(
        state.lock().unwrap().writes.len(),
        5,
        "known conflict must not retry even if retryable=true"
    );
    assert_eq!(
        {
            let s = state.lock().unwrap();
            (s.queries, s.reads)
        },
        counts
    );
    server.abort();
}

#[tokio::test]
async fn model_reset_requires_explicit_matching_state_revision_without_defaulting_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let state = Shared::default();
    state.lock().unwrap().model = true;
    let (base, server) = serve(state.clone()).await;
    let args = [
        "replace",
        "/folder/photo.png",
        source.to_str().unwrap(),
        "--expected-revision",
        "1",
        "--reset-model-state",
    ];
    error(&run(dir.path(), &base, &args).await, "INVALID_INPUT");
    let mut args = args.to_vec();
    args.extend(["--expected-state-revision", "2"]);
    error(
        &run(dir.path(), &base, &args).await,
        "state_revision_conflict",
    );
    assert!(state.lock().unwrap().writes.is_empty());
    *args.last_mut().unwrap() = "0";
    state.lock().unwrap().state_revision = None;
    error(&run(dir.path(), &base, &args).await, "INVALID_RESPONSE");
    state.lock().unwrap().state_revision = Some(0);
    success(&run(dir.path(), &base, &args).await);
    let s = state.lock().unwrap();
    assert_eq!(s.writes.len(), 1);
    assert_eq!(s.writes[0].options["reset_model_state"], true);
    assert_eq!(s.writes[0].options["expected_state_revision"], 0);
    server.abort();
}

#[tokio::test]
async fn skip_of_processing_asset_preserves_zero_revision_without_binding() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let state = Shared::default();
    {
        let mut s = state.lock().unwrap();
        s.skipped = true;
        s.revision = 0;
    }
    let (base, server) = serve(state.clone()).await;
    let out = run(
        dir.path(),
        &base,
        &[
            "upload",
            source.to_str().unwrap(),
            "/folder",
            "--conflict",
            "skip",
            "--idempotency-key",
            "skip",
        ],
    )
    .await;
    let value = success(&out);
    assert_eq!(value["skipped"], true);
    assert_eq!(value["revision"], 0);
    assert!(value.get("binding").is_none());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not bound"));
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(journal_file(dir.path(), "json")).unwrap()).unwrap();
    assert!(receipt.get("binding").is_none());
    server.abort();
}

#[tokio::test]
async fn definitive_conflicts_do_not_retry_but_in_progress_keeps_recovery_key() {
    for code in [
        "idempotency_conflict",
        "revision_conflict",
        "idempotency_in_progress",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let source = source(dir.path());
        let state = Shared::default();
        state.lock().unwrap().error = Some(code);
        let (base, server) = serve(state.clone()).await;
        let out = run(
            dir.path(),
            &base,
            &[
                "upload",
                source.to_str().unwrap(),
                "/folder",
                "--conflict",
                "error",
                "--idempotency-key",
                "conflict",
            ],
        )
        .await;
        let pending = code == "idempotency_in_progress";
        let value = error(
            &out,
            if pending {
                "ASSET_WRITE_UNCERTAIN"
            } else {
                code
            },
        );
        assert_eq!(value["idempotency_key"], "conflict");
        assert_eq!(
            state.lock().unwrap().writes.len(),
            if pending { 3 } else { 1 }
        );
        assert_eq!(out.status.code() == Some(7), pending);
        server.abort();
    }
}

#[tokio::test]
async fn corrupt_staged_bytes_and_options_are_rejected_before_replay() {
    for corrupt_options in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = source(dir.path());
        let state = Shared::default();
        let (base, server) = serve(state.clone()).await;
        let args = [
            "upload",
            source.to_str().unwrap(),
            "/folder",
            "--idempotency-key",
            "corrupt",
        ];
        success(&run(dir.path(), &base, &args).await);
        if corrupt_options {
            let path = journal_file(dir.path(), "json");
            let mut receipt: Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            receipt["options"]["destination"]["org_id"] = json!(USER);
            std::fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        } else {
            std::fs::write(journal_file(dir.path(), "bytes"), b"tampered").unwrap();
        }
        let mut resume = args.to_vec();
        resume.push("--resume");
        error(
            &run(dir.path(), &base, &resume).await,
            "ASSET_RECEIPT_ERROR",
        );
        assert_eq!(state.lock().unwrap().writes.len(), 1);
        server.abort();
    }
}

#[tokio::test]
async fn source_validation_rejects_empty_directory_oversize_and_symlinks_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let empty = dir.path().join("empty.png");
    std::fs::write(&empty, b"").unwrap();
    let huge = dir.path().join("huge.png");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(100 * 1024 * 1024 + 1)
        .unwrap();
    let folder = dir.path().join("directory.png");
    std::fs::create_dir(&folder).unwrap();
    let mut paths = vec![empty, huge, folder];
    #[cfg(unix)]
    {
        let file = source(dir.path());
        let link = dir.path().join("symlink.png");
        std::os::unix::fs::symlink(file, &link).unwrap();
        paths.push(link);
        let fifo = dir.path().join("fifo.png");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        paths.push(fifo);
    }
    for path in paths {
        let out = run(
            dir.path(),
            &base,
            &["upload", path.to_str().unwrap(), "/folder"],
        )
        .await;
        assert!(!out.status.success(), "rejected {}", path.display());
        assert!(out.stdout.is_empty());
    }
    assert!(state.lock().unwrap().writes.is_empty());
    assert_eq!(state.lock().unwrap().queries, 0);
    assert!(
        !dir.path()
            .join("state/asset-writes")
            .read_dir()
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "json"))
    );
    server.abort();
}

#[tokio::test]
async fn same_key_in_another_backend_has_independent_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let first = Shared::default();
    let second = Shared::default();
    let (base1, server1) = serve(first.clone()).await;
    let (base2, server2) = serve(second.clone()).await;
    let args = [
        "upload",
        source.to_str().unwrap(),
        "/folder",
        "--idempotency-key",
        "same-key",
    ];
    success(&run(dir.path(), &base1, &args).await);
    success(&run(dir.path(), &base2, &args).await);
    assert_eq!(first.lock().unwrap().writes.len(), 1);
    assert_eq!(second.lock().unwrap().writes.len(), 1);
    assert_eq!(
        dir.path()
            .join("state/asset-writes")
            .read_dir()
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "json"))
            .count(),
        2
    );
    server1.abort();
    server2.abort();
}
