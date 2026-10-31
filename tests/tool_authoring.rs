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
    routing::post,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Cursor, Write},
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;
use virse_api::Client;
use zip::{ZipWriter, write::SimpleFileOptions};
const USER: &str = "00000000-0000-0000-0000-000000000001";
const ORG: &str = "00000000-0000-0000-0000-000000000002";
const SKILL: &str = "00000000-0000-0000-0000-000000000003";
const V1: &str = "00000000-0000-0000-0000-000000000004";
const V2: &str = "00000000-0000-0000-0000-000000000005";
const R1: &str = "00000000-0000-0000-0000-000000000006";
const R2: &str = "00000000-0000-0000-0000-000000000007";
const DRAFT: &str = "00000000-0000-0000-0000-000000000008";
fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}
fn initialized(path: &Path) {
    tool_project::init(path, Some("Example")).unwrap();
}
fn change(path: &Path, name: &str) {
    let mut meta: Value =
        serde_json::from_slice(&fs::read(path.join("meta.json")).unwrap()).unwrap();
    meta["name"] = json!(name);
    fs::write(path.join("meta.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
}
fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn skill_md() -> Vec<u8> {
    b"---\nname: Example\ndescription: Do a useful task.\nwhen_to_use: Use this tool when the user requests a useful task.\n---\n\nInstructions.\n".to_vec()
}
#[test]
fn init_refuses_existing_and_pack_is_deterministic_and_readonly() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("tool");
    initialized(&project);
    assert_eq!(
        tool_project::init(&project, None).unwrap_err().code,
        "DESTINATION_EXISTS"
    );
    fs::create_dir(project.join("scripts")).unwrap();
    fs::write(project.join("scripts/must-not-run.sh"), "exit 99\n").unwrap();
    fs::write(project.join(".env"), "SECRET=do-not-package").unwrap();
    fs::create_dir(project.join("node_modules")).unwrap();
    fs::write(project.join("node_modules/secret"), "secret").unwrap();
    let before = fs::read(project.join("SKILL.md")).unwrap();
    let a = tool_project::pack(&project).unwrap();
    let b = tool_project::pack(&project).unwrap();
    assert_eq!(a.bytes, b.bytes);
    assert!(a.excluded.contains(&".env".into()));
    assert!(a.files.contains(&"scripts/must-not-run.sh".into()));
    assert_eq!(fs::read(project.join("SKILL.md")).unwrap(), before);
    assert!(
        tool_project::unpack(&a.bytes, &a.checksum)
            .unwrap()
            .contains_key("scripts/must-not-run.sh")
    );
}
#[test]
fn validation_rejects_missing_frontmatter_duplicate_json_bad_interface_and_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("tool");
    initialized(&project);
    fs::write(project.join("SKILL.md"), "# no frontmatter").unwrap();
    assert!(tool_project::validate(&project).is_err());
    fs::write(project.join("SKILL.md"), skill_md()).unwrap();
    for bad in [
        r#"{"fields":[],"fields":[]}"#,
        r#"{"fields":[{"id":"x","type":"missing"}]}"#,
        r#"{"fields":[],"promptTemplate":"{{unknown}}"}"#,
        r#"{"version":true,"fields":[]}"#,
    ] {
        fs::write(project.join("interface.json"), bad).unwrap();
        assert!(tool_project::validate(&project).is_err(), "{bad}");
    }
    fs::write(
        project.join("interface.json"),
        r#"{"version":1,"fields":[]}"#,
    )
    .unwrap();
    fs::write(
        project.join("seemingly-safe.txt"),
        "-----BEGIN PRIVATE KEY-----",
    )
    .unwrap();
    assert_eq!(
        tool_project::pack(&project).unwrap_err().code,
        "PACKAGE_SECRET_DETECTED"
    );
}
#[test]
fn scalar_only_frontmatter_and_root_skill_required() {
    let temp = tempfile::tempdir().unwrap();
    let p = temp.path().join("tool");
    initialized(&p);
    fs::write(p.join("SKILL.md"),"---\nname: Example\ndescription: |\n  more text\nwhen_to_use: Use this tool for a concrete user task.\n---\nBody\n").unwrap();
    assert!(
        tool_project::validate(&p)
            .unwrap_err()
            .message
            .contains("one-line")
            || tool_project::validate(&p)
                .unwrap_err()
                .message
                .contains("unsupported")
    );
}
#[cfg(unix)]
#[test]
fn refuses_symlinks_hardlinks_and_fifo() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let p = temp.path().join("tool");
    initialized(&p);
    fs::write(temp.path().join("outside"), "private").unwrap();
    symlink(temp.path().join("outside"), p.join("linked.txt")).unwrap();
    assert!(tool_project::pack(&p).is_err());
    fs::remove_file(p.join("linked.txt")).unwrap();
    fs::hard_link(temp.path().join("outside"), p.join("hard.txt")).unwrap();
    assert!(tool_project::pack(&p).is_err());
    fs::remove_file(p.join("hard.txt")).unwrap();
    symlink(&p, temp.path().join("alias")).unwrap();
    assert!(tool_project::pack(&temp.path().join("alias")).is_err());
    rustix::fs::mknodat(
        rustix::fs::CWD,
        p.join("pipe"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    assert!(tool_project::pack(&p).is_err());
}
#[test]
fn zip_rejects_aliases_collisions_checksum_and_symlink() {
    let md = skill_md();
    for name in [
        "../escape",
        "/absolute",
        "./alias",
        "folder//alias",
        "C:/drive",
        "a\\b",
        ".env",
    ] {
        let bytes = archive(&[("SKILL.md", &md), (name, b"payload")]);
        assert!(
            tool_project::unpack(&bytes, &tool_project::digest(&bytes)).is_err(),
            "{name}"
        );
    }
    let bytes = archive(&[("SKILL.md", &md), ("a", b"file"), ("a/b", b"child")]);
    assert!(tool_project::unpack(&bytes, &tool_project::digest(&bytes)).is_err());
    let bytes = archive(&[("SKILL.md", &md)]);
    assert_eq!(
        tool_project::unpack(&bytes, "wrong").unwrap_err().code,
        "PACKAGE_CHECKSUM_MISMATCH"
    );
    let mut bytes = archive(&[("SKILL.md", &md), ("link", b"/etc/passwd")]);
    let central: Vec<usize> = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, v)| *v == b"PK\x01\x02")
        .map(|(p, _)| p)
        .collect();
    let p = central[1] + 38;
    bytes[p..p + 4].copy_from_slice(&((0o120777u32) << 16).to_le_bytes());
    assert!(tool_project::unpack(&bytes, &tool_project::digest(&bytes)).is_err());
}
#[test]
fn zip_duplicate_entries_not_hidden_by_library_and_size_budget() {
    let md = skill_md();
    let mut bytes = archive(&[("SKILL.md", &md), ("aaa", b"a"), ("bbb", b"b")]);
    for p in (0..bytes.len() - 3).collect::<Vec<_>>() {
        if &bytes[p..p + 3] == b"bbb" {
            bytes[p..p + 3].copy_from_slice(b"aaa");
        }
    }
    assert!(tool_project::unpack(&bytes, &tool_project::digest(&bytes)).is_err());
    let mut bytes = archive(&[("SKILL.md", &md)]);
    let p = bytes.windows(4).position(|v| v == b"PK\x01\x02").unwrap() + 24;
    bytes[p..p + 4].copy_from_slice(&(101u32 * 1024 * 1024).to_le_bytes());
    assert!(tool_project::unpack(&bytes, &tool_project::digest(&bytes)).is_err());
}
#[test]
fn install_never_overwrites_existing_directory_or_link() {
    let temp = tempfile::tempdir().unwrap();
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();
    let files = BTreeMap::from([("SKILL.md".into(), skill_md())]);
    assert_eq!(
        tool_project::install_files(&dest, &files).unwrap_err().code,
        "DESTINATION_EXISTS"
    );
    assert!(!dest.join("SKILL.md").exists());
}

