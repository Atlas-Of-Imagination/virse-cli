#![allow(dead_code)]
use axum::{Json, Router, routing::get};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::process::Command;
pub const ORG: &str = "00000000-0000-0000-0000-000000000001";
pub const USER: &str = "00000000-0000-0000-0000-000000000002";
pub const CANVAS: &str = "00000000-0000-0000-0000-000000000003";
pub const ONE: &str = "00000000-0000-0000-0000-000000000004";
pub const TWO: &str = "00000000-0000-0000-0000-000000000005";
pub const GROUP: &str = "00000000-0000-0000-0000-000000000006";
pub const ASSET: &str = "00000000-0000-0000-0000-000000000007";
pub const DEST: &str = "00000000-0000-0000-0000-000000000008";
pub fn envelope(value: Value) -> Json<Value> {
    Json(json!({"request_id":"test","data":value}))
}
pub async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = app.route(
        "/auth/me",
        get(|| async { Json(json!({"status":1,"id":USER})) }),
    );
    (
        base,
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    )
}
pub fn command(root: &Path, base: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_virse"));
    cmd.args(["--json", "--org", ORG, "--canvas", CANVAS])
        .args(args)
        .env("VIRSE_BACKEND_URL", base)
        .env("VIRSE_API_TOKEN", "virse_sk_workflow_test")
        .env("VIRSE_CONFIG_DIR", root.join("state"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}
pub async fn run(root: &Path, base: &str, args: &[&str]) -> Output {
    tokio::time::timeout(Duration::from_secs(25), command(root, base, args).output())
        .await
        .unwrap()
        .unwrap()
}
pub fn value(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    json_output(out)
}
pub fn json_output(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}
pub fn error(out: &Output) -> Value {
    assert!(!out.status.success());
    serde_json::from_str(String::from_utf8_lossy(&out.stderr).lines().last().unwrap()).unwrap()
}
pub fn item(id: &str, name: &str, kind: &str, parent: Option<&str>) -> Value {
    json!({"id":id,"name":name,"kind":kind,"parent_id":parent,"capabilities":{}})
}
