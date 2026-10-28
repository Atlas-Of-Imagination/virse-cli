use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{CloseFrame, Message},
    },
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;
use virse_api::{
    Client, ToolIdentity, ToolListOptions, ToolMutation, ToolRunAction, ToolRunRequest, ToolScope,
    ToolView,
};
type Records = Arc<Mutex<Vec<(String, Value)>>>;
type Queries = Arc<Mutex<Vec<HashMap<String, String>>>>;

const CANVAS: &str = "00000000-0000-0000-0000-000000000001";
const ORG: &str = "00000000-0000-0000-0000-000000000002";
const RUN: &str = "00000000-0000-0000-0000-000000000003";
const USER: &str = "00000000-0000-0000-0000-000000000004";
const TOKEN: &str = "virse_sk_tool_test";
fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}
fn snapshot(revision: u64, status: &str) -> Value {
    json!({"run_id":RUN,"kind":"tool","canvas_id":CANVAS,"element_id":USER,"owner_id":USER,"session_id":null,"agent_turn_id":null,"status":status,"revision":revision,"result":{"outputs":[]},"error":null,"actions":{"cancel":status=="queued","interrupt":status=="running","resume":false},"tool":{"skill_id":"tool-1","version_id":"artifact-1","version":"internal-label"}})
}
fn event(revision: u64, status: &str) -> Value {
    json!({"type":"canvas_run_updated","project_id":CANVAS,"run":snapshot(revision,status)})
}
async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (
        url,
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    )
}
fn request() -> ToolRunRequest {
    serde_json::from_value(json!({"org_id":ORG,"tool":{"skill_id":"tool-1","version_id":"artifact-1"},"locale":"zh","inputs":{"title":"hello"},"target":{"create_element":{"type":"skill","skill_id":"tool-1","version_id":"artifact-1","layout":{"position":{"x":10,"y":20}}}}})).unwrap()
}
#[tokio::test]
async fn management_uses_camel_case_and_preserves_release_identity() {
    let records = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let app = Router::new().route("/{*path}", post(|Path(path): Path<String>, State(records): State<Records>, headers: HeaderMap, Json(body): Json<Value>| async move {
        assert_eq!(headers["authorization"],format!("Bearer {TOKEN}"));
        assert!(body.get("organization_id").is_none());
        records.lock().unwrap().push((path.clone(),body));
        if path.ends_with("/versions") { Json(json!({"versions":[{"id":"release-2","artifactVersionId":"artifact-1","version":"2.0.0","current":true,"legacy":false,"releaseNote":"metadata only"}]})) }
        else { Json(json!({"ok":true})) }
    })).with_state(records.clone());
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    for view in [ToolView::Created, ToolView::Mine, ToolView::Store] {
        client
            .tools(
                view,
                &ToolListOptions {
                    organization_id: Some(id(ORG)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    client.tool("tool-1", Some(id(ORG)), "en").await.unwrap();
    let versions = client.tool_versions("tool-1", Some(id(ORG))).await.unwrap();
    assert_eq!(versions.versions[0].id, "release-2");
    assert_eq!(versions.versions[0].artifact_version_id, "artifact-1");
    client
        .set_tool_scope("tool-1", Some(id(ORG)), ToolScope::Team)
        .await
        .unwrap();
    for action in [
        ToolMutation::Add,
        ToolMutation::Remove,
        ToolMutation::Enable,
        ToolMutation::Disable,
        ToolMutation::Star,
        ToolMutation::Unstar,
        ToolMutation::Delete,
    ] {
        client
            .mutate_tool("tool-1", Some(id(ORG)), action)
            .await
            .unwrap();
    }
    let records = records.lock().unwrap();
    assert_eq!(records[0].0, "skills/created/list");
    assert_eq!(records[1].0, "skills/mine/list");
    assert_eq!(records[2].0, "skills/store/list");
    assert_eq!(records[5].1["accessScope"], "team");
    assert_eq!(records[10].1["starred"], true);
    assert_eq!(records[11].1["starred"], false);
    assert_eq!(records[12].0, "skills/creator/delete");
    assert!(records.iter().all(|(_, b)| b["organizationId"] == ORG));
    server.abort();
}
#[tokio::test]
async fn formal_schema_resolves_without_owner_api_and_pins_exact_artifact() {
    let queries = Arc::new(Mutex::new(Vec::<HashMap<String, String>>::new()));
    let app=Router::new().route("/v2/skills/tool-1/input-schema",get(|State(queries):State<Queries>, Query(query):Query<HashMap<String,String>>,headers:HeaderMap|async move {
        assert_eq!(headers["authorization"],format!("Bearer {TOKEN}"));
        assert_eq!(query["canvas_id"],CANVAS); assert_eq!(query["org_id"],ORG);
        queries.lock().unwrap().push(query.clone());
        let pinned=query.contains_key("version_id");
        if pinned { assert_eq!(query["version_id"],"artifact-1"); assert!(!query.contains_key("version")); }
        else { assert_eq!(query["version"],"2.0.0"); }
        Json(json!({"skill_id":"tool-1","version_id":"artifact-1","version":"1.0.0","input_schema":{"version":1,"fields":[{"id":if pinned {"frozen"}else{"live"},"labelEn":"Caption"}]}}))
    })).with_state(queries.clone());
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    let schema = client
        .tool_schema(
            id(CANVAS),
            id(ORG),
            &ToolIdentity {
                skill_id: "tool-1".into(),
                version: Some("2.0.0".into()),
                version_id: None,
            },
            "en",
        )
        .await
        .unwrap();
    assert_eq!(schema.input_schema.unwrap()["fields"][0]["id"], "frozen");
    assert_eq!(queries.lock().unwrap().len(), 2);
    server.abort();
}
#[tokio::test]
async fn v2_submission_actions_and_business_errors_are_authoritative() {
    let app=Router::new().route(&format!("/v2/canvas/projects/{CANVAS}/tool-runs"),post(|headers:HeaderMap,Json(body):Json<Value>|async move{
        assert_eq!(headers["idempotency-key"],"same-key"); assert_eq!(headers["x-virse-expected-user"],USER); assert_eq!(body["org_id"],ORG);
        assert!(body.get("organizationId").is_none()); assert_eq!(body["tool"]["version_id"],"artifact-1"); assert!(body["tool"].get("version").is_none());
        (StatusCode::ACCEPTED,Json(snapshot(1,"queued")))
    })).route(&format!("/v2/canvas/projects/{CANVAS}/tool-runs/{RUN}/{{action}}"),post(|Path(action):Path<String>,Query(query):Query<HashMap<String,String>>|async move{
        assert_eq!(query["org_id"],ORG);
        match action.as_str(){"interrupt"=>(StatusCode::ACCEPTED,Json(snapshot(3,"interrupting"))),"resume"=>(StatusCode::ACCEPTED,Json(snapshot(4,"queued"))),_ =>(StatusCode::CONFLICT,Json(json!({"detail":"CANVAS_RUN_ALREADY_DISPATCHED"})))}
    }));
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    assert_eq!(
        client
            .submit_tool_run_as(id(CANVAS), &request(), "same-key", id(USER))
            .await
            .unwrap()
            .status,
        "queued"
    );
    let interrupted = client
        .tool_run_action(id(CANVAS), id(ORG), id(RUN), ToolRunAction::Interrupt)
        .await
        .unwrap();
    assert_eq!(interrupted.status, "interrupting");
    assert!(!interrupted.stop_waiting());
    assert!(!interrupted.actions.cancel);
    assert_eq!(
        client
            .tool_run_action(id(CANVAS), id(ORG), id(RUN), ToolRunAction::Resume)
            .await
            .unwrap()
            .status,
        "queued"
    );
    assert_eq!(
        client
            .tool_run_action(id(CANVAS), id(ORG), id(RUN), ToolRunAction::Cancel)
            .await
            .unwrap_err()
            .code,
        "CANVAS_RUN_ALREADY_DISPATCHED"
    );
    server.abort();
}
#[derive(Default)]
struct Observer {
    ready: AtomicBool,
    reads: AtomicUsize,
    sockets: AtomicUsize,
}
#[tokio::test]
async fn watcher_waits_for_active_users_reconciles_and_ignores_stale_or_unrelated_events() {
    let state = Arc::new(Observer::default());
    let app=Router::new().route(&format!("/v2/canvas/projects/{CANVAS}/tool-runs/{RUN}"),get(|State(s):State<Arc<Observer>>|async move{
        assert!(s.ready.load(Ordering::SeqCst)); s.reads.fetch_add(1,Ordering::SeqCst); Json(snapshot(2,"running"))
    })).route(&format!("/canvas/ws/{CANVAS}"),get(|State(s):State<Arc<Observer>>,headers:HeaderMap,ws:WebSocketUpgrade|async move{
        assert_eq!(headers["authorization"],format!("Bearer {TOKEN}"));
        ws.on_upgrade(move|mut socket|async move{
            tokio::time::sleep(Duration::from_millis(60)).await; assert_eq!(s.reads.load(Ordering::SeqCst),0);
            s.ready.store(true,Ordering::SeqCst);
            socket.send(Message::Text(json!({"type":"active_users","users":[]}).to_string().into())).await.unwrap();
            while s.reads.load(Ordering::SeqCst)==0 { tokio::task::yield_now().await; }
            tokio::time::sleep(Duration::from_millis(80)).await;
            let mut unrelated=event(99,"succeeded"); unrelated["run"]["run_id"]=json!(USER);
            for v in [event(1,"succeeded"),unrelated,event(3,"running"),event(3,"succeeded"),event(4,"needs_attention")] { socket.send(Message::Text(v.to_string().into())).await.unwrap(); }
            // There is no subscription command or POST of any kind.
            if let Ok(Some(Ok(Message::Text(text))))=tokio::time::timeout(Duration::from_millis(80),socket.recv()).await { assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["type"],"ping"); }
        })
    })).with_state(state.clone());
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    let runs = tokio::time::timeout(
        Duration::from_secs(3),
        client
            .watch_tool_run(id(CANVAS), id(ORG), id(RUN))
            .collect::<Vec<_>>(),
    )
    .await
    .unwrap();
    let runs: Vec<_> = runs.into_iter().map(Result::unwrap).collect();
    assert_eq!(
        runs.iter().map(|r| r.revision).collect::<Vec<_>>(),
        vec![2, 3, 4]
    );
    assert_eq!(runs.last().unwrap().status, "needs_attention");
    assert_eq!(state.reads.load(Ordering::SeqCst), 1);
    server.abort();
}
#[tokio::test]
async fn reconnect_get_catches_a_completion_without_any_event() {
    let state = Arc::new(Observer::default());
    let app = Router::new()
        .route(
            &format!("/v2/canvas/projects/{CANVAS}/tool-runs/{RUN}"),
            get(|State(s): State<Arc<Observer>>| async move {
                s.reads.fetch_add(1, Ordering::SeqCst);
                Json(if s.sockets.load(Ordering::SeqCst) > 1 {
                    snapshot(2, "succeeded")
                } else {
                    snapshot(1, "running")
                })
            }),
        )
        .route(
            &format!("/canvas/ws/{CANVAS}"),
            get(
                |State(s): State<Arc<Observer>>, ws: WebSocketUpgrade| async move {
                    ws.on_upgrade(move |mut socket| async move {
                        let count = s.sockets.fetch_add(1, Ordering::SeqCst) + 1;
                        socket
                            .send(Message::Text(
                                json!({"type":"active_users"}).to_string().into(),
                            ))
                            .await
                            .unwrap();
                        while s.reads.load(Ordering::SeqCst) < count {
                            tokio::task::yield_now().await;
                        }
                        if count == 1 {
                            socket.send(Message::Close(None)).await.unwrap();
                        } else {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    })
                },
            ),
        )
        .with_state(state.clone());
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    let runs = tokio::time::timeout(
        Duration::from_secs(5),
        client
            .watch_tool_run(id(CANVAS), id(ORG), id(RUN))
            .collect::<Vec<_>>(),
    )
    .await
    .unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].as_ref().unwrap().status, "succeeded");
    assert_eq!(state.reads.load(Ordering::SeqCst), 2);
    server.abort();
}
#[tokio::test]
async fn ws_auth_close_before_ready_fails_without_get_or_retry() {
    let state = Arc::new(Observer::default());
    let app = Router::new()
        .route(
            &format!("/canvas/ws/{CANVAS}"),
            get(
                |State(s): State<Arc<Observer>>, ws: WebSocketUpgrade| async move {
                    s.sockets.fetch_add(1, Ordering::SeqCst);
                    ws.on_upgrade(|mut socket| async move {
                        socket
                            .send(Message::Close(Some(CloseFrame {
                                code: 4003,
                                reason: "denied".into(),
                            })))
                            .await
                            .unwrap();
                    })
                },
            ),
        )
        .with_state(state.clone());
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    let stream = client.watch_tool_run(id(CANVAS), id(ORG), id(RUN));
    futures_util::pin_mut!(stream);
    let error = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.status, Some(403));
    assert_eq!(state.sockets.load(Ordering::SeqCst), 1);
    server.abort();
}
#[test]
fn unpinned_or_mismatched_run_is_rejected_locally() {
    let mut body = request();
    body.tool.version_id = None;
    assert!(body.validate().is_err());
    let mut body = request();
    body.target.create_element.as_mut().unwrap().version_id = Some("different".into());
    assert!(body.validate().is_err());
    let mut body = request();
    body.target.element_id = Some(id(USER));
    assert!(body.validate().is_err());
    let mut body = request();
    body.target
        .create_element
        .as_mut()
        .unwrap()
        .layout
        .position
        .as_mut()
        .unwrap()
        .x = f64::NAN;
    assert!(body.validate().is_err());
}

#[tokio::test]
async fn get_finds_own_unpublished_draft_beyond_first_page_in_same_org() {
    let offsets = Arc::new(Mutex::new(Vec::<u64>::new()));
    let app = Router::new()
        .route("/skills/store/detail", post(|Json(body): Json<Value>| async move {
            assert_eq!(body["organizationId"], ORG);
            (StatusCode::NOT_FOUND, Json(json!({"code":"SKILL_NOT_FOUND","message":"skill not found"})))
        }))
        .route("/skills/created/list", post(|State(offsets): State<Arc<Mutex<Vec<u64>>>>, headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
            assert_eq!(body["organizationId"], ORG);
            assert_eq!(body["locale"], "en");
            assert_eq!(body["limit"], 100);
            assert!(body.get("query").is_none());
            let offset = body["offset"].as_u64().unwrap();
            offsets.lock().unwrap().push(offset);
            let skills = if offset == 0 {
                (0..100).map(|n| json!({"id":format!("other-{n}")})).collect::<Vec<_>>()
            } else {
                assert_eq!(offset, 100);
                vec![json!({"id":"draft-tool","reviewStatus":"draft","accessScope":"personal"})]
            };
            Json(json!({"skills":skills,"total":101}))
        })).with_state(offsets.clone());
    let (base, server) = serve(app).await;
    let client = Client::new(&base, TOKEN).unwrap();
    let value = client
        .tool("draft-tool", Some(id(ORG)), "en")
        .await
        .unwrap();
    assert_eq!(value["skill"]["id"], "draft-tool");
    assert_eq!(value["skill"]["reviewStatus"], "draft");
    assert_eq!(*offsets.lock().unwrap(), vec![0, 100]);
    server.abort();
}