#[derive(Clone)]
struct Snapshot {
    bytes: Vec<u8>,
    metadata: Value,
}
struct Server {
    calls: Vec<(String, Option<String>, Value)>,
    lost: HashSet<String>,
    replies: HashMap<String, Value>,
    payloads: HashMap<String, (Value, Vec<u8>)>,
    current: Value,
    snapshots: HashMap<String, Snapshot>,
    draft: Option<Snapshot>,
    bad_checksum: bool,
    invalid_validation: bool,
    cas_conflict: bool,
    deny_team: bool,
    draft_revision: u64,
}
impl Default for Server {
    fn default() -> Self {
        Self {
            calls: vec![],
            lost: HashSet::new(),
            replies: HashMap::new(),
            payloads: HashMap::new(),
            current: json!({"id":SKILL,"creatorUserId":USER,"versionId":V1,"releaseId":null,"metadataHash":"hash-initial","accessScope":"personal","name":"Example","version":"1.0.0"}),
            snapshots: HashMap::new(),
            draft: None,
            bad_checksum: false,
            invalid_validation: false,
            cas_conflict: false,
            deny_team: false,
            draft_revision: 0,
        }
    }
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
    assert_eq!(headers["authorization"], "Bearer virse_sk_test_authoring");
    let path = uri.path().to_owned();
    let key = headers
        .get("idempotency-key")
        .map(|s| s.to_str().unwrap().to_owned());
    let (body, package) = if headers
        .get("content-type")
        .is_some_and(|s| s.to_str().unwrap().starts_with("multipart/"))
    {
        parse_multipart(&bytes, &headers)
    } else {
        (serde_json::from_slice(&bytes).unwrap(), vec![])
    };
    let mut s = state.lock().unwrap();
    s.calls.push((path.clone(), key.clone(), body.clone()));
    if let Some(key) = &key {
        if let Some(old) = s.payloads.get(key) {
            assert_eq!(
                old,
                &(body.clone(), package.clone()),
                "idempotency payload changed"
            );
        } else {
            s.payloads
                .insert(key.clone(), (body.clone(), package.clone()));
        }
        if let Some(reply) = s.replies.get(key) {
            return Json(reply.clone()).into_response();
        }
    }
    if (path.ends_with("/creator/finalize") || path.ends_with("/draft/publish"))
        && s.deny_team
        && body.get("accessScope") == Some(&json!("team"))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"code":"TEAM_REQUIRED","message":"team required"})),
        )
            .into_response();
    }
    let result = if path.ends_with("/creator/create") {
        assert_eq!(body["organization_id"], ORG);
        assert_eq!(body["package_source"], "uploaded_package");
        for field in ["name", "description", "detail", "scenario"] {
            s.current[field] = body[field].clone();
        }
        s.snapshots.insert(V1.into(),Snapshot{bytes:package,metadata:json!({"name":body["name"],"description":body["description"],"detail":body["detail"],"scenario":body["scenario"]})});
        json!({"skill":s.current})
    } else if path.ends_with("/creator/replace") {
        assert_eq!(body["skill_id"], SKILL);
        assert_eq!(body["expected_version_id"], s.current["versionId"]);
        s.current["versionId"] = json!(V2);
        s.current["metadataHash"] = json!("hash-replaced");
        s.current["name"] = body["name"].clone();
        s.snapshots.insert(
            V2.into(),
            Snapshot {
                bytes: package,
                metadata: json!({"name":body["name"]}),
            },
        );
        json!({"skill":s.current})
    } else if path.ends_with("/creator/update") {
        s.current["accessScope"] = body["accessScope"].clone();
        s.current["metadataHash"] = json!(format!(
            "hash-scope-{}",
            body["accessScope"].as_str().unwrap()
        ));
        json!({"skill":s.current})
    } else if path.ends_with("/creator/finalize") {
        assert_eq!(body["expectedVersionId"], s.current["versionId"]);
        assert_eq!(body["expectedMetadataHash"], s.current["metadataHash"]);
        s.current["releaseId"] = json!(R1);
        if let Some(scope) = body.get("accessScope") {
            s.current["accessScope"] = scope.clone();
            s.current["metadataHash"] = json!(format!("hash-scope-{}", scope.as_str().unwrap()));
        }
        json!({"skill":s.current,"versionId":s.current["versionId"],"releaseId":R1,"version":"1.0.0","noOp":false})
    } else if path.ends_with("/draft") {
        if s.cas_conflict {
            return (
                StatusCode::CONFLICT,
                Json(json!({"detail":{"code":"SKILL_VERSION_CONFLICT","message":"base changed"}})),
            )
                .into_response();
        }
        let options: Value = serde_json::from_str(body["options"].as_str().unwrap()).unwrap();
        assert_eq!(options["baseVersionId"], s.current["versionId"]);
        assert_eq!(options["expectedMetadataHash"], s.current["metadataHash"]);
        if s.draft.is_some() {
            assert_eq!(options["revisionId"], DRAFT);
            assert_eq!(options["expectedRevision"], s.draft_revision);
        }
        s.draft_revision += 1;
        s.draft = Some(Snapshot {
            bytes: package,
            metadata: options["config"].clone(),
        });
        json!({"draft":{"revisionId":DRAFT,"baseVersionId":s.current["versionId"],"serverRevision":s.draft_revision}})
    } else if path.ends_with("/draft/validate") {
        assert!(key.is_none());
        json!({"revisionId":DRAFT,"valid":!s.invalid_validation,"errors":[]})
    } else if path.ends_with("/draft/publish") {
        assert_eq!(body["expectedRevision"], s.draft_revision);
        assert_eq!(body["expectedVersionId"], s.current["versionId"]);
        assert_eq!(body["expectedMetadataHash"], s.current["metadataHash"]);
        let mut draft = s.draft.take().unwrap();
        let current_snapshot = &s.snapshots[s.current["versionId"].as_str().unwrap()];
        let same = draft.bytes == current_snapshot.bytes
            && ["name", "description", "detail", "coverUrl", "coverInputUrl"]
                .iter()
                .all(|k| draft.metadata[*k] == current_snapshot.metadata[*k]);
        if !same {
            let mut metadata = current_snapshot.metadata.clone();
            for key in ["name", "description", "detail", "coverUrl", "coverInputUrl"] {
                metadata[key] = draft.metadata[key].clone();
                s.current[key] = draft.metadata[key].clone();
            }
            draft.metadata = metadata;
            s.current["versionId"] = json!(V2);
            s.current["releaseId"] = json!(R2);
            s.current["version"] = json!("1.0.1");
            s.current["metadataHash"] = json!("hash-v2");
            s.snapshots.insert(V2.into(), draft);
        }
        if let Some(scope) = body.get("accessScope") {
            s.current["accessScope"] = scope.clone();
            s.current["metadataHash"] = json!(format!("hash-scope-{}", scope.as_str().unwrap()));
        }
        json!({"skill":s.current,"versionId":s.current["versionId"],"releaseId":s.current["releaseId"],"version":s.current["version"],"noOp":same})
    } else if path.ends_with("/source") {
        assert!(key.is_none());
        let historical = body.get("releaseId") == Some(&json!(R1));
        let version = if historical {
            V1
        } else {
            s.current["versionId"].as_str().unwrap()
        };
        let snapshot = &s.snapshots[version];
        json!({"skill":s.current,"sourceMetadata":snapshot.metadata,"versionId":version,"releaseId":if historical{json!(R1)}else{s.current["releaseId"].clone()},"version":if historical{"1.0.0"}else{s.current["version"].as_str().unwrap()},"metadataHash":s.current["metadataHash"],"packageChecksum":if s.bad_checksum{"bad".into()}else{tool_project::digest(&snapshot.bytes)},"packageBase64":STANDARD.encode(&snapshot.bytes),"packageFileName":"package.zip"})
    } else {
        panic!("unexpected route {path}")
    };
    if let Some(key) = key {
        s.replies.insert(key, result.clone());
    }
    if s.lost.remove(&path) {
        return (StatusCode::BAD_GATEWAY,Json(json!({"detail":{"code":"LOST_RESPONSE","message":"simulated accepted write with lost response"}}))).into_response();
    }
    Json(result).into_response()
}
async fn server() -> (
    Client,
    tool_publish::AuthoringIdentity,
    Shared,
    tokio::task::JoinHandle<()>,
) {
    let state = Arc::new(Mutex::new(Server::default()));
    let app = Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) }))
        .fallback(post(endpoint))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        Client::new(&backend, "virse_sk_test_authoring").unwrap(),
        tool_publish::AuthoringIdentity::new(&backend, id(USER), id(ORG)).unwrap(),
        state,
        task,
    )
}
fn lose(state: &Shared, path: &str) {
    state.lock().unwrap().lost.insert(path.into());
}
async fn first(
    client: &Client,
    identity: &tool_publish::AuthoringIdentity,
    project: &Path,
    root: &Path,
    scope: Option<&str>,
) -> Value {
    tool_publish::publish(client, project, root, identity, scope, false)
        .await
        .unwrap()
}
#[tokio::test]
async fn create_lost_response_frozen_resume_no_duplicate_and_noop() {
    let (client, identity, state, task) = server().await;
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("tool");
    let store = temp.path().join("state");
    initialized(&project);
    lose(&state, "/skills/creator/create");
    let error = tool_publish::publish(&client, &project, &store, &identity, None, false)
        .await
        .unwrap_err();
    assert_eq!(error.code, "TOOL_PUBLISH_UNCERTAIN");
    assert!(error.idempotency_key.is_some());
    change(&project, "Changed after upload");
    assert_eq!(
        tool_publish::publish(&client, &project, &store, &identity, None, false)
            .await
            .unwrap_err()
            .code,
        "TOOL_PUBLISH_PENDING"
    );
    let result = tool_publish::publish(&client, &project, &store, &identity, None, true)
        .await
        .unwrap();
    assert_eq!(result["skill"]["name"], "Example");
    let s = state.lock().unwrap();
    let creates: Vec<_> = s
        .calls
        .iter()
        .filter(|c| c.0.ends_with("/creator/create"))
        .collect();
    assert_eq!(creates.len(), 2);
    assert_eq!(creates[0].1, creates[1].1);
    drop(s);
    task.abort();
}
#[tokio::test]
async fn every_write_phase_can_resume_with_its_original_key() {
    for failing in ["/skills/creator/finalize"] {
        let (client, identity, state, task) = server().await;
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("tool");
        let root = temp.path().join("state");
        initialized(&project);
        lose(&state, failing);
        assert_eq!(
            tool_publish::publish(&client, &project, &root, &identity, Some("team"), false)
                .await
                .unwrap_err()
                .code,
            "TOOL_PUBLISH_UNCERTAIN"
        );
        let result = first(&client, &identity, &project, &root, Some("team")).await;
        assert_eq!(result["skill"]["accessScope"], "team");
        let before = state.lock().unwrap().calls.len();
        first(&client, &identity, &project, &root, None).await;
        assert!(state.lock().unwrap().calls.len() > before);
        let s = state.lock().unwrap();
        let writes: Vec<_> = s.calls.iter().filter(|c| c.0 == failing).collect();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].1, writes[1].1);
        drop(s);
        task.abort();
    }
}
#[tokio::test]
async fn draft_and_publish_resume_scope_retained_and_explicit_change() {
    for phase in ["draft", "draft/publish"] {
        let (client, identity, state, task) = server().await;
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("tool");
        let root = temp.path().join("state");
        initialized(&project);
        first(&client, &identity, &project, &root, Some("team")).await;
        change(&project, "Changed");
        let path = format!("/skills/{SKILL}/{phase}");
        lose(&state, &path);
        assert_eq!(
            tool_publish::publish(&client, &project, &root, &identity, None, false)
                .await
                .unwrap_err()
                .code,
            "TOOL_PUBLISH_UNCERTAIN"
        );
        let result = first(&client, &identity, &project, &root, None).await;
        assert_eq!(result["skill"]["accessScope"], "team");
        assert_eq!(result["version"], "1.0.1");
        let calls = state.lock().unwrap().calls.clone();
        let repeats: Vec<_> = calls.iter().filter(|c| c.0 == path).collect();
        assert_eq!(repeats.len(), 2);
        assert_eq!(repeats[0].1, repeats[1].1);
        task.abort();
    }
    let (client, identity, state, task) = server().await;
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("tool");
    let root = temp.path().join("state");
    initialized(&project);
    first(&client, &identity, &project, &root, None).await;
    change(&project, "Scope changed");
    lose(&state, &format!("/skills/{SKILL}/draft/publish"));
    assert_eq!(
        tool_publish::publish(&client, &project, &root, &identity, Some("team"), false)
            .await
            .unwrap_err()
            .code,
        "TOOL_PUBLISH_UNCERTAIN"
    );
    assert_eq!(
        first(&client, &identity, &project, &root, Some("team")).await["skill"]["accessScope"],
        "team"
    );
    assert_eq!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c.0.ends_with("/draft/publish"))
            .count(),
        2
    );
    task.abort();
}
#[tokio::test]
async fn pull_history_restores_source_metadata_but_uses_live_cas_base() {
    let (client, identity, state, task) = server().await;
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("tool");
    let root = temp.path().join("state");
    initialized(&project);
    first(&client, &identity, &project, &root, Some("team")).await;
    change(&project, "Latest Name");
    first(&client, &identity, &project, &root, None).await;
    let output = temp.path().join("old");
    let pulled = tool_publish::pull(&client, id(SKILL), &output, Some(id(R1)), &root, &identity)
        .await
        .unwrap();
    assert_eq!(pulled["sourceVersionId"], V1);
    assert_eq!(pulled["baseVersionId"], V2);
    let meta: Value = serde_json::from_slice(&fs::read(output.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["name"], "Example");
    let result = first(&client, &identity, &output, &root, None).await;
    assert_eq!(result["skill"]["name"], "Example");
    assert_eq!(result["skill"]["accessScope"], "team");
    assert_eq!(
        tool_publish::pull(&client, id(SKILL), &output, None, &root, &identity)
            .await
            .unwrap_err()
            .code,
        "DESTINATION_EXISTS"
    );
    state.lock().unwrap().bad_checksum = true;
    let bad = temp.path().join("corrupt");
    assert_eq!(
        tool_publish::pull(&client, id(SKILL), &bad, None, &root, &identity)
            .await
            .unwrap_err()
            .code,
        "PACKAGE_CHECKSUM_MISMATCH"
    );
    assert!(!bad.exists());
    task.abort();
}
#[tokio::test]
async fn cas_conflict_does_not_publish_and_invalid_draft_stays_unpublished() {
    let (client, identity, state, task) = server().await;
    let temp = tempfile::tempdir().unwrap();
    let p = temp.path().join("tool");
    let root = temp.path().join("state");
    initialized(&p);
    first(&client, &identity, &p, &root, None).await;
    change(&p, "Upgrade");
    state.lock().unwrap().cas_conflict = true;
    assert_eq!(
        tool_publish::publish(&client, &p, &root, &identity, None, false)
            .await
            .unwrap_err()
            .code,
        "SKILL_VERSION_CONFLICT"
    );
    assert!(state.lock().unwrap().draft.is_none());
    state.lock().unwrap().cas_conflict = false;
    state.lock().unwrap().invalid_validation = true;
    assert_eq!(
        tool_publish::publish(&client, &p, &root, &identity, None, false)
            .await
            .unwrap_err()
            .code,
        "TOOL_REMOTE_VALIDATION_FAILED"
    );
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c.0.ends_with("/draft/publish"))
    );
    task.abort();
}
#[tokio::test]
async fn receipts_isolate_backend_account_org_and_detect_staged_tampering() {
    let (client, identity, state, task) = server().await;
    let temp = tempfile::tempdir().unwrap();
    let p = temp.path().join("tool");
    let root = temp.path().join("state");
    initialized(&p);
    lose(&state, "/skills/creator/create");
    tool_publish::publish(&client, &p, &root, &identity, None, false)
        .await
        .unwrap_err();
    let zip = fs::read_dir(root.join("tool-authoring"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "zip"))
        .unwrap();
    fs::write(zip, "changed").unwrap();
    assert_eq!(
        tool_publish::publish(&client, &p, &root, &identity, None, true)
            .await
            .unwrap_err()
            .code,
        "TOOL_RECEIPT_ERROR"
    );
    for changed in [
        tool_publish::AuthoringIdentity::new(&identity.backend, id(USER), Uuid::new_v4()).unwrap(),
        tool_publish::AuthoringIdentity::new(&identity.backend, Uuid::new_v4(), id(ORG)).unwrap(),
        tool_publish::AuthoringIdentity::new("https://different.example", id(USER), id(ORG))
            .unwrap(),
    ] {
        assert_eq!(
            tool_publish::publish(&client, &p, &root, &changed, None, true)
                .await
                .unwrap_err()
                .code,
            "TOOL_PUBLISH_NOT_PENDING"
        );
    }
    task.abort();
}
#[tokio::test]
async fn sdk_rejects_protected_multipart_fields_before_network() {
    let client = Client::new("http://127.0.0.1:1", "virse_sk_test_authoring").unwrap();
    for key in [
        "organization_id",
        "skill_id",
        "package_source",
        "expected_version_id",
        "unknown",
    ] {
        let metadata = json!({key:"evil"});
        let error = client
            .tool_create_package(id(ORG), vec![1], &metadata, "key")
            .await
            .unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT");
    }
}

