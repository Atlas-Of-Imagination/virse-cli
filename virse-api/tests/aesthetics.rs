use axum::{
    Json, Router,
    body::Bytes,
    extract::{OriginalUri, State, WebSocketUpgrade},
    http::{HeaderMap, Method},
    routing::{any, get},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use virse_api::{AestheticOperation as Op, Client};
type Records = Arc<Mutex<Vec<(Method, String, Value)>>>;
const ID: &str = "11111111-1111-4111-8111-111111111111";
#[tokio::test]
async fn existing_operations_use_common_routes_without_conversation_state() {
    let records: Records = Default::default();
    let app = Router::new()
        .fallback(any(
            |State(r): State<Records>,
             uri: OriginalUri,
             method: Method,
             headers: HeaderMap,
             bytes: Bytes| async move {
                assert_eq!(headers["authorization"], "Bearer virse_sk_test");
                r.lock().unwrap().push((
                    method,
                    uri.path().into(),
                    if bytes.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes).unwrap()
                    },
                ));
                Json(json!({"ok":true}))
            },
        ))
        .with_state(records.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = Client::new(&url, "virse_sk_test").unwrap();
    let id = Uuid::parse_str(ID).unwrap();
    client
        .aesthetic(Op::Context, Some(id), Value::Null)
        .await
        .unwrap();
    client
        .aesthetic(
            Op::Revise,
            Some(id),
            json!({"expected_revision":0,"summary":"matte"}),
        )
        .await
        .unwrap();
    let r = records.lock().unwrap();
    assert_eq!(r[0].0, Method::GET);
    assert_eq!(
        r[0].1,
        format!("/v2/aesthetics/tuning/sessions/{ID}/context")
    );
    assert_eq!(r[1].2, json!({"expected_revision":0,"summary":"matte"}));
    assert_eq!(r.len(), 2);
    task.abort();
}
#[tokio::test]
async fn training_watch_reads_once_after_socket_ready() {
    let run = json!({"run_id":ID,"kind":"aesthetic_training","owner_id":ID,"element_id":null,"session_id":null,"agent_turn_id":null,"canvas_id":ID,"status":"succeeded","revision":2,"result":{},"actions":{"cancel":false,"interrupt":false,"resume":false}});
    let app = Router::new()
        .route(
            "/canvas/ws/{id}",
            get(|ws: WebSocketUpgrade| async {
                ws.on_upgrade(|mut socket| async move {
                    socket
                        .send(axum::extract::ws::Message::Text(
                            json!({"type":"active_users"}).to_string().into(),
                        ))
                        .await
                        .unwrap();
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                })
            }),
        )
        .route(
            "/v2/aesthetics/canvases/{id}/training-runs/{run}",
            get(move || {
                let run = run.clone();
                async move { Json(run) }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = Client::new(&url, "virse_sk_test").unwrap();
    let id = Uuid::parse_str(ID).unwrap();
    let stream = client.watch_aesthetic_training(id, id, id);
    futures_util::pin_mut!(stream);
    assert_eq!(stream.next().await.unwrap().unwrap().status, "succeeded");
    assert!(stream.next().await.is_none());
    task.abort();
}
