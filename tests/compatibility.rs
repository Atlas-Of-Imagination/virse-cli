use axum::{Json, Router, http::HeaderMap, routing::get};
use serde_json::json;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn websocket_upgrade_preserves_policy_and_stops_observer() {
    use axum::extract::ws::{Message, WebSocketUpgrade};
    let app=Router::new()
        .route("/client-compatibility",get(|| async {Json(json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test"}))}))
        .route("/canvas/ws/{id}",get(|headers:HeaderMap,ws:WebSocketUpgrade| async move {
            assert_eq!(headers["x-virse-client"],"cli");
            assert_eq!(headers["x-virse-contract-version"],"1");
            ws.on_upgrade(|mut socket| async move {
                socket.send(Message::Text(json!({"type":"client_upgrade_required","code":"CLIENT_UPGRADE_REQUIRED","minimum_contract_version":2}).to_string().into())).await.unwrap();
            })
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = virse_api::Client::new(&format!("http://{address}"), "virse_sk_test")
        .unwrap()
        .with_cli_contract(1);
    let mut socket = client.composer_socket(uuid::Uuid::new_v4()).await.unwrap();
    let error = socket.next().await.unwrap_err();
    assert_eq!(error.code, "CLIENT_UPGRADE_REQUIRED");
    assert!(!error.retryable);
    assert_eq!(error.details.unwrap()["minimum_contract_version"], 2);
    server.abort();
}

#[tokio::test]
async fn cached_update_is_advisory_and_stdout_remains_json() {
    let app = Router::new()
        .route("/client-compatibility", get(|| async { Json(json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test"})) }))
        .fallback(|| async { Json(json!({"organizations":[]})) });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cli-update.json");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        &cache,
        json!({"checked_at":now,"latest":"v999.0.0"}).to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let out = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_virse"))
            .args(["space", "list", "--json"])
            .env("VIRSE_API_TOKEN", "virse_sk_test")
            .env("VIRSE_BACKEND_URL", format!("http://{address}"))
            .env("VIRSE_CONFIG_DIR", dir.path())
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
    assert!(String::from_utf8_lossy(&out.stderr).contains("https://virse.ai/install."));
    server.abort();
}

#[tokio::test]
async fn preflight_blocks_before_business_request_and_reports_update() {
    for (policy, exit) in [
        (
            json!({"minimum_contract_version":2,"require_web_version":false,"policy_id":"test"}),
            9,
        ),
        (json!({}), 5),
    ] {
        let writes = Arc::new(AtomicUsize::new(0));
        let count = writes.clone();
        let app = Router::new()
            .route(
                "/client-compatibility",
                get(move || {
                    let policy = policy.clone();
                    async move { Json(policy) }
                }),
            )
            .fallback(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Json(json!({}))
                }
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let out = tokio::task::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_virse"))
                .args(["space", "list", "--json"])
                .env("VIRSE_API_TOKEN", "virse_sk_test")
                .env("VIRSE_BACKEND_URL", format!("http://{address}"))
                .env("VIRSE_CONFIG_DIR", dir.path())
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(out.status.code(), Some(exit));
        assert!(out.stdout.is_empty());
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
        if exit == 9 {
            assert!(
                error["error"]["details"]["update_command"]
                    .as_str()
                    .unwrap()
                    .contains("https://virse.ai/install.")
            );
        }
        task.abort();
    }
}

#[tokio::test]
async fn sdk_identity_is_opt_in_and_top_level_upgrade_is_terminal() {
    let app = Router::new().route("/auth/me",get(|headers: HeaderMap| async move {
        if headers.get("x-virse-client").is_some() {
            assert_eq!(headers["x-virse-contract-version"],"1");
            (axum::http::StatusCode::CONFLICT,Json(json!({"code":"CLIENT_UPGRADE_REQUIRED","message":"upgrade","minimum_contract_version":2,"retryable":true})))
        } else {(axum::http::StatusCode::OK,Json(json!({"id":"test"})))}
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = virse_api::Client::new(&format!("http://{address}"), "virse_sk_test").unwrap();
    assert!(client.me().await.is_ok());
    let error = client.with_cli_contract(1).me().await.unwrap_err();
    assert_eq!(error.code, "CLIENT_UPGRADE_REQUIRED");
    assert!(!error.retryable);
    assert_eq!(error.details.unwrap()["minimum_contract_version"], 2);
    task.abort();
}
