use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::{io::AsyncWriteExt, process::Command};
const ID: &str = "11111111-1111-4111-8111-111111111111";
const TOKEN: &str = "virse_sk_browser_auth_test";
#[derive(Default)]
struct Mock {
    starts: AtomicUsize,
    acks: AtomicUsize,
    revokes: AtomicUsize,
    approved: AtomicBool,
}
async fn run(dir: &Path, base: &str, args: &[&str]) -> std::process::Output {
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        Command::new(env!("CARGO_BIN_EXE_virse"))
            .args(args)
            .env("VIRSE_BACKEND_URL", base)
            .env_remove("VIRSE_API_TOKEN")
            .env("VIRSE_CONFIG_DIR", dir)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap()
}
#[tokio::test]
async fn browser_callback_ack_loss_resume_and_logout_keep_one_key() {
    let state = Arc::new(Mock::default());
    let app=Router::new().route("/auth/cli/start",post(|State(s):State<Arc<Mock>>,Json(body):Json<Value>|async move {
        s.starts.fetch_add(1,Ordering::SeqCst);
        assert_eq!(body["mode"],"loopback");assert_eq!(body["code_challenge"].as_str().unwrap().len(),43);
        let callback=url::Url::parse(body["redirect_uri"].as_str().unwrap()).unwrap();
        let state=body["state"].as_str().unwrap().to_owned();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            s.approved.store(true,Ordering::SeqCst);
            let mut socket=tokio::net::TcpStream::connect(("127.0.0.1",callback.port().unwrap())).await.unwrap();
            socket.write_all(format!("GET /callback?code={ID}&state={state}&result=approved HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
        });
        Json(json!({"request_id":ID,"device_secret":"secret","user_code":"ABCDEF-GHIJKL","authorization_url":"https://app.example.test/cli/authorize?code=ABCDEF-GHIJKL","expires_in":600,"interval":5}))
    })).route("/auth/cli/token",post(|State(s):State<Arc<Mock>>,Json(body):Json<Value>|async move {
        assert_eq!(body["device_secret"],"secret");assert!(body["code_verifier"].as_str().unwrap().len()>=43);
        Json(if s.approved.load(Ordering::SeqCst){json!({"access_token":TOKEN,"key_id":ID,"user_id":ID})}else{json!({"status":"authorization_pending","interval":5})})
    })).route("/auth/cli/ack",post(|State(s):State<Arc<Mock>>|async move {
        if s.acks.fetch_add(1,Ordering::SeqCst)==0{(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"detail":{"code":"UNAVAILABLE"}})))}else{(StatusCode::OK,Json(json!({"status":"acknowledged"})))}
    })).route("/auth/me",get(|headers:HeaderMap|async move {assert_eq!(headers["authorization"],format!("Bearer {TOKEN}"));Json(json!({"id":ID}))}))
    .route(&format!("/api-keys/{ID}"),delete(|State(s):State<Arc<Mock>>|async move{s.revokes.fetch_add(1,Ordering::SeqCst);Json(json!({"status":1}))})).with_state(state.clone());
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", socket.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let first = run(
        dir.path(),
        &base,
        &["auth", "login", "--no-browser", "--json"],
    )
    .await;
    assert_eq!(
        first.status.code(),
        Some(5),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run(
        dir.path(),
        &base,
        &["auth", "login", "--no-browser", "--json"],
    )
    .await;
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(!String::from_utf8_lossy(&second.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&second.stderr).contains(TOKEN));
    assert_eq!(state.starts.load(Ordering::SeqCst), 1);
    assert_eq!(state.acks.load(Ordering::SeqCst), 2);
    let status = run(dir.path(), &base, &["auth", "status", "--json"]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&status.stdout).unwrap()["credential_source"],
        "browser"
    );
    assert!(
        run(dir.path(), &base, &["auth", "logout"])
            .await
            .status
            .success()
    );
    assert_eq!(state.revokes.load(Ordering::SeqCst), 1);
    assert_eq!(
        run(dir.path(), &base, &["auth", "status"])
            .await
            .status
            .code(),
        Some(3)
    );
    server.abort();
}

