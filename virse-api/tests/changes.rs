use axum::{
    Json, Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{CloseFrame, Message},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;
use virse_api::{ChangeKind, ChangesWatchEvent, Client, Result};

const ORG: &str = "00000000-0000-0000-0000-000000000001";
const TOKEN: &str = "virse_sk_changes_test";

#[derive(Default)]
struct Mock {
    pages: VecDeque<(u16, Value)>,
    scripts: VecDeque<Vec<Message>>,
    cursors: Vec<Option<String>>,
    sockets: usize,
    handshake_status: Option<u16>,
}
type Shared = Arc<Mutex<Mock>>;

fn page(cursor: &str, bootstrap: bool, more: bool, items: Value) -> (u16, Value) {
    (
        200,
        json!({"request_id":"changes-test","data":{"items":items,"next_cursor":cursor,"has_more":more,"bootstrap":bootstrap}}),
    )
}
fn event(kind: &str) -> Value {
    json!({"event_id":Uuid::new_v4(),"kind":kind,"occurred_at":"2026-10-28T00:00:00Z"})
}
fn ready() -> Message {
    Message::Text(
        json!({"type":"asset_changes_ready","org_id":ORG})
            .to_string()
            .into(),
    )
}
fn available() -> Message {
    Message::Text(
        json!({"type":"asset_changes_available","org_id":ORG})
            .to_string()
            .into(),
    )
}
fn close(code: u16) -> Message {
    Message::Close(Some(CloseFrame {
        code,
        reason: "test".into(),
    }))
}
fn error(status: u16, code: &str, retryable: bool) -> (u16, Value) {
    (
        status,
        json!({"request_id":"changes-error","error":{"code":code,"message":"Rejected","retryable":retryable}}),
    )
}

async fn serve(mock: Mock) -> (Client, Shared, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Mutex::new(mock));
    let app = Router::new()
        .route(
            "/api/v1/changes",
            get(
                |State(state): State<Shared>,
                 Query(query): Query<HashMap<String, String>>,
                 headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                    assert_eq!(query["org_id"], ORG);
                    assert!(!query.contains_key("token"));
                    let mut state = state.lock().unwrap();
                    state.cursors.push(query.get("cursor").cloned());
                    let (status, body) = state
                        .pages
                        .pop_front()
                        .expect("unexpected HTTP polling/request");
                    (StatusCode::from_u16(status).unwrap(), Json(body))
                },
            ),
        )
        .route(
            "/api/v1/changes/ws",
            get(
                |State(state): State<Shared>,
                 Query(query): Query<HashMap<String, String>>,
                 headers: HeaderMap,
                 ws: WebSocketUpgrade| async move {
                    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                    assert_eq!(query, HashMap::from([("org_id".into(), ORG.into())]));
                    let (script, rejected) = {
                        let mut state = state.lock().unwrap();
                        state.sockets += 1;
                        (
                            state.scripts.pop_front().unwrap_or_default(),
                            state.handshake_status,
                        )
                    };
                    if let Some(status) = rejected {
                        return StatusCode::from_u16(status).unwrap().into_response();
                    }
                    ws.on_upgrade(move |mut socket| async move {
                        for message in script {
                            let closing = matches!(message, Message::Close(_));
                            if socket.send(message).await.is_err() || closing {
                                return;
                            }
                        }
                        while socket.recv().await.is_some() {}
                    })
                    .into_response()
                },
            ),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(&format!("http://{}", listener.local_addr().unwrap()), TOKEN).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (client, state, server)
}

async fn next<S: Stream<Item = Result<ChangesWatchEvent>>>(
    stream: Pin<&mut S>,
) -> Result<ChangesWatchEvent> {
    tokio::time::timeout(Duration::from_secs(8), stream.into_future())
        .await
        .unwrap()
        .0
        .expect("stream ended")
}
fn delivered(event: ChangesWatchEvent) -> virse_api::ChangesPage {
    match event {
        ChangesWatchEvent::Page(page) => page,
        other => panic!("expected page: {other:?}"),
    }
}

#[tokio::test]
async fn baseline_ready_catchup_pagination_and_duplicate_hints_keep_delivered_cursor() {
    let (client, state, server) = serve(Mock {
        pages: VecDeque::from([
            page("baseline", true, false, json!([])),
            page("c1", false, true, json!([event("created")])),
            page("c2", false, false, json!([event("content_updated")])),
            page("c2", false, false, json!([])),
            page("c2", false, false, json!([])),
        ]),
        scripts: VecDeque::from([vec![ready(), available(), ready()]]),
        ..Mock::default()
    })
    .await;
    let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), None);
    futures_util::pin_mut!(stream);
    assert!(delivered(next(stream.as_mut()).await.unwrap()).bootstrap);
    assert_eq!(state.lock().unwrap().sockets, 0);
    let first = delivered(next(stream.as_mut()).await.unwrap());
    assert_eq!(first.items[0].kind, ChangeKind::Created);
    assert_eq!(first.next_cursor, "c1");
    let second = delivered(next(stream.as_mut()).await.unwrap());
    assert_eq!(second.items[0].kind, ChangeKind::ContentUpdated);
    assert_eq!(second.next_cursor, "c2");
    assert!(
        delivered(next(stream.as_mut()).await.unwrap())
            .items
            .is_empty()
    );
    assert!(
        delivered(next(stream.as_mut()).await.unwrap())
            .items
            .is_empty()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), stream.next())
            .await
            .is_err()
    );
    assert_eq!(
        state.lock().unwrap().cursors,
        vec![
            None,
            Some("baseline".into()),
            Some("c1".into()),
            Some("c2".into()),
            Some("c2".into())
        ]
    );
    server.abort();
}

