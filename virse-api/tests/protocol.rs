use axum::{
    Json, Router,
    body::Body,
    extract::{State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
use virse_api::{Client, DownloadInfo, SubmitTask};

const CANVAS: &str = "00000000-0000-0000-0000-000000000001";
const TASK: &str = "00000000-0000-0000-0000-000000000002";
const TOKEN: &str = "virse_sk_test_secret";
fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}
fn snapshot(revision: u64, status: &str) -> Value {
    json!({"task_id":TASK,"project_id":CANVAS,"operation":"image_generate","status":status,"revision":revision,"outputs":[{"element_id":CANVAS,"output_index":0,"status":"completed","asset_id":TASK,"error_code":null}],"actions":{"cancel":false,"retry":false},"result":{}})
}
async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, task)
}
fn request() -> SubmitTask {
    serde_json::from_value(json!({"operation":"image_generate","input":{"model":"test-model","user_query":"use $1","references":{"$1":{"source":{"kind":"canvas_element","element_id":CANVAS}}}},"outputs":[{"output_index":0,"create_element":{"position":{"x":12,"y":20},"size":{"width":512,"height":512}}}]})).unwrap()
}

#[tokio::test]
async fn real_http_envelopes_combined_create_and_existing_target() {
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let app = Router::new()
        .route(
            "/auth/me",
            get(|headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                Json(json!({"status":1,"message":"Success","id":"user"}))
            }),
        )
        .route(
            "/api/v1/resources/query",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["kinds"], json!(["canvas"]));
                assert_eq!(body["recursive"], true);
                Json(json!({"request_id":"req1","data":{"items":[],"next_cursor":"next"}}))
            }),
        )
        .route(
            &format!("/v2/canvas/projects/{CANVAS}/generations"),
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let captured = captured.clone();
                async move {
                    assert_eq!(headers["idempotency-key"], "same-key");
                    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                    captured.lock().unwrap().push(body);
                    (StatusCode::ACCEPTED, Json(snapshot(0, "queued")))
                }
            }),
        );
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    assert_eq!(client.me().await.unwrap(), json!({"id":"user"}));
    assert_eq!(
        client.canvases(uuid(CANVAS), 50, None).await.unwrap()["next_cursor"],
        "next"
    );
    let mut body = request();
    client
        .submit(uuid(CANVAS), &body, "same-key")
        .await
        .unwrap();
    body.outputs[0].create_element = None;
    body.outputs[0].element_id = Some(uuid(CANVAS));
    client
        .submit(uuid(CANVAS), &body, "same-key")
        .await
        .unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(
        requests[0]["outputs"][0]["create_element"]["position"]["x"],
        12.0
    );
    assert!(requests[0]["outputs"][0].get("element_id").is_none());
    assert_eq!(
        requests[0]["input"]["references"]["$1"]["source"]["element_id"],
        CANVAS
    );
    assert!(requests[1]["outputs"][0].get("create_element").is_none());
    assert_eq!(requests[1]["outputs"][0]["element_id"], CANVAS);
    server.abort();
}

#[tokio::test]
async fn rejects_legacy_business_failure_and_redacts_errors() {
    let (base, server) = serve(Router::new().route(
        "/auth/me",
        get(|| async { Json(json!({"status":0,"message":format!("denied {TOKEN}")})) }),
    ))
    .await;
    let err = Client::new(&base, TOKEN).unwrap().me().await.unwrap_err();
    assert!(!format!("{err:?}").contains(TOKEN));
    assert_eq!(err.status, Some(200));
    server.abort();
}

#[tokio::test]
async fn distinguishes_http_failures_and_does_not_follow_api_redirects() {
    for status in [401u16, 403, 409, 429, 503, 302] {
        let (base, server) = serve(Router::new().route(
            "/auth/me",
            get(move || async move {
                (
                    StatusCode::from_u16(status).unwrap(),
                    Json(json!({"detail":"REJECTED"})),
                )
            }),
        ))
        .await;
        let err = Client::new(&base, TOKEN).unwrap().me().await.unwrap_err();
        assert_eq!(err.status, Some(status));
        assert_eq!(err.code, "REJECTED");
        assert_eq!(err.retryable, status == 429 || status >= 500);
        server.abort();
    }
}

#[derive(Clone)]
struct WsState {
    connections: Arc<AtomicUsize>,
    reads: Arc<AtomicUsize>,
    reconnect: bool,
}
async fn websocket(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<WsState>,
) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
    let connection = state.connections.fetch_add(1, Ordering::SeqCst);
    ws.on_upgrade(move |mut socket| async move {
        let Some(Ok(Message::Text(text))) = socket.recv().await else { panic!("missing subscribe") };
        let subscribe: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(subscribe,json!({"type":"generation_subscribe","data":{"task_ids":[TASK]}}));
        if state.reconnect && connection==0 {
            socket.send(Message::Text(json!({"type":"generation_updated","project_id":CANVAS,"task":snapshot(2,"running")}).to_string().into())).await.unwrap();
            socket.send(Message::Close(None)).await.unwrap(); return;
        }
        for (revision,status) in [(2,"running"),(1,"running"),(2,"running"),(3,"succeeded")] {
            socket.send(Message::Text(json!({"type":"generation_snapshot","project_id":CANVAS,"task":snapshot(revision,status)}).to_string().into())).await.unwrap();
        }
    })
}