#[tokio::test]
async fn metadata_only_updates_are_not_cached_and_unsupported_edits_fail_before_write() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    let root = tmp.path().join("state");
    initialized(&p);
    first(&client, &identity, &p, &root, None).await;
    let before = tool_project::pack(&p).unwrap();
    change(&p, "Metadata Only");
    let after = tool_project::pack(&p).unwrap();
    assert_eq!(before.bytes, after.bytes);
    assert_ne!(before.fingerprint(), after.fingerprint());
    assert_eq!(
        first(&client, &identity, &p, &root, None).await["skill"]["name"],
        "Metadata Only"
    );
    let mut meta: Value = serde_json::from_slice(&fs::read(p.join("meta.json")).unwrap()).unwrap();
    meta["tags"] = json!(["unsupported-new-tag"]);
    fs::write(p.join("meta.json"), meta.to_string()).unwrap();
    let before = state.lock().unwrap().calls.len();
    assert_eq!(
        tool_publish::publish(&client, &p, &root, &identity, None, false)
            .await
            .unwrap_err()
            .code,
        "TOOL_METADATA_UPDATE_UNSUPPORTED"
    );
    assert_eq!(state.lock().unwrap().calls.len(), before);
    task.abort();
}
#[tokio::test]
async fn unchanged_pull_publish_is_server_noop_and_external_cas_is_checked() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    let root = tmp.path().join("state");
    initialized(&p);
    let first_result = first(&client, &identity, &p, &root, None).await;
    let output = tmp.path().join("pulled");
    tool_publish::pull(&client, id(SKILL), &output, None, &root, &identity)
        .await
        .unwrap();
    assert_eq!(
        tool_project::pack(&p).unwrap().bytes,
        tool_project::pack(&output).unwrap().bytes
    );
    let result = first(&client, &identity, &output, &root, None).await;
    assert_eq!(result["releaseId"], first_result["releaseId"]);
    assert_eq!(result["noOp"], true);
    state.lock().unwrap().cas_conflict = true;
    assert_eq!(
        tool_publish::publish(&client, &output, &root, &identity, None, false)
            .await
            .unwrap_err()
            .code,
        "SKILL_VERSION_CONFLICT"
    );
    task.abort();
}
#[tokio::test]
async fn denied_team_finalize_can_be_corrected_without_creating_a_second_tool() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    let root = tmp.path().join("state");
    initialized(&p);
    state.lock().unwrap().deny_team = true;
    let error = tool_publish::publish(&client, &p, &root, &identity, Some("team"), false)
        .await
        .unwrap_err();
    assert_eq!(error.code, "TEAM_REQUIRED");
    assert_eq!(error.details.unwrap()["skillId"], SKILL);
    let result = first(&client, &identity, &p, &root, Some("personal")).await;
    assert_eq!(result["skill"]["accessScope"], "personal");
    let s = state.lock().unwrap();
    assert_eq!(
        s.calls
            .iter()
            .filter(|c| c.0.ends_with("/creator/create"))
            .count(),
        1
    );
    assert!(!s.calls.iter().any(|c| c.0.ends_with("/creator/replace")));
    drop(s);
    task.abort();
}
#[tokio::test]
async fn rejected_draft_validation_reuses_revision_for_corrected_package() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    let root = tmp.path().join("state");
    initialized(&p);
    first(&client, &identity, &p, &root, None).await;
    change(&p, "Invalid revision");
    state.lock().unwrap().invalid_validation = true;
    let error = tool_publish::publish(&client, &p, &root, &identity, None, false)
        .await
        .unwrap_err();
    assert_eq!(error.code, "TOOL_REMOTE_VALIDATION_FAILED");
    assert_eq!(error.details.unwrap()["revisionId"], DRAFT);
    change(&p, "Corrected revision");
    state.lock().unwrap().invalid_validation = false;
    assert_eq!(
        first(&client, &identity, &p, &root, None).await["skill"]["name"],
        "Corrected revision"
    );
    assert_eq!(state.lock().unwrap().draft_revision, 2);
    task.abort();
}
#[tokio::test]
async fn atomic_scope_replay_does_not_overwrite_a_later_remote_change() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    let root = tmp.path().join("state");
    initialized(&p);
    lose(&state, "/skills/creator/finalize");
    assert_eq!(
        tool_publish::publish(&client, &p, &root, &identity, Some("team"), false)
            .await
            .unwrap_err()
            .code,
        "TOOL_PUBLISH_UNCERTAIN"
    );
    {
        let mut s = state.lock().unwrap();
        assert_eq!(s.current["accessScope"], "team");
        s.current["accessScope"] = json!("personal");
        s.current["metadataHash"] = json!("newer-remote-change");
    }
    // Same durable publish key returns its original receipt. No scope assignment
    // endpoint is called, and subsequent new writes use the original pinned CAS.
    first(&client, &identity, &p, &root, Some("team")).await;
    assert_eq!(state.lock().unwrap().current["accessScope"], "personal");
    assert!(
        !state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c.0.ends_with("/creator/update"))
    );
    task.abort();
}
#[tokio::test]
async fn pull_unpublished_draft_replaces_it_instead_of_creating_another_tool() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    let root = tmp.path().join("state");
    initialized(&p);
    state.lock().unwrap().deny_team = true;
    tool_publish::publish(&client, &p, &root, &identity, Some("team"), false)
        .await
        .unwrap_err();
    let output = tmp.path().join("draft");
    tool_publish::pull(&client, id(SKILL), &output, None, &root, &identity)
        .await
        .unwrap();
    change(&output, "Edited draft");
    let result = first(&client, &identity, &output, &root, None).await;
    assert_eq!(result["skill"]["name"], "Edited draft");
    let s = state.lock().unwrap();
    assert_eq!(
        s.calls
            .iter()
            .filter(|c| c.0.ends_with("/creator/create"))
            .count(),
        1
    );
    assert_eq!(
        s.calls
            .iter()
            .filter(|c| c.0.ends_with("/creator/replace"))
            .count(),
        1
    );
    drop(s);
    task.abort();
}