#[tokio::test]
async fn device_login_waits_without_callback_and_rejected_logout_keeps_credentials() {
    let polls = Arc::new(AtomicUsize::new(0));
    let captured = polls.clone();
    let app=Router::new().route("/auth/cli/start",post(|Json(body):Json<Value>|async move {
        assert_eq!(body["mode"],"device");assert!(body["redirect_uri"].is_null());
        Json(json!({"request_id":ID,"device_secret":"secret","user_code":"ABCDEF-GHIJKL","authorization_url":"https://app.example.test/cli/authorize?code=ABCDEF-GHIJKL","expires_in":600,"interval":5}))
    })).route("/auth/cli/token",post(move ||{let polls=captured.clone();async move {
        Json(if polls.fetch_add(1,Ordering::SeqCst)==0{json!({"status":"authorization_pending","interval":5})}else{json!({"access_token":TOKEN,"key_id":ID,"user_id":ID})})
    }})).route("/auth/cli/ack",post(||async{Json(json!({"status":"acknowledged"}))}))
    .route("/auth/me",get(||async{Json(json!({"id":ID}))}))
    .route(&format!("/api-keys/{ID}"),delete(||async{(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"detail":"unavailable"})))}));
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", socket.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let start = std::time::Instant::now();
    let result = run(
        dir.path(),
        &base,
        &["auth", "login", "--device", "--no-browser", "--json"],
    )
    .await;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(start.elapsed() >= std::time::Duration::from_secs(5));
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    assert_eq!(
        run(dir.path(), &base, &["auth", "logout"])
            .await
            .status
            .code(),
        Some(5)
    );
    assert!(
        run(dir.path(), &base, &["auth", "status"])
            .await
            .status
            .success()
    );
    server.abort();
}

fn scope(base: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(json!(["default", format!("{base}/")]).to_string())
    )
}

fn seed_credentials(dir: &Path, base: &str) -> Value {
    let credentials = json!({scope(base): "virse_sk_old_login"});
    write_json(dir.join("credentials.json"), &credentials);
    write_json(
        dir.join(format!("cli-auth-origin-{}.json", scope(base))),
        &json!({"key_id":"old-key", "user_id":"old-account"}),
    );
    credentials
}

fn write_json(path: impl AsRef<Path>, value: &Value) {
    std::fs::write(&path, value.to_string()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn read_json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn seed_pending(dir: &Path, base: &str, expires: u64, issued: bool) {
    let pending = json!({
        "start": {"request_id":"old-request", "device_secret":"old-secret",
            "user_code":"OLD", "authorization_url":"https://app.example.test/old",
            "expires_in":600,"interval":5},
        "proof":{"request_id":"old-request","device_secret":"old-secret","code_verifier":"old-proof"},
        "state":"old-state", "redirect":null, "expires":expires,
        "key":if issued {json!({"access_token":"virse_sk_revoked", "key_id":ID,"user_id":ID})} else {Value::Null}
    });
    write_json(
        dir.join(format!("cli-auth-pending-{}.json", scope(base))),
        &pending,
    );
}

async fn replacement_server(
    stale_code: &'static str,
    reject_new: bool,
) -> (String, Arc<Mock>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Mock::default());
    let app = Router::new()
        .route(
            "/auth/cli/start",
            post(|State(s): State<Arc<Mock>>| async move {
                s.starts.fetch_add(1, Ordering::SeqCst);
                Json(
                    json!({"request_id":ID,"device_secret":"secret","user_code":"NEW",
                "authorization_url":"https://app.example.test/new","expires_in":600,"interval":5}),
                )
            }),
        )
        .route(
            "/auth/cli/token",
            post(move |Json(body): Json<Value>| async move {
                if body["request_id"] == "old-request" {
                    (
                        StatusCode::GONE,
                        Json(json!({"detail":{"code":stale_code}})),
                    )
                } else if reject_new {
                    (
                        StatusCode::FORBIDDEN,
                        Json(json!({"detail":{"code":"ACCESS_DENIED"}})),
                    )
                } else {
                    (
                        StatusCode::OK,
                        Json(json!({"access_token":TOKEN,"key_id":ID,"user_id":ID})),
                    )
                }
            }),
        )
        .route(
            "/auth/cli/ack",
            post(|State(s): State<Arc<Mock>>| async move {
                s.acks.fetch_add(1, Ordering::SeqCst);
                Json(json!({"status":"acknowledged"}))
            }),
        )
        .route(
            "/auth/me",
            get(|headers: HeaderMap| async move {
                if headers["authorization"] == "Bearer virse_sk_revoked" {
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"detail":"Invalid token"})),
                    )
                } else {
                    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                    (StatusCode::OK, Json(json!({"id":ID})))
                }
            }),
        )
        .with_state(state.clone());
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", socket.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
    (base, state, server)
}

