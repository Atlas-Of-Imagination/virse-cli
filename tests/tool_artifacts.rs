#![allow(dead_code)]
#[path = "../src/tool_project.rs"]
mod tool_project;
#[path = "../src/tool_publish.rs"]
mod tool_publish;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    sync::{Arc, Mutex},
};
use uuid::Uuid;
use virse_api::Client;
const USER: &str = "00000000-0000-0000-0000-000000000001";
const ORG: &str = "00000000-0000-0000-0000-000000000002";
const TOOL: &str = "00000000-0000-0000-0000-000000000003";
#[derive(Default)]
struct Server {
    current: Value,
    artifacts: HashMap<String, Value>,
    replies: HashMap<String, Value>,
    requests: HashMap<String, (Value, Vec<u8>)>,
    calls: Vec<String>,
    lose: String,
    release_count: usize,
    deny: bool,
}
type Shared = Arc<Mutex<Server>>;
fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|s| s == needle)
}
fn parse_multipart(bytes: &[u8], headers: &HeaderMap) -> (Value, Vec<u8>) {
    let content = headers["content-type"].to_str().unwrap();
    let boundary = content.split("boundary=").nth(1).unwrap();
    let marker = format!("--{boundary}").into_bytes();
    let mut pos = 0;
    let mut fields = serde_json::Map::new();
    let mut zip = Vec::new();
    while let Some(begin) = find(&bytes[pos..], &marker) {
        let start = pos + begin + marker.len();
        if bytes.get(start..start + 2) == Some(b"--") {
            break;
        }
        let next = find(&bytes[start..], &marker).map(|p| start + p).unwrap();
        let sep = find(&bytes[start..next], b"\r\n\r\n")
            .map(|p| start + p)
            .unwrap();
        let header = std::str::from_utf8(&bytes[start..sep]).unwrap();
        let name = header
            .split("name=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let value = &bytes[sep + 4..next - 2];
        if name == "package_file" {
            zip = value.to_vec();
        } else {
            assert!(
                !fields.contains_key(name),
                "duplicate multipart field {name}"
            );
            fields.insert(name.into(), json!(std::str::from_utf8(value).unwrap()));
        }
        pos = next;
    }
    (Value::Object(fields), zip)
}

async fn endpoint(
    State(state): State<Shared>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    let path = uri.path().to_owned();
    let key = headers
        .get("idempotency-key")
        .map(|v| v.to_str().unwrap().to_owned());
    let (body, package) = if headers
        .get("content-type")
        .is_some_and(|v| v.to_str().unwrap().starts_with("multipart/"))
    {
        parse_multipart(&bytes, &headers)
    } else {
        (
            if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            },
            vec![],
        )
    };
    let mut s = state.lock().unwrap();
    s.calls.push(path.clone());
    if let Some(key) = &key {
        if let Some(old) = s.requests.get(key) {
            assert_eq!(old, &(body.clone(), package.clone()));
        } else {
            s.requests
                .insert(key.clone(), (body.clone(), package.clone()));
        }
        if let Some(reply) = s.replies.get(key) {
            return Json(reply.clone()).into_response();
        }
    }
    let response = if path == "/v2/skills" {
        assert!(s.current.is_null(), "duplicate tool identity");
        s.current = json!({"id":TOOL,"creatorUserId":USER,"name":body["name"],"description":body["description"],
            "detail":body["detail"],"metadataHash":"base","versionId":null,"releaseId":null,"accessScope":"personal"});
        json!({"skill":s.current})
    } else if path.ends_with("/artifacts") {
        if s.deny {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"code":"DENIED","message":"denied"})),
            )
                .into_response();
        }
        assert_eq!(body["expected_metadata_hash"], s.current["metadataHash"]);
        let id = Uuid::new_v4().to_string();
        let artifact = json!({"id":id,"skillId":TOOL,"checksum":tool_project::digest(&package),
            "validation":{"valid":true,"warnings":[{"code":"HINT"}]},
            "publicationBase":{"releaseId":s.current["releaseId"],"metadataHash":s.current["metadataHash"]}});
        s.artifacts.insert(id, artifact.clone());
        json!({"skill":s.current,"artifact":artifact})
    } else if path.ends_with("/releases") {
        let id = body["artifact_id"].as_str().unwrap();
        assert!(s.artifacts.contains_key(id));
        assert_eq!(body["expected_metadata_hash"], s.current["metadataHash"]);
        s.release_count += 1;
        s.current["versionId"] = json!(id);
        s.current["releaseId"] = json!(Uuid::new_v4());
        s.current["metadataHash"] = json!(format!("hash-{}", s.release_count));
        json!({"skill":s.current,"versionId":id,"releaseId":s.current["releaseId"],"version":"1.0.0","noOp":false})
    } else if path.contains("/artifacts/") {
        json!({"skill":s.current,"artifact":s.artifacts[path.rsplit('/').next().unwrap()]})
    } else {
        panic!("unexpected route {path}")
    };
    if let Some(key) = key {
        s.replies.insert(key, response.clone());
    }
    if s.lose == path {
        s.lose.clear();
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"code":"LOST","message":"lost response"})),
        )
            .into_response();
    }
    Json(response).into_response()
}
async fn server() -> (
    Client,
    tool_publish::AuthoringIdentity,
    Shared,
    tokio::task::JoinHandle<()>,
) {
    let state = Arc::new(Mutex::new(Server::default()));
    let app = Router::new()
        .fallback(any(endpoint))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        Client::new(&url, "virse_sk_test").unwrap(),
        tool_publish::AuthoringIdentity::new(
            &url,
            Uuid::parse_str(USER).unwrap(),
            Uuid::parse_str(ORG).unwrap(),
        )
        .unwrap(),
        state,
        task,
    )
}
#[tokio::test]
async fn each_v2_write_resumes_frozen_payload_without_duplicate_creation() {
    for path in [
        "/v2/skills".to_string(),
        format!("/v2/skills/{TOOL}/artifacts"),
        format!("/v2/skills/{TOOL}/releases"),
    ] {
        let (client, identity, state, task) = server().await;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let journal = tmp.path().join("journal");
        tool_project::init(&project, Some("Demo")).unwrap();
        state.lock().unwrap().lose = path;
        assert!(
            tool_publish::author(&client, &project, &journal, &identity, None, false, false)
                .await
                .is_err()
        );
        fs::write(project.join("SKILL.md"), "changed invalid contents").unwrap();
        let result =
            tool_publish::author(&client, &project, &journal, &identity, None, true, false)
                .await
                .unwrap();
        assert_eq!(result["skill"]["id"], TOOL);
        assert_eq!(state.lock().unwrap().release_count, 1);
        assert_eq!(state.lock().unwrap().artifacts.len(), 1);
        task.abort();
    }
}
#[tokio::test]
async fn build_does_not_publish_and_explicit_artifact_publication_replays() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let journal = tmp.path().join("journal");
    tool_project::init(&project, Some("Demo")).unwrap();
    let built = tool_publish::author(&client, &project, &journal, &identity, None, false, true)
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().release_count, 0);
    let artifact = Uuid::parse_str(built["artifact"]["id"].as_str().unwrap()).unwrap();
    for _ in 0..2 {
        tool_publish::publish_artifact(
            &client,
            &identity,
            Uuid::parse_str(TOOL).unwrap(),
            artifact,
            None,
        )
        .await
        .unwrap();
    }
    assert_eq!(state.lock().unwrap().release_count, 1);
    task.abort();
}
#[tokio::test]
async fn rejected_upload_keeps_the_confirmed_identity() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let journal = tmp.path().join("journal");
    tool_project::init(&project, Some("Demo")).unwrap();
    state.lock().unwrap().deny = true;
    assert!(
        tool_publish::author(&client, &project, &journal, &identity, None, false, false)
            .await
            .is_err()
    );
    state.lock().unwrap().deny = false;
    fs::write(project.join("extra.txt"), "corrected content").unwrap();
    tool_publish::author(&client, &project, &journal, &identity, None, false, false)
        .await
        .unwrap();
    assert_eq!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|p| p.as_str() == "/v2/skills")
            .count(),
        1
    );
    task.abort();
}
