use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Output, Stdio},
    sync::{Arc, Mutex},
};
use tokio::process::Command;
const ORG: &str = "00000000-0000-0000-0000-000000000001";
const OWNER_ORG: &str = "00000000-0000-0000-0000-000000000002";
const RESOURCE: &str = "00000000-0000-0000-0000-000000000003";
const USER: &str = "00000000-0000-0000-0000-000000000004";
const RULE: &str = "00000000-0000-0000-0000-000000000005";
const INHERITED: &str = "00000000-0000-0000-0000-000000000006";
const CANVAS: &str = "00000000-0000-0000-0000-000000000007";
#[derive(Default)]
struct Data {
    calls: Vec<(String, Value)>,
    writes: usize,
    readonly: bool,
    lost_write: bool,
    impact_failed: bool,
    refresh_failed: bool,
    wrong_resource: bool,
    member_max: Option<u8>,
    link_url: Option<String>,
    claimable: bool,
}
type Shared = Arc<Mutex<Data>>;
fn row(
    id: &str,
    kind: &str,
    subject_type: &str,
    subject: Value,
    role: u8,
    editable: (bool, u8),
    inherited: Value,
) -> Value {
    json!({"rule_id":id,"row_type":kind,"subject_type":subject_type,"subject_id":subject,"display":kind,"role":role,"removable":editable.0,"max_role":editable.1,"inherited_from":inherited})
}
fn settings(s: &Data) -> Value {
    json!({"status":1,"message":"Success","dentry_id":if s.wrong_resource{CANVAS}else{RESOURCE},"owner_org_id":OWNER_ORG,"link_url":s.link_url.as_deref().unwrap_or("/s/test-token"),"token":"test-token","my_access":if s.readonly{1}else{4},"is_creator":!s.readonly,"can_manage":!s.readonly,"external_sharing":true,"visibility_summary":"private","rules":[
        row(RULE,"member","member",json!(format!("{USER}@{ORG}")),2,(!s.readonly,s.member_max.unwrap_or(4)),Value::Null),
        row("link-id","link","link",Value::Null,0,(false,3),Value::Null),
        row("org-id","org_all","org",json!(OWNER_ORG),1,(false,3),Value::Null),
        row(INHERITED,"inherited","member",json!(format!("{USER}@{OWNER_ORG}")),3,(false,0),json!({"dentry_id":CANVAS,"name":"parent"}))
    ]})
}
fn record(s: &Shared, path: &str, headers: &HeaderMap, body: &Value) {
    assert_eq!(
        headers["authorization"],
        "Bearer virse_sk_permissions_fixture"
    );
    assert!(headers.get("idempotency-key").is_none());
    s.lock().unwrap().calls.push((path.into(), body.clone()));
}
async fn endpoint(
    State(s): State<Shared>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let path = uri.path();
    record(&s, path, &headers, &body);
    let mut data = s.lock().unwrap();
    let value = match path {
        "/api/v1/resources/query" => {
            json!({"request_id":"r","data":{"items":[{"id":RESOURCE,"parent_id":null,"name":"folder","kind":"folder"}],"next_cursor":null,"snapshot":"s","truncated":false}})
        }
        "/versa/share/settings" => {
            if data.refresh_failed && data.writes > 1 {
                return Json(json!({"status":0,"code":"not_found","message":"No longer visible"}))
                    .into_response();
            }
            settings(&data)
        }
        "/versa/share/impact" => {
            if data.impact_failed {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"detail":"preview unavailable"})),
                )
                    .into_response();
            }
            assert!(matches!(
                body["change"]["kind"].as_str(),
                Some("rule_set" | "rule_remove")
            ));
            assert!(body["change"].get("op").is_none());
            json!({"status":1,"gain_count":2,"lose_count":1,"retained":[],"retained_count":3,"link_role_before":0,"link_role_after":2})
        }
        "/versa/share/rule/set" | "/versa/share/rule/remove" | "/versa/share/settings/delete" => {
            data.writes += 1;
            if data.lost_write {
                return (StatusCode::OK, "truncated after commit").into_response();
            }
            match path {
                "/versa/share/rule/set" => settings(&data),
                "/versa/share/rule/remove" => json!({"status":1,"removed":1}),
                _ => json!({"status":1,"removed_grants":2,"link_role":0}),
            }
        }
        "/versa/resolve" | "/versa/canvas/resolve" => {
            if data.claimable {
                json!({"status":1,"claimable":true,"dentry_id":RESOURCE,"name":"folder","kind":"folder","invited_role":2,"invited_by":null})
            } else {
                json!({"status":1,"claimable":false,"area":"shared-with-me","dentry":{"id":RESOURCE,"name":"folder"},"access":2})
            }
        }
        "/versa/share/claim" => {
            data.writes += 1;
            json!({"status":1,"claimed":true,"area":"shared-with-me","dentry":{"id":RESOURCE},"access":2})
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Json(value).into_response()
}
async fn serve(s: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new().route("/client-compatibility", axum::routing::get(|| async { axum::Json(serde_json::json!({"minimum_contract_version":1,"require_web_version":false,"policy_id":"test-1"})) })).fallback(post(endpoint)).with_state(s);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, server)
}
async fn run(dir: &Path, base: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_virse"));
    cmd.args(["--org", ORG, "resource"])
        .args(args)
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", "virse_sk_permissions_fixture")
        .env("VIRSE_CONFIG_DIR", dir)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(std::time::Duration::from_secs(10), cmd.output())
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
async fn settings_and_all_audiences_use_selected_workspace_and_real_subject_contract() {
    let s = Shared::default();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let get = output(
        &run(
            dir.path(),
            &base,
            &["permissions", "get", "/folder", "--json"],
        )
        .await,
    );
    assert_eq!(get["owner_org_id"], OWNER_ORG);
    assert_eq!(get["rules"][3]["inherited_from"]["name"], "parent");
    for (audience, expected_subject, expected_id, role, number) in [
        (
            vec!["--member", USER],
            "member",
            Some(format!("{USER}@{ORG}")),
            "manage",
            4,
        ),
        (vec!["--group", USER], "group", Some(USER.into()), "edit", 3),
        (
            vec!["--email", " Person@Example.INVALID "],
            "email",
            Some("person@example.invalid".into()),
            "view",
            1,
        ),
        (vec!["--org-members"], "org", None, "use", 2),
        (vec!["--link"], "link", None, "none", 0),
    ] {
        let mut args = vec!["permissions", "set", "/folder"];
        args.extend(audience);
        args.extend(["--role", role, "--json"]);
        let result = output(&run(dir.path(), &base, &args).await);
        assert_eq!(result["applied"], true);
        assert_eq!(result["settings"]["owner_org_id"], OWNER_ORG);
        let s = s.lock().unwrap();
        let (path, body) = s.calls.last().unwrap();
        assert_eq!(path, "/versa/share/rule/set");
        assert_eq!(body["org_id"], ORG);
        assert_eq!(body["dentry_id"], RESOURCE);
        assert_eq!(body["subject_type"], expected_subject);
        assert_eq!(body["role"], number);
        assert_eq!(
            body.get("subject_id").and_then(Value::as_str),
            expected_id.as_deref()
        );
        let (path, preview) = &s.calls[s.calls.len() - 2];
        assert_eq!(path, "/versa/share/impact");
        assert_eq!(preview["change"]["role"], number);
    }
    assert_eq!(s.lock().unwrap().writes, 5);
    server.abort();
}
#[tokio::test]
async fn previews_never_write_and_preview_failure_blocks_permission_change() {
    let s = Shared::default();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec![
            "permissions",
            "set",
            RESOURCE,
            "--link",
            "--role",
            "edit",
            "--dry-run",
            "--json",
        ],
        vec![
            "permissions",
            "remove",
            RESOURCE,
            RULE,
            "--dry-run",
            "--json",
        ],
        vec!["permissions", "clear", RESOURCE, "--dry-run", "--json"],
    ] {
        assert_eq!(
            output(&run(dir.path(), &base, &args).await)["dry_run"],
            true
        );
    }
    assert_eq!(s.lock().unwrap().writes, 0);
    s.lock().unwrap().impact_failed = true;
    assert_eq!(
        run(
            dir.path(),
            &base,
            &[
                "permissions",
                "set",
                RESOURCE,
                "--link",
                "--role",
                "use",
                "--json"
            ]
        )
        .await
        .status
        .code(),
        Some(5)
    );
    assert_eq!(s.lock().unwrap().writes, 0);
    server.abort();
}
#[tokio::test]
async fn read_only_inherited_and_capped_rows_are_not_modified() {
    let s = Shared::default();
    s.lock().unwrap().readonly = true;
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let view = output(
        &run(
            dir.path(),
            &base,
            &["permissions", "get", RESOURCE, "--json"],
        )
        .await,
    );
    assert_eq!(view["can_manage"], false);
    assert_eq!(view["rules"].as_array().unwrap().len(), 4);
    assert_eq!(
        run(
            dir.path(),
            &base,
            &["permissions", "set", RESOURCE, "--link", "--role", "edit"]
        )
        .await
        .status
        .code(),
        Some(4)
    );
    s.lock().unwrap().readonly = false;
    assert_eq!(
        run(
            dir.path(),
            &base,
            &["permissions", "remove", RESOURCE, INHERITED]
        )
        .await
        .status
        .code(),
        Some(4)
    );
    s.lock().unwrap().member_max = Some(0);
    assert_eq!(
        run(
            dir.path(),
            &base,
            &[
                "permissions",
                "set",
                RESOURCE,
                "--member",
                USER,
                "--role",
                "view"
            ]
        )
        .await
        .status
        .code(),
        Some(4)
    );
    assert_eq!(s.lock().unwrap().writes, 0);
    server.abort();
}
#[tokio::test]
async fn clear_and_remove_do_not_claim_inherited_access_is_revoked() {
    let s = Shared::default();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let clear = output(
        &run(
            dir.path(),
            &base,
            &["permissions", "clear", RESOURCE, "--json"],
        )
        .await,
    );
    assert_eq!(clear["applied"], true);
    assert_eq!(clear["result"]["removed_grants"], 2);
    assert_eq!(clear["settings"]["rules"][3]["role"], 3);
    s.lock().unwrap().refresh_failed = true;
    let remove = output(
        &run(
            dir.path(),
            &base,
            &["permissions", "remove", RESOURCE, RULE, "--json"],
        )
        .await,
    );
    assert_eq!(remove["applied"], true);
    assert_eq!(remove["result"]["removed"], 1);
    assert!(remove["settings"].is_null());
    assert_eq!(remove["refresh_error"]["code"], "not_found");
    server.abort();
}
#[tokio::test]
async fn unknown_write_outcome_is_not_retried() {
    let s = Shared::default();
    s.lock().unwrap().lost_write = true;
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &base,
        &[
            "permissions",
            "set",
            RESOURCE,
            "--link",
            "--role",
            "use",
            "--json",
        ],
    )
    .await;
    assert_eq!(out.status.code(), Some(7));
    assert!(out.stdout.is_empty());
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["code"], "SHARE_MUTATION_UNCERTAIN");
    assert_eq!(error["error"]["details"]["resource_id"], RESOURCE);
    assert_eq!(s.lock().unwrap().writes, 1);
    server.abort();
}
#[tokio::test]
async fn links_preserve_backend_address_and_resolve_does_not_auto_claim() {
    let s = Shared::default();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let link = output(
        &run(
            dir.path(),
            &base,
            &[
                "share",
                "link",
                RESOURCE,
                "--web-origin",
                "https://web.example.test",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(link["link_url"], "https://web.example.test/s/test-token");
    s.lock().unwrap().link_url = Some(format!("/canvas/{CANVAS}?share=test-token"));
    let link = output(
        &run(
            dir.path(),
            &base,
            &[
                "share",
                "link",
                RESOURCE,
                "--web-origin",
                "https://web.example.test",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(
        link["link_url"],
        format!("https://web.example.test/canvas/{CANVAS}?share=test-token")
    );
    s.lock().unwrap().claimable = true;
    let url = format!("https://uncontacted.invalid/canvas/{CANVAS}?share=test-token");
    let resolved = output(&run(dir.path(), &base, &["share", "resolve", &url, "--json"]).await);
    assert_eq!(resolved["claimable"], true);
    {
        let s = s.lock().unwrap();
        let (path, body) = s.calls.last().unwrap();
        assert_eq!(path, "/versa/canvas/resolve");
        assert_eq!(body["canvas_space_id"], CANVAS);
        assert_eq!(body["token"], "test-token");
        assert_eq!(body["org_id"], ORG);
        assert_eq!(s.writes, 0);
    }
    let claimed = output(&run(dir.path(), &base, &["share", "claim", &url, "--json"]).await);
    assert_eq!(claimed["claimed"], true);
    assert_eq!(
        s.lock().unwrap().calls.last().unwrap().1,
        json!({"chosen_org_id":ORG,"token":"test-token"})
    );
    output(
        &run(
            dir.path(),
            &base,
            &["share", "claim", "--resource", RESOURCE, "--json"],
        )
        .await,
    );
    assert_eq!(
        s.lock().unwrap().calls.last().unwrap().1,
        json!({"chosen_org_id":ORG,"dentry_id":RESOURCE})
    );
    s.lock().unwrap().claimable = false;
    let resolved = output(
        &run(
            dir.path(),
            &base,
            &["share", "resolve", "/s/test-token", "--json"],
        )
        .await,
    );
    assert_eq!(resolved["claimable"], false);
    server.abort();
}
#[tokio::test]
async fn invalid_roles_audiences_and_addresses_never_write() {
    let s = Shared::default();
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec![
            "permissions",
            "set",
            RESOURCE,
            "--group",
            USER,
            "--role",
            "manage",
        ],
        vec![
            "permissions",
            "set",
            RESOURCE,
            "--member",
            USER,
            "--role",
            "none",
        ],
        vec![
            "permissions",
            "set",
            RESOURCE,
            "--link",
            "--org-members",
            "--role",
            "view",
        ],
        vec!["permissions", "set", RESOURCE, "--role", "view"],
        vec!["share", "claim"],
        vec!["share", "claim", "token", "--resource", RESOURCE],
        vec!["share", "resolve", "https://evil.invalid/unknown/path"],
        vec!["share", "resolve", "javascript:alert(1)"],
        vec!["share", "resolve", "//evil.invalid/s/token"],
    ] {
        assert_eq!(run(dir.path(), &base, &args).await.status.code(), Some(2));
    }
    assert_eq!(s.lock().unwrap().writes, 0);
    server.abort();
}
#[tokio::test]
async fn settings_must_match_requested_resource() {
    let s = Shared::default();
    s.lock().unwrap().wrong_resource = true;
    let (base, server) = serve(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    assert!(
        !run(
            dir.path(),
            &base,
            &["permissions", "set", RESOURCE, "--link", "--role", "view"]
        )
        .await
        .status
        .success()
    );
    assert_eq!(s.lock().unwrap().writes, 0);
    server.abort();
}
