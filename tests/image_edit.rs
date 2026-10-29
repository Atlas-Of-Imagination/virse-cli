mod support;
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nmock-png-for-protocol-test";
#[derive(Default)]
struct Data {
    base: String,
    uploads: usize,
    reads: usize,
    downloads: usize,
    submits: Vec<(String, Value)>,
    changed: bool,
    no_inpaint: bool,
    bad_upload: bool,
    rejected_upload: bool,
    lose_submit: bool,
    mask_denied: bool,
}
type Shared = Arc<Mutex<Data>>;
async fn server(s: Shared) -> (String, tokio::task::JoinHandle<()>) {
    let router = Router::new()
        .route("/generation/models/catalog", post(|State(s):State<Shared>| async move {
            let scenarios = if s.lock().unwrap().no_inpaint {json!(["image-to-image"])} else {json!(["image-to-image","inpaint"])};
            Json(json!({"schema_version":2,"catalog_version":"test","models":[{"model_key":"edit-model","mode":"image","display_name":"Edit","supported_scenarios":scenarios}]}))
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/elements/{ONE}"),get(|State(s):State<Shared>|async move {
            let mut s=s.lock().unwrap();s.reads+=1;
            Json(json!({"element":{"id":ONE,"type":"image","content_ref":{"kind":"asset","id":if s.changed {DEST}else{ASSET}},"layout":{"position":{"x":10,"y":20},"size":{"width":300,"height":200}}}}))
        }))
        .route("/upload_images",post(|State(s):State<Shared>,headers:HeaderMap,bytes:Bytes|async move {
            assert_eq!(headers["authorization"],"Bearer virse_sk_workflow_test");
            let body=String::from_utf8_lossy(&bytes);
            assert!(body.contains("name=\"user_images\""));assert!(body.contains("name=\"save_asset\"\r\n\r\nfalse"));
            assert!(bytes.windows(PNG.len()).any(|w| w==PNG));
            let mut s=s.lock().unwrap();s.uploads+=1;
            if s.rejected_upload {return (StatusCode::FORBIDDEN,Json(json!({"detail":"no_permission"}))).into_response();}
            if s.bad_upload {return (StatusCode::OK,"lost response").into_response();}
            Json(json!({"status":1,"image_filepaths":["uploads/frozen-mask.png"]})).into_response()
        }))
        .route(&format!("/v2/canvas/projects/{CANVAS}/generations"),post(|State(s):State<Shared>,headers:HeaderMap,Json(body):Json<Value>|async move {
            let mut s=s.lock().unwrap();s.submits.push((headers["idempotency-key"].to_str().unwrap().into(),body.clone()));
            if s.lose_submit && s.submits.len()==1 {return (StatusCode::OK,"lost response").into_response();}
            Json(json!({"task_id":TWO,"project_id":CANVAS,"operation":body["operation"],"status":"succeeded","revision":2,"outputs":[]})).into_response()
        }))
        .route("/api/v1/asset-context",get(||async{envelope(json!({"org_id":ORG,"canvas_space_id":CANVAS}))}))
        .route(&format!("/api/v1/assets/{TWO}"),get(||async{envelope(json!({"id":TWO,"type":"image","status":"ready","content":{"revision":1,"metadata_state":"ready","size_bytes":PNG.len()}}))}))
        .route(&format!("/api/v1/assets/{TWO}/download"),get(|State(s):State<Shared>|async move{
            let s=s.lock().unwrap();
            if s.mask_denied {return (StatusCode::FORBIDDEN,Json(json!({"detail":"no_permission"}))).into_response();}
            envelope(json!({"url":format!("{}/mask.png",s.base),"filename":"mask.png","delivery":"authenticated_stream","content":{"revision":1,"metadata_state":"ready"}})).into_response()
        }))
        .route("/mask.png",get(|State(s):State<Shared>|async move{s.lock().unwrap().downloads+=1;PNG}))
        .with_state(s.clone());
    let (base, handle) = serve(router).await;
    s.lock().unwrap().base = base.clone();
    (base, handle)
}
fn arguments() -> Vec<&'static str> {
    vec![
        "generation",
        "edit",
        "--source-element",
        ONE,
        "--prompt",
        "Make the cup blue",
        "--model",
        "edit-model",
        "--idempotency-key",
        "edit-test",
    ]
}

