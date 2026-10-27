use axum::{
    Json, Router,
    extract::{Path as ApiPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Output, Stdio},
    sync::{Arc, Mutex},
};
use tokio::process::Command;
const ORG: &str = "00000000-0000-0000-0000-000000000001";
const FOLDER: &str = "00000000-0000-0000-0000-000000000002";
const FILE: &str = "00000000-0000-0000-0000-000000000003";
const ASSET: &str = "00000000-0000-0000-0000-000000000004";
const OTHER: &str = "00000000-0000-0000-0000-000000000005";
const USER: &str = "00000000-0000-0000-0000-000000000006";
fn item(id: &str, parent: Option<&str>, name: &str, kind: &str) -> Value {
    json!({"id":id,"parent_id":parent,"name":name,"kind":kind,"asset_id":if kind=="asset_image"{Some(ASSET)}else{None},"capabilities":{"read":true,"download":true}})
}
fn detail(resource: Value) -> Value {
    json!({"resource":resource,"kind":"asset_image","asset":{"id":ASSET},"detail":{}})
}
#[derive(Default)]
struct StateData {
    queries: Vec<Value>,
    mutations: Vec<(String, Value)>,
    headers: Vec<String>,
    renamed: bool,
    ambiguous: bool,
    reject_move: bool,
    lost_response: bool,
    create_count: usize,
    uncertain_create: bool,
    partial_delete: bool,
}
type Shared = Arc<Mutex<StateData>>;
async fn query(State(shared): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let mut state = shared.lock().unwrap();
    state.queries.push(body.clone());
    assert_eq!(body["context"], json!({"org_id":ORG}));
    let folder = item(
        FOLDER,
        None,
        if state.renamed { "renamed" } else { "素材" },
        "folder",
    );
    let file = item(FILE, Some(FOLDER), "产品 图.jpg", "asset_image");
    let (items, cursor) = if body["recursive"] == true {
        (vec![folder, file], None)
    } else if body["parent_id"] == FOLDER {
        (vec![file], None)
    } else if state.ambiguous {
        if body["cursor"].is_null() {
            (vec![folder], Some("page2"))
        } else {
            (vec![item(OTHER, None, "素材", "folder")], None)
        }
    } else {
        (vec![folder], None)
    };
    Json(
        json!({"request_id":"read","data":{"items":items,"next_cursor":cursor,"snapshot":"snapshot","truncated":false}}),
    )
}
async fn resource(ApiPath(id): ApiPath<String>, State(shared): State<Shared>) -> Json<Value> {
    let state = shared.lock().unwrap();
    let value = if id == FOLDER {
        item(
            FOLDER,
            None,
            if state.renamed { "renamed" } else { "素材" },
            "folder",
        )
    } else {
        item(&id, Some(FOLDER), "产品 图.jpg", "asset_image")
    };
    Json(json!({"request_id":"read","data":detail(value)}))
}
async fn batch(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let mut state = shared.lock().unwrap();
    state
        .headers
        .push(headers["x-virse-expected-user"].to_str().unwrap().into());
    state.mutations.push((
        headers["idempotency-key"].to_str().unwrap().into(),
        body.clone(),
    ));
    assert_eq!(body["context"], json!({"org_id":ORG}));
    if state.reject_move && body["op"] == "move" && body["confirm_widen"] == false {
        return Json(json!({"request_id":"write","data":{"items":[{"id":FILE,"outcome":"failed","error":{"code":"visibility_widened","message":"Access becomes wider","details":{"before":"private","after":"shared"}},"data":null}]}})).into_response();
    }
    if body["op"] == "rename" {
        state.renamed = true;
    }
    if state.lost_response && state.mutations.len() == 1 {
        return (StatusCode::OK, "lost response").into_response();
    }
    if state.partial_delete && body["op"] == "trash" {
        return Json(json!({"request_id":"write","data":{"items":[
            {"id":FILE,"outcome":"succeeded","data":{},"error":null},
            {"id":OTHER,"outcome":"failed","error":{"code":"forbidden","message":"No permission","retryable":true}}
        ]}})).into_response();
    }
    let items: Vec<_> = body["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| json!({"id":id,"outcome":"succeeded","error":null,"data":{"ok":true}}))
        .collect();
    Json(json!({"request_id":"write","data":{"items":items}})).into_response()
}
async fn serve(state: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let app=Router::new()
        .route("/auth/me",get(||async{Json(json!({"status":1,"id":USER}))}))
        .route("/api/v1/resources/query",post(query))
        .route("/api/v1/resources/{id}",get(resource))
        .route("/api/v1/resources/batch",post(batch))
        .route("/versa/folder/create",post(|State(s):State<Shared>,Json(body):Json<Value>|async move {
            let mut state=s.lock().unwrap();
            state.create_count+=1;
            assert_eq!(body["parent_id"],FOLDER);
            if state.uncertain_create { return (StatusCode::OK,"truncated").into_response(); }
            Json(json!({"status":1,"dentry":item(OTHER,Some(FOLDER),body["name"].as_str().unwrap(),"folder")})).into_response()
        }))
        .route("/versa/trash/list",post(|Json(body):Json<Value>|async move {
            assert_eq!(body["org_id"],ORG);
            Json(json!({"status":1,"items":[item(FILE,Some(FOLDER),"产品 图.jpg","asset_image")]}))
        }))
        .route(&format!("/api/v1/assets/{ASSET}/download"),get(|Query(q):Query<std::collections::HashMap<String,String>>|async move {
            assert_eq!(q.get("org_id").unwrap(),ORG);assert_eq!(q.get("dentry_id").unwrap(),FILE);assert!(!q.contains_key("canvas_space_id"));
            Json(json!({"request_id":"download","data":{"url":"/file","filename":"test.jpg","delivery":"authenticated_stream"}}))
        }))
        .route("/file",get(||async{"resource bytes"})).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, handle)
}
async fn run(root: &Path, base: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_virse"));
    cmd.args(["--org", ORG, "resource"])
        .args(args)
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", "virse_sk_mock_resource")
        .env("VIRSE_CONFIG_DIR", root)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output())
        .await
        .unwrap()
        .unwrap()
}
fn output(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[tokio::test]
async fn paths_tree_search_and_download_use_resource_context() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let list = output(&run(dir.path(), &base, &["ls", "/素材", "--json"]).await);
    assert_eq!(list["items"][0]["id"], FILE);
    let tree = output(&run(dir.path(), &base, &["tree", "/", "--json"]).await);
    assert_eq!(tree["nodes"][0]["resource"]["id"], FOLDER);
    assert_eq!(tree["nodes"][0]["children"][0]["resource"]["id"], FILE);
    assert_eq!(tree["incomplete"], false);
    let human = run(dir.path(), &base, &["tree", "/"]).await;
    assert!(String::from_utf8_lossy(&human.stdout).contains("└── 素材/"));
    let shallow = output(&run(dir.path(), &base, &["tree", "/", "--depth", "1", "--json"]).await);
    assert_eq!(shallow["incomplete"], true);
    assert_eq!(shallow["nodes"][0]["collapsed"], true);
    let bounded = output(
        &run(
            dir.path(),
            &base,
            &["tree", "/", "--max-items", "1", "--json"],
        )
        .await,
    );
    assert_eq!(bounded["incomplete"], true);
    output(
        &run(
            dir.path(),
            &base,
            &["search", "产品", "/素材", "--kind", "image", "--json"],
        )
        .await,
    );
    {
        let s = state.lock().unwrap();
        let q = s.queries.last().unwrap();
        assert_eq!(q["query"], "产品");
        assert_eq!(q["recursive"], true);
        assert_eq!(q["kinds"], json!(["asset_image"]));
    }
    let get = output(&run(dir.path(), &base, &["get", "/素材/产品 图.jpg", "--json"]).await);
    assert_eq!(get["resource"]["id"], FILE);
    let path = dir.path().join("picture.jpg");
    output(
        &run(
            dir.path(),
            &base,
            &[
                "download",
                "/素材/产品 图.jpg",
                "--output",
                path.to_str().unwrap(),
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(std::fs::read(path).unwrap(), b"resource bytes");
    server.abort();
}
#[tokio::test]
async fn duplicate_names_across_pages_are_never_guessed() {
    let state = Shared::default();
    state.lock().unwrap().ambiguous = true;
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &base, &["delete", "/素材", "--json"]).await;
    assert!(!out.status.success());
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["code"], "AMBIGUOUS_PATH");
    assert_eq!(
        error["error"]["details"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(state.lock().unwrap().mutations.is_empty());
    let out = run(dir.path(), &base, &["get", "/素材"]).await;
    assert!(String::from_utf8_lossy(&out.stderr).contains(OTHER));
    output(&run(dir.path(), &base, &["get", &format!("@{FOLDER}"), "--json"]).await);
    server.abort();
}
#[tokio::test]
async fn rename_replay_uses_frozen_ids_after_name_changes() {
    let state = Shared::default();
    state.lock().unwrap().lost_response = true;
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "rename",
        "/素材",
        "renamed",
        "--idempotency-key",
        "rename-key",
        "--json",
    ];
    output(&run(dir.path(), &base, &args).await);
    let reads = state.lock().unwrap().queries.len();
    output(&run(dir.path(), &base, &args).await);
    {
        let s = state.lock().unwrap();
        assert_eq!(s.queries.len(), reads);
        assert_eq!(s.mutations.len(), 3);
        assert!(s.mutations.windows(2).all(|a| a[0] == a[1]));
        assert!(s.headers.iter().all(|u| u == USER));
        assert_eq!(s.mutations[0].1["ids"], json!([FOLDER]));
    }
    let conflict = run(
        dir.path(),
        &base,
        &[
            "rename",
            "/素材",
            "another",
            "--idempotency-key",
            "rename-key",
            "--json",
        ],
    )
    .await;
    assert_eq!(conflict.status.code(), Some(2));
    assert_eq!(state.lock().unwrap().mutations.len(), 3);
    server.abort();
}
#[tokio::test]
async fn move_confirmation_is_explicit_and_delete_restore_share_batch_protocol() {
    let state = Shared::default();
    state.lock().unwrap().reject_move = true;
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let refused = run(
        dir.path(),
        &base,
        &["move", "/素材/产品 图.jpg", "/", "--json"],
    )
    .await;
    assert_eq!(refused.status.code(), Some(6));
    let data: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(data["items"][0]["error"]["details"]["after"], "shared");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--confirm-access-change"));
    output(
        &run(
            dir.path(),
            &base,
            &[
                "move",
                "/素材/产品 图.jpg",
                "/",
                "--confirm-access-change",
                "--json",
            ],
        )
        .await,
    );
    {
        let s = state.lock().unwrap();
        assert_eq!(s.mutations.len(), 2);
        assert_eq!(s.mutations[1].1["confirm_widen"], true);
        assert_eq!(s.mutations[1].1["confirm_narrow"], true);
        assert_ne!(s.mutations[0].0, s.mutations[1].0);
    }
    output(
        &run(
            dir.path(),
            &base,
            &["copy", "/素材/产品 图.jpg", "/", "--json"],
        )
        .await,
    );
    output(
        &run(
            dir.path(),
            &base,
            &[
                "delete",
                &format!("@{FILE}"),
                &format!("@{OTHER}"),
                "--json",
            ],
        )
        .await,
    );
    output(&run(dir.path(), &base, &["trash", "--json"]).await);
    output(
        &run(
            dir.path(),
            &base,
            &["restore", &format!("@{FILE}"), "--json"],
        )
        .await,
    );
    {
        let s = state.lock().unwrap();
        assert_eq!(s.mutations[2].1["op"], "copy");
        assert_eq!(s.mutations[3].1["op"], "trash");
        assert_eq!(s.mutations[3].1["ids"].as_array().unwrap().len(), 2);
        assert_eq!(s.mutations[4].1["op"], "restore");
    }
    server.abort();
}
#[tokio::test]
async fn mkdir_and_invalid_paths_do_not_invent_containers() {
    let state = Shared::default();
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let folder = output(&run(dir.path(), &base, &["mkdir", "/素材/new folder", "--json"]).await);
    assert_eq!(folder["id"], OTHER);
    for args in [
        vec!["mkdir", "/素材"],
        vec!["mkdir", "/素材/产品 图.jpg/child"],
        vec!["delete", "/"],
        vec!["get", "relative/path"],
        vec!["get", "/素材/../x"],
    ] {
        assert!(!run(dir.path(), &base, &args).await.status.success());
    }
    assert_eq!(state.lock().unwrap().create_count, 1);
    assert!(state.lock().unwrap().mutations.is_empty());
    server.abort();
}

#[tokio::test]
async fn incomplete_batch_is_reported_without_automatically_repeating_successes() {
    let state = Shared::default();
    state.lock().unwrap().partial_delete = true;
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &base,
        &[
            "delete",
            &format!("@{FILE}"),
            &format!("@{OTHER}"),
            "--json",
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(6));
    let data: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(data["items"][0]["outcome"], "succeeded");
    assert_eq!(data["items"][1]["error"]["code"], "forbidden");
    assert_eq!(state.lock().unwrap().mutations.len(), 1);
    server.abort();
}
#[tokio::test]
async fn uncertain_folder_creation_is_not_blindly_retried() {
    let state = Shared::default();
    state.lock().unwrap().uncertain_create = true;
    let (base, server) = serve(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &base,
        &[
            "mkdir",
            "new folder",
            "--parent",
            &format!("@{FOLDER}"),
            "--json",
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(7));
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["code"], "FOLDER_CREATION_UNCERTAIN");
    assert_eq!(state.lock().unwrap().create_count, 1);
    server.abort();
}

#[tokio::test]
async fn virtual_root_still_checks_organization_permission() {
    let app=Router::new().route("/api/v1/resources/query",post(||async {
        (StatusCode::FORBIDDEN,Json(json!({"request_id":"denied","error":{"code":"forbidden","message":"Not a member","retryable":false}})))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &base, &["get", "/", "--json"]).await;
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stdout.is_empty());
    server.abort();
}