#[tokio::test]
async fn websocket_reconciles_reconnect_and_ignores_stale_revisions() {
    for reconnect in [false, true] {
        let state = WsState {
            connections: Arc::new(AtomicUsize::new(0)),
            reads: Arc::new(AtomicUsize::new(0)),
            reconnect,
        };
        let app = Router::new()
            .route(&format!("/canvas/ws/{CANVAS}"), get(websocket))
            .route(
                &format!("/v2/canvas/projects/{CANVAS}/generations/{TASK}"),
                get(|State(s): State<WsState>| async move {
                    let read = s.reads.fetch_add(1, Ordering::SeqCst);
                    if s.reconnect && read > 0 {
                        Json(snapshot(1, "succeeded"))
                    } else {
                        Json(snapshot(0, "queued"))
                    }
                }),
            )
            .with_state(state.clone());
        let (base, server) = serve(app).await;
        let client = Client::new(&base, TOKEN).unwrap();
        let observed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.watch(uuid(CANVAS), uuid(TASK)).collect::<Vec<_>>(),
        )
        .await
        .unwrap();
        let tasks: Vec<_> = observed.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            tasks.iter().map(|t| t.revision).collect::<Vec<_>>(),
            vec![0, 2, 3]
        );
        assert_eq!(
            state.connections.load(Ordering::SeqCst),
            if reconnect { 2 } else { 1 }
        );
        assert_eq!(
            state.reads.load(Ordering::SeqCst),
            if reconnect { 2 } else { 1 }
        );
        server.abort();
    }
}

#[tokio::test]
async fn already_completed_and_needs_attention_do_not_open_websocket() {
    for status in [
        "succeeded",
        "needs_attention",
        "failed",
        "partially_succeeded",
        "cancelled",
    ] {
        let (base, server) = serve(Router::new().route(
            &format!("/v2/canvas/projects/{CANVAS}/generations/{TASK}"),
            get(move || async move { Json(snapshot(5, status)) }),
        ))
        .await;
        let client = Client::new(&base, TOKEN).unwrap();
        let tasks = client
            .watch(uuid(CANVAS), uuid(TASK))
            .collect::<Vec<_>>()
            .await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].as_ref().unwrap().status, status);
        server.abort();
    }
}

#[tokio::test]
async fn unsupported_cancel_keeps_server_error() {
    let (base, server) = serve(Router::new().route(
        &format!("/v2/canvas/projects/{CANVAS}/generations/{TASK}/cancel"),
        post(|| async {
            (
                StatusCode::CONFLICT,
                Json(json!({"detail":"GENERATION_CANCEL_UNSUPPORTED"})),
            )
        }),
    ))
    .await;
    let e = Client::new(&base, TOKEN)
        .unwrap()
        .cancel(uuid(CANVAS), uuid(TASK))
        .await
        .unwrap_err();
    assert_eq!(e.code, "GENERATION_CANCEL_UNSUPPORTED");
    server.abort();
}

#[tokio::test]
async fn download_redirect_strips_auth_and_external_delivery_never_gets_token() {
    let (external, external_server) =
        serve(Router::new().fallback(|request: Request<Body>| async move {
            assert!(request.headers().get("authorization").is_none());
            "image bytes"
        }))
        .await;
    let location = external.clone();
    let (base, server) = serve(Router::new().route(
        "/file",
        get(move |headers: HeaderMap| {
            let location = location.clone();
            async move {
                assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                (StatusCode::FOUND, [("location", location)], "").into_response()
            }
        }),
    ))
    .await;
    let client = Client::new(&base, TOKEN).unwrap();
    let mut info = DownloadInfo {
        url: format!("{base}/file"),
        filename: "a.png".into(),
        delivery: "authenticated_stream".into(),
        extra: Default::default(),
    };
    assert_eq!(
        client.download(&info).await.unwrap().bytes().await.unwrap(),
        "image bytes"
    );
    info.url = external;
    assert!(client.download(&info).await.is_err());
    info.delivery = "signed_url".into();
    assert_eq!(
        client.download(&info).await.unwrap().bytes().await.unwrap(),
        "image bytes"
    );
    server.abort();
    external_server.abort();
}

#[test]
fn invalid_outputs_and_backend_origins_fail_locally() {
    for url in [
        "",
        "https://user:password@api.virse.ai",
        "https://api.virse.ai/path",
        "https://api.virse.ai?token=secret",
        "ftp://api.virse.ai",
    ] {
        assert!(Client::new(url, TOKEN).is_err());
    }
    let mut body = request();
    body.outputs[0].element_id = Some(uuid(CANVAS));
    assert!(body.validate().is_err());
    body.outputs[0].element_id = None;
    body.outputs[0].create_element.as_mut().unwrap().size.width = -1.0;
    assert!(body.validate().is_err());
}

#[tokio::test]
async fn canvas_read_uses_explicit_none_instead_of_empty_expansion() {
    let app = Router::new().route(
        &format!("/canvas/projects/{CANVAS}"),
        get(|axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
            // Mirrors backend ReadOptions: an empty option is rejected; `none` is explicit.
            if query.get("include").map(String::as_str) != Some("elements")
                || query.get("expand").map(String::as_str) != Some("none") {
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"code":"invalid_read_options"})));
            }
            (StatusCode::OK, Json(json!({"status":1,"message":"Success","project":{"id":CANVAS},"elements":[]})))
        }),
    );
    let (base, server) = serve(app).await;
    let response = Client::new(&base, TOKEN)
        .unwrap()
        .canvas(uuid(CANVAS))
        .await
        .unwrap();
    assert_eq!(response["project"]["id"], CANVAS);
    assert_eq!(response["elements"], json!([]));
    server.abort();
}