#[tokio::test]
async fn reconnect_ready_catches_up_from_last_delivery_without_baseline_reset() {
    let (client, state, server) = serve(Mock {
        pages: VecDeque::from([
            page("c1", false, false, json!([event("renamed")])),
            page("c2", false, false, json!([event("scope_invalidated")])),
        ]),
        scripts: VecDeque::from([vec![ready(), close(1000)], vec![ready()]]),
        ..Mock::default()
    })
    .await;
    let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("resume"));
    futures_util::pin_mut!(stream);
    assert_eq!(
        delivered(next(stream.as_mut()).await.unwrap()).next_cursor,
        "c1"
    );
    let notice = next(stream.as_mut()).await.unwrap();
    assert!(
        matches!(notice, ChangesWatchEvent::Notice {cursor: Some(ref c), attempt: 1, ..} if c == "c1")
    );
    let page = delivered(next(stream.as_mut()).await.unwrap());
    assert_eq!(page.items[0].kind, ChangeKind::ScopeInvalidated);
    assert!(page.items[0].asset_id.is_none());
    assert_eq!(page.next_cursor, "c2");
    assert_eq!(
        state.lock().unwrap().cursors,
        vec![Some("resume".into()), Some("c1".into())]
    );
    assert_eq!(state.lock().unwrap().sockets, 2);
    server.abort();
}

#[tokio::test]
async fn disabled_auth_permission_and_expired_cursor_never_retry_even_if_marked_retryable() {
    for (status, code) in [
        (503, "changes_not_enabled"),
        (401, "invalid_token"),
        (403, "workspace_access_revoked"),
        (409, "cursor_expired"),
        (409, "sync_reset_required"),
    ] {
        for retryable in [false, true] {
            let (client, state, server) = serve(Mock {
                pages: VecDeque::from([error(status, code, retryable)]),
                scripts: VecDeque::from([vec![ready()]]),
                ..Mock::default()
            })
            .await;
            let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("old"));
            futures_util::pin_mut!(stream);
            let error = next(stream.as_mut()).await.unwrap_err();
            assert_eq!(error.status, Some(status));
            assert_eq!(error.code, code);
            if status == 409 {
                assert!(error.message.contains("full resource resync"));
            }
            assert!(stream.next().await.is_none());
            assert_eq!(state.lock().unwrap().cursors, vec![Some("old".into())]);
            assert_eq!(state.lock().unwrap().sockets, 1);
            server.abort();
        }
    }
}

#[tokio::test]
async fn disabled_bootstrap_is_immediate_and_never_opens_websocket() {
    let (client, state, server) = serve(Mock {
        pages: VecDeque::from([error(503, "changes_not_enabled", false)]),
        ..Mock::default()
    })
    .await;
    let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), None);
    futures_util::pin_mut!(stream);
    assert_eq!(
        next(stream.as_mut()).await.unwrap_err().code,
        "changes_not_enabled"
    );
    assert_eq!(state.lock().unwrap().sockets, 0);
    assert_eq!(state.lock().unwrap().cursors, vec![None]);
    server.abort();
}

#[tokio::test]
async fn successful_response_cannot_silently_reset_resume_cursor() {
    let (client, state, server) = serve(Mock {
        pages: VecDeque::from([page("new-baseline", true, false, json!([]))]),
        scripts: VecDeque::from([vec![ready()]]),
        ..Mock::default()
    })
    .await;
    let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("old"));
    futures_util::pin_mut!(stream);
    let error = next(stream.as_mut()).await.unwrap_err();
    assert_eq!(error.code, "sync_reset_required");
    assert_eq!(error.status, Some(409));
    assert_eq!(state.lock().unwrap().cursors, vec![Some("old".into())]);
    server.abort();
}