#[tokio::test]
async fn get_never_falls_back_on_access_service_or_unrelated_not_found_errors() {
    for (status, code) in [
        (403, "SKILL_NOT_FOUND"),
        (500, "SKILL_NOT_FOUND"),
        (404, "OTHER_NOT_FOUND"),
    ] {
        let reads = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/skills/store/detail",
                post(move || async move {
                    (
                        StatusCode::from_u16(status).unwrap(),
                        Json(json!({"code":code,"message":"denied"})),
                    )
                }),
            )
            .route(
                "/skills/created/list",
                post(|State(reads): State<Arc<AtomicUsize>>| async move {
                    reads.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"skills":[{"id":"draft-tool"}],"total":1}))
                }),
            )
            .with_state(reads.clone());
        let (base, server) = serve(app).await;
        let client = Client::new(&base, TOKEN).unwrap();
        let error = client
            .tool("draft-tool", Some(id(ORG)), "zh")
            .await
            .unwrap_err();
        assert_eq!(error.status, Some(status));
        assert_eq!(error.code, code);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        server.abort();
    }
}

#[tokio::test]
async fn draft_abandon_sends_one_scoped_camel_case_request_even_on_failure() {
    for status in [
        StatusCode::OK,
        StatusCode::FORBIDDEN,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let posts = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route("/skills/tool-1/draft/abandon", post(
            move |State(posts): State<Arc<AtomicUsize>>, headers: HeaderMap, Json(body): Json<Value>| async move {
                posts.fetch_add(1, Ordering::SeqCst);
                assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                assert!(!headers.contains_key("idempotency-key"));
                assert_eq!(body, json!({"skillId":"tool-1","revisionId":"revision-1","organizationId":ORG}));
                if status == StatusCode::OK {
                    (status, Json(json!({"success":true,"revisionId":"revision-1"})))
                } else {
                    (status, Json(json!({"code":"DRAFT_REJECTED","message":"denied"})))
                }
            }
        )).with_state(posts.clone());
        let (base, server) = serve(app).await;
        let client = Client::new(&base, TOKEN).unwrap();
        let result = client
            .abandon_tool_draft("tool-1", "revision-1", Some(id(ORG)))
            .await;
        if status == StatusCode::OK {
            assert_eq!(
                result.unwrap(),
                json!({"success":true,"revisionId":"revision-1"})
            );
        } else {
            assert_eq!(result.unwrap_err().status, Some(status.as_u16()));
        }
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