#[tokio::test]
async fn instruction_edit_pins_source_places_next_to_it_and_replays_without_rereading() {
    let s = Shared::default();
    let (base, handle) = server(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let mut args = arguments();
    args.extend([
        "--reference",
        GROUP,
        "--parameters",
        r#"{"quality":"medium"}"#,
        "--wait",
    ]);
    value(&run(dir.path(), &base, &args).await);
    s.lock().unwrap().changed = true;
    value(&run(dir.path(), &base, &args).await);
    let s = s.lock().unwrap();
    assert_eq!(s.reads, 1);
    assert_eq!(s.uploads, 0);
    assert_eq!(s.submits.len(), 2);
    assert_eq!(s.submits[0], s.submits[1]);
    let body = &s.submits[0].1;
    assert_eq!(body["operation"], "image_quick_edit");
    assert_eq!(body["input"]["organization_id"], ORG);
    assert_eq!(
        body["input"]["references"]["$1"]["source"]["expected_asset_id"],
        ASSET
    );
    assert_eq!(
        body["input"]["references"]["$2"]["source"]["element_id"],
        GROUP
    );
    assert_eq!(
        body["outputs"][0]["create_element"]["position"],
        json!({"x":350.0,"y":20.0})
    );
    assert_eq!(
        body["outputs"][0]["create_element"]["size"],
        json!({"width":300.0,"height":200.0})
    );
    assert!(body["outputs"][0].get("element_id").is_none());
    handle.abort();
}

#[tokio::test]
async fn inpaint_uploads_once_across_lost_submission_and_replay_rejects_changed_file() {
    let s = Shared::default();
    s.lock().unwrap().lose_submit = true;
    let (base, handle) = server(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mask.png");
    std::fs::write(&path, PNG).unwrap();
    let mut args = arguments();
    args.extend([
        "--mask-file",
        path.to_str().unwrap(),
        "--target-element",
        ONE,
    ]);
    value(&run(dir.path(), &base, &args).await);
    value(&run(dir.path(), &base, &args).await);
    {
        let s = s.lock().unwrap();
        assert_eq!(s.uploads, 1);
        assert_eq!(s.submits.len(), 3);
        assert!(s.submits.iter().all(|r| r == &s.submits[0]));
        assert_eq!(s.submits[0].1["operation"], "image_inpaint");
        assert_eq!(
            s.submits[0].1["input"]["mask_image_filepath"],
            "uploads/frozen-mask.png"
        );
        assert_eq!(s.submits[0].1["outputs"][0]["element_id"], ONE);
        assert!(s.submits[0].1["outputs"][0].get("create_element").is_none());
    }
    std::fs::write(&path, b"changed").unwrap();
    let out = run(dir.path(), &base, &args).await;
    assert_eq!(error(&out)["error"]["code"], "INVALID_INPUT");
    assert_eq!(s.lock().unwrap().uploads, 1);
    handle.abort();
}

#[tokio::test]
async fn uncertain_mask_upload_never_submits_or_silently_uploads_again() {
    let s = Shared::default();
    s.lock().unwrap().bad_upload = true;
    let (base, handle) = server(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mask.png");
    std::fs::write(&path, PNG).unwrap();
    let mut args = arguments();
    args.extend(["--mask-file", path.to_str().unwrap()]);
    for _ in 0..2 {
        let out = run(dir.path(), &base, &args).await;
        assert_eq!(out.status.code(), Some(7));
        assert_eq!(error(&out)["error"]["code"], "MASK_UPLOAD_UNCERTAIN");
    }
    let s = s.lock().unwrap();
    assert_eq!(s.uploads, 1);
    assert!(s.submits.is_empty());
    handle.abort();
}

#[tokio::test]
async fn unsupported_model_and_invalid_mask_do_not_upload_or_submit() {
    let s = Shared::default();
    s.lock().unwrap().no_inpaint = true;
    let (base, handle) = server(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mask.png");
    std::fs::write(&path, PNG).unwrap();
    let mut args = arguments();
    args.extend(["--mask-file", path.to_str().unwrap()]);
    assert_eq!(
        error(&run(dir.path(), &base, &args).await)["error"]["code"],
        "INVALID_INPUT"
    );
    s.lock().unwrap().no_inpaint = false;
    std::fs::write(&path, b"not png").unwrap();
    assert_eq!(
        error(&run(dir.path(), &base, &args).await)["error"]["code"],
        "INVALID_INPUT"
    );
    let s = s.lock().unwrap();
    assert_eq!(s.uploads, 0);
    assert!(s.submits.is_empty());
    handle.abort();
}

#[tokio::test]
async fn mask_asset_uses_authorized_download_and_reuses_frozen_upload() {
    let s = Shared::default();
    let (base, handle) = server(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "generation",
        "edit",
        "--source-asset",
        ASSET,
        "--mask-asset",
        TWO,
        "--prompt",
        "Blue cup",
        "--model",
        "edit-model",
        "--idempotency-key",
        "asset-edit",
    ];
    value(&run(dir.path(), &base, &args).await);
    value(&run(dir.path(), &base, &args).await);
    let s = s.lock().unwrap();
    assert_eq!(s.downloads, 1);
    assert_eq!(s.uploads, 1);
    assert_eq!(s.submits[0], s.submits[1]);
    assert_eq!(
        s.submits[0].1["input"]["references"]["$1"]["source"]["asset_id"],
        ASSET
    );
    handle.abort();
}

#[tokio::test]
async fn mask_download_denial_is_not_bypassed_with_preview_or_storage_path() {
    let s = Shared::default();
    s.lock().unwrap().mask_denied = true;
    let (base, handle) = server(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let mut args = arguments();
    args.extend(["--mask-asset", TWO]);
    assert_eq!(
        error(&run(dir.path(), &base, &args).await)["error"]["status"],
        403
    );
    let s = s.lock().unwrap();
    assert_eq!(s.downloads, 0);
    assert_eq!(s.uploads, 0);
    assert!(s.submits.is_empty());
    handle.abort();
}

#[tokio::test]
async fn rejected_upload_reports_denial_instead_of_unknown_and_is_not_repeated() {
    let s = Shared::default();
    s.lock().unwrap().rejected_upload = true;
    let (base, handle) = server(s.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mask.png");
    std::fs::write(&path, PNG).unwrap();
    let mut args = arguments();
    args.extend(["--mask-file", path.to_str().unwrap()]);
    for _ in 0..2 {
        let out = run(dir.path(), &base, &args).await;
        assert_eq!(out.status.code(), Some(4));
        assert_eq!(error(&out)["error"]["status"], 403);
    }
    let s = s.lock().unwrap();
    assert_eq!(s.uploads, 1);
    assert!(s.submits.is_empty());
    handle.abort();
}

#[tokio::test]
async fn conflicting_sources_masks_or_target_layout_are_rejected_before_network() {
    let dir = tempfile::tempdir().unwrap();
    for extra in [
        vec!["--source-asset", ASSET],
        vec!["--mask-file", "mask.png", "--mask-asset", TWO],
        vec!["--target-element", ONE, "--x", "1"],
    ] {
        let mut args = arguments();
        args.extend(extra);
        assert_eq!(
            error(&run(dir.path(), "http://127.0.0.1:1", &args).await)["error"]["code"],
            "INVALID_INPUT"
        );
    }
}