#[test]
fn offline_interface_checks_steps_outputs_and_controls() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    initialized(&p);
    for bad in [
        json!({"fields":[],"steps":[1]}),
        json!({"fields":[],"outputs":[1]}),
        json!({"fields":[],"outputs":[{"type":"image","count":false}]}),
        json!({"fields":[],"steps":[{"body":"{{undeclared}}"}]}),
        json!({"fields":[{"id":"value","type":"text","label":"\u{0000}"}]}),
        json!({"fields":[{"id":"value","type":"text"}],"promptTemplate":"${value}"}),
    ] {
        fs::write(p.join("interface.json"), bad.to_string()).unwrap();
        assert!(tool_project::validate(&p).is_err(), "{bad}");
    }
    fs::write(p.join("interface.json"),json!({"fields":[{"id":"pick","type":"select","options":[{"value":"x"}]}],"steps":[{"body":"Choose {{pick}}"}],"outputs":[{"type":"image","count":1}]}).to_string()).unwrap();
    assert!(tool_project::validate(&p).is_ok());
}
#[test]
fn pull_accepts_one_wrapper_directory_and_rejects_multiple_skills() {
    let bytes = archive(&[
        ("wrapper/SKILL.md", &skill_md()),
        ("wrapper/scripts/tool.py", b"print('not executed')"),
    ]);
    let files = tool_project::unpack(&bytes, &tool_project::digest(&bytes)).unwrap();
    assert!(files.contains_key("SKILL.md"));
    assert!(files.contains_key("scripts/tool.py"));
    let bytes = archive(&[("SKILL.md", &skill_md()), ("other/SKILL.md", &skill_md())]);
    assert!(tool_project::unpack(&bytes, &tool_project::digest(&bytes)).is_err());
}
#[tokio::test]
async fn incomplete_pull_binding_cannot_publish_an_unverified_directory() {
    let (client, identity, state, task) = server().await;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    let root = tmp.path().join("state");
    initialized(&p);
    first(&client, &identity, &p, &root, None).await;
    let output = tmp.path().join("pulled");
    tool_publish::pull(&client, id(SKILL), &output, None, &root, &identity)
        .await
        .unwrap();
    for entry in fs::read_dir(root.join("tool-authoring")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|s| s == "json") {
            let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            if state["directory"] == json!(output) {
                state["installation_pending"] = json!(true);
                fs::write(path, state.to_string()).unwrap();
            }
        }
    }
    let calls = state.lock().unwrap().calls.len();
    assert_eq!(
        tool_publish::publish(&client, &output, &root, &identity, None, false)
            .await
            .unwrap_err()
            .code,
        "TOOL_PULL_INCOMPLETE"
    );
    assert_eq!(state.lock().unwrap().calls.len(), calls);
    task.abort();
}

#[tokio::test]
async fn publishing_never_creates_receipts_inside_source_root() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tool");
    initialized(&p);
    let client = Client::new("http://127.0.0.1:1", "virse_sk_test_authoring").unwrap();
    let identity =
        tool_publish::AuthoringIdentity::new("http://127.0.0.1:1", id(USER), id(ORG)).unwrap();
    assert_eq!(
        tool_publish::publish(&client, &p, &p.join("state"), &identity, None, false)
            .await
            .unwrap_err()
            .code,
        "TOOL_RECEIPT_LOCATION"
    );
    assert!(!p.join("state").exists());
}