#[tokio::test]
async fn repeated_login_replaces_saved_account_without_logout() {
    let (base, state, server) = replacement_server("", false).await;
    let dir = tempfile::tempdir().unwrap();
    seed_credentials(dir.path(), &base);
    for _ in 0..2 {
        let result = run(
            dir.path(),
            &base,
            &["auth", "login", "--device", "--no-browser"],
        )
        .await;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            read_json(dir.path().join("credentials.json"))[scope(&base)],
            TOKEN
        );
        let origin = read_json(
            dir.path()
                .join(format!("cli-auth-origin-{}.json", scope(&base))),
        );
        assert_eq!(origin["user_id"], ID);
        assert_eq!(origin["key_id"], ID);
    }
    assert_eq!(state.starts.load(Ordering::SeqCst), 2);
    assert_eq!(state.acks.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn stale_login_automatically_starts_new_authorization() {
    let future = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;
    for (expires, issued, code) in [
        (1, false, ""),
        (future, false, "AUTHORIZATION_EXPIRED"),
        (future, false, "DELIVERY_EXPIRED"),
        (future, false, "INVALID_AUTHORIZATION_PROOF"),
        (future, false, "AUTHORIZATION_ALREADY_CLAIMED"),
        (future, false, "ACCESS_DENIED"),
        (1, true, ""),
    ] {
        let (base, state, server) = replacement_server(code, false).await;
        let dir = tempfile::tempdir().unwrap();
        seed_credentials(dir.path(), &base);
        seed_pending(dir.path(), &base, expires, issued);
        let result = run(
            dir.path(),
            &base,
            &["auth", "login", "--device", "--no-browser"],
        )
        .await;
        assert!(
            result.status.success(),
            "{code}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            read_json(dir.path().join("credentials.json"))[scope(&base)],
            TOKEN
        );
        assert_eq!(state.starts.load(Ordering::SeqCst), 1);
        assert_eq!(state.acks.load(Ordering::SeqCst), 1);
        server.abort();
    }
}

#[tokio::test]
async fn rejected_replacement_keeps_current_login_and_allows_retry() {
    let (base, state, server) = replacement_server("", true).await;
    let dir = tempfile::tempdir().unwrap();
    let credentials = seed_credentials(dir.path(), &base);
    let origin_path = dir
        .path()
        .join(format!("cli-auth-origin-{}.json", scope(&base)));
    let origin = json!({"key_id":"old-key", "user_id":"old-account"});
    write_json(&origin_path, &origin);
    for _ in 0..2 {
        let result = run(
            dir.path(),
            &base,
            &["auth", "login", "--device", "--no-browser"],
        )
        .await;
        assert_eq!(result.status.code(), Some(4));
        assert_eq!(read_json(dir.path().join("credentials.json")), credentials);
        assert_eq!(read_json(&origin_path), origin);
        assert!(
            read_json(
                dir.path()
                    .join(format!("cli-auth-pending-{}.json", scope(&base)))
            )
            .is_null()
        );
    }
    assert_eq!(state.starts.load(Ordering::SeqCst), 2);
    assert_eq!(state.acks.load(Ordering::SeqCst), 0);
    server.abort();
}