#[tokio::test]
async fn websocket_rejections_are_terminal_and_never_fetch_new_baseline() {
    for (close_code, status) in [
        (4000, 401),
        (4005, 401),
        (4006, 401),
        (1008, 401),
        (4403, 403),
        (4409, 409),
        (1013, 503),
    ] {
        let (client, state, server) = serve(Mock {
            scripts: VecDeque::from([vec![close(close_code)]]),
            ..Mock::default()
        })
        .await;
        let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("resume"));
        futures_util::pin_mut!(stream);
        let error = next(stream.as_mut()).await.unwrap_err();
        assert_eq!(error.status, Some(status));
        if status == 503 {
            assert_eq!(error.code, "CHANGES_UNAVAILABLE");
        }
        assert!(state.lock().unwrap().cursors.is_empty());
        assert_eq!(state.lock().unwrap().sockets, 1);
        server.abort();
    }
    for status in [401, 403, 409, 503] {
        let (client, state, server) = serve(Mock {
            handshake_status: Some(status),
            ..Mock::default()
        })
        .await;
        let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("resume"));
        futures_util::pin_mut!(stream);
        let error = next(stream.as_mut()).await.unwrap_err();
        assert_eq!(error.status, Some(status));
        if status == 503 {
            assert_eq!(error.code, "CHANGES_UNAVAILABLE");
        }
        assert!(state.lock().unwrap().cursors.is_empty());
        assert_eq!(state.lock().unwrap().sockets, 1);
        server.abort();
    }
}

#[tokio::test]
async fn revoked_notice_stops_and_foreign_org_hint_is_rejected() {
    for (message, code) in [
        (
            json!({"type":"workspace_access_revoked","clear_cache":true}),
            "workspace_access_revoked",
        ),
        (
            json!({"type":"asset_changes_ready","org_id":Uuid::new_v4()}),
            "INVALID_RESPONSE",
        ),
    ] {
        let (client, state, server) = serve(Mock {
            scripts: VecDeque::from([vec![Message::Text(message.to_string().into())]]),
            ..Mock::default()
        })
        .await;
        let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("resume"));
        futures_util::pin_mut!(stream);
        assert_eq!(next(stream.as_mut()).await.unwrap_err().code, code);
        assert!(stream.next().await.is_none());
        assert!(state.lock().unwrap().cursors.is_empty());
        server.abort();
    }
}

#[tokio::test]
async fn flapping_ready_does_not_reset_bounded_reconnect_budget() {
    let (client, state, server) = serve(Mock {
        pages: (0..5)
            .map(|_| page("same", false, false, json!([])))
            .collect(),
        scripts: (0..5).map(|_| vec![ready(), close(1000)]).collect(),
        ..Mock::default()
    })
    .await;
    let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("same"));
    futures_util::pin_mut!(stream);
    for attempt in 1..=5 {
        assert_eq!(
            delivered(next(stream.as_mut()).await.unwrap()).next_cursor,
            "same"
        );
        if attempt < 5 {
            assert!(
                matches!(next(stream.as_mut()).await.unwrap(), ChangesWatchEvent::Notice {attempt: n, ..} if n == attempt)
            );
        }
    }
    assert_eq!(
        next(stream.as_mut()).await.unwrap_err().code,
        "CHANGES_UNAVAILABLE"
    );
    assert_eq!(state.lock().unwrap().sockets, 5);
    assert_eq!(state.lock().unwrap().cursors.len(), 5);
    server.abort();
}

#[tokio::test]
async fn retryable_http_failure_retries_same_cursor_and_is_bounded() {
    let (client, state, server) = serve(Mock {
        pages: (0..4)
            .map(|_| error(502, "upstream_unavailable", true))
            .collect(),
        scripts: VecDeque::from([vec![ready()]]),
        ..Mock::default()
    })
    .await;
    let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some("unchanged"));
    futures_util::pin_mut!(stream);
    assert_eq!(next(stream.as_mut()).await.unwrap_err().status, Some(502));
    assert_eq!(
        state.lock().unwrap().cursors,
        vec![Some("unchanged".into()); 4]
    );
    assert_eq!(state.lock().unwrap().sockets, 1);
    server.abort();
}

#[tokio::test]
async fn invalid_pages_do_not_deliver_inconsistent_checkpoints_or_private_scope_targets() {
    for response in [
        page("same", false, true, json!([])),
        page("same", false, false, json!([event("created")])),
        page("", false, false, json!([])),
        page(
            "next",
            false,
            false,
            json!([{"event_id":Uuid::new_v4(),"kind":"scope_invalidated","resource_id":Uuid::new_v4(),"occurred_at":"now"}]),
        ),
    ] {
        let (client, _, server) = serve(Mock {
            pages: VecDeque::from([response]),
            ..Mock::default()
        })
        .await;
        assert_eq!(
            client
                .changes(Uuid::parse_str(ORG).unwrap(), Some("same"))
                .await
                .unwrap_err()
                .code,
            "INVALID_RESPONSE"
        );
        server.abort();
    }
}

#[tokio::test]
async fn empty_cursor_is_invalid_input_without_any_network_request() {
    let client = Client::new("http://127.0.0.1:1", TOKEN).unwrap();
    assert_eq!(
        client
            .changes(Uuid::parse_str(ORG).unwrap(), Some(""))
            .await
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
    let stream = client.watch_changes(Uuid::parse_str(ORG).unwrap(), Some(""));
    futures_util::pin_mut!(stream);
    assert_eq!(
        next(stream.as_mut()).await.unwrap_err().code,
        "INVALID_INPUT"
    );
}
