use axum::{
    Json, Router,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use uuid::Uuid;
use virse_api::{
    AccessContext, Asset, AssetPage, AssetQuery, AssetRepresentation, AssetRetention, AssetType,
    AssetUploadFile, Client, Destination, DocumentType, MAX_ASSET_UPLOAD_BYTES, ReplaceOptions,
    ResourcePage, UploadConflict, UploadOptions,
};

const ORG: &str = "00000000-0000-0000-0000-000000000001";
const USER: &str = "00000000-0000-0000-0000-000000000002";
const ASSET: &str = "00000000-0000-0000-0000-000000000003";
const ANCHOR: &str = "00000000-0000-0000-0000-000000000004";
const TOKEN: &str = "virse_sk_asset_test";

fn id(value: &str) -> Uuid {
    Uuid::parse_str(value).unwrap()
}
fn context() -> AccessContext {
    AccessContext::organization(id(ORG))
}
fn options() -> UploadOptions {
    UploadOptions::new(Destination::folder(id(ORG), Some(id(ANCHOR))))
}
fn file() -> AssetUploadFile {
    AssetUploadFile::new("原件.png", vec![0, 255, 13, 10, 128, 42])
}
fn asset(revision: u64) -> Value {
    json!({"id":ASSET,"type":"image","document_type":null,"status":"ready",
        "description":"asset", "created_at":"2026-10-28T00:00:00Z","modified_at":"2026-10-28T00:00:00Z",
        "content":{"revision":revision,"mime_type":"image/png","size_bytes":6,"checksum":null,
            "metadata_state":"partial","future_content":{"v":1}},
        "capabilities":{"read":true,"download":false,"replace_content":true,"future_permission":true},
        "attributes":{"width":1,"future_attribute":[1,2]},"future_asset":{"v":2}})
}
fn envelope(data: Value) -> Json<Value> {
    Json(json!({"request_id":"asset-test","data":data}))
}
async fn serve(app: Router) -> (Client, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(&format!("http://{}", listener.local_addr().unwrap()), TOKEN).unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (client, handle)
}
fn write_headers(headers: &HeaderMap) {
    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
    assert_eq!(headers["x-virse-expected-user"], USER);
    assert_eq!(headers["idempotency-key"], "frozen-key");
    // api 是服务端缺省值，显式头也必须为 api。
    if let Some(source) = headers.get("x-virse-write-source") {
        assert_eq!(source, "api");
    }
}
fn field<'a>(headers: &HeaderMap, body: &'a [u8], name: &str) -> &'a [u8] {
    let content_type = headers["content-type"].to_str().unwrap();
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap();
    let marker = format!("name=\"{name}\"");
    let start = body
        .windows(marker.len())
        .position(|v| v == marker.as_bytes())
        .unwrap();
    let part = &body[start..];
    let start = part.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
    let part = &part[start..];
    let end = format!("\r\n--{boundary}");
    let end = part
        .windows(end.len())
        .position(|v| v == end.as_bytes())
        .unwrap();
    &part[..end]
}

#[tokio::test]
async fn upload_uses_owned_binary_multipart_and_explicit_replay_preserves_identity() {
    let records = Arc::new(Mutex::new(Vec::<Value>::new()));
    let app = Router::new().route("/api/v1/assets", post(
        |State(records): State<Arc<Mutex<Vec<Value>>>>, headers: HeaderMap, body: Bytes| async move {
            write_headers(&headers);
            assert_eq!(field(&headers, &body, "file"), file().bytes);
            assert!(body.windows("filename=\"原件.png\"".len()).any(|v| v == "filename=\"原件.png\"".as_bytes()));
            let options: Value = serde_json::from_slice(field(&headers, &body, "options")).unwrap();
            assert_eq!(options, json!({"destination":{"kind":"drive","org_id":ORG,"parent_id":ANCHOR},"conflict":"rename","retention":"persistent"}));
            records.lock().unwrap().push(options);
            (StatusCode::CREATED, envelope(json!({"asset":asset(1),"resource":{"id":ANCHOR,"parent_id":null,"name":"原件.png","kind":"asset_image","asset_id":ASSET},"skipped":false,"future_upload":true})))
        }
    )).with_state(records.clone());
    let (client, server) = serve(app).await;
    for _ in 0..2 {
        let result = client
            .upload_asset(file(), &options(), "frozen-key", id(USER))
            .await
            .unwrap();
        assert_eq!(result.asset.id, id(ASSET));
        assert_eq!(result.asset.content.revision, 1);
        assert_eq!(result.resource.unwrap().asset_id, Some(id(ASSET)));
        assert!(!result.skipped);
        assert_eq!(result.extra["future_upload"], true);
        assert_eq!(result.asset.content.extra["future_content"], json!({"v":1}));
    }
    assert_eq!(records.lock().unwrap().len(), 2);
    server.abort();
}

#[tokio::test]
async fn upload_rejects_contradictory_resource_identity_or_uncommitted_content() {
    for inconsistent_resource in [false, true] {
        let app = Router::new().route("/api/v1/assets", post(move || async move {
            envelope(json!({"asset":asset(if inconsistent_resource { 1 } else { 0 }),
                "resource":if inconsistent_resource { json!({"id":ANCHOR,"name":"file.png","kind":"asset_image","asset_id":USER}) } else { Value::Null },
                "skipped":false}))
        }));
        let (client, server) = serve(app).await;
        assert_eq!(
            client
                .upload_asset(file(), &options(), "key", id(USER))
                .await
                .unwrap_err()
                .code,
            "INVALID_RESPONSE"
        );
        server.abort();
    }
}

#[tokio::test]
async fn upload_skip_preserves_existing_revision_and_nullable_resource() {
    let app = Router::new().route(
        "/api/v1/assets",
        post(|headers: HeaderMap, body: Bytes| async move {
            let options: Value = serde_json::from_slice(field(&headers, &body, "options")).unwrap();
            assert_eq!(options["conflict"], "skip");
            envelope(json!({"asset":asset(9),"resource":null,"skipped":true}))
        }),
    );
    let (client, server) = serve(app).await;
    let mut options = options();
    options.conflict = UploadConflict::Skip;
    let result = client
        .upload_asset(file(), &options, "frozen-key", id(USER))
        .await
        .unwrap();
    assert!(result.skipped);
    assert_eq!(result.asset.content.revision, 9);
    assert!(result.resource.is_none());
    server.abort();
}

#[tokio::test]
async fn replace_put_pins_context_and_both_revisions() {
    let app = Router::new().route(&format!("/api/v1/assets/{ASSET}/content"), put(|headers: HeaderMap, body: Bytes| async move {
        write_headers(&headers);
        assert_eq!(field(&headers, &body, "file"), file().bytes);
        let options: Value = serde_json::from_slice(field(&headers, &body, "options")).unwrap();
        assert_eq!(options, json!({"context":{"org_id":ORG,"dentry_id":ANCHOR},"expected_revision":3,"reset_model_state":true,"expected_state_revision":0}));
        envelope(asset(4))
    }));
    let (client, server) = serve(app).await;
    let mut options = ReplaceOptions::new(context(), 3);
    options.context.dentry_id = Some(id(ANCHOR));
    options.reset_model_state = true;
    options.expected_state_revision = Some(0);
    let result = client
        .replace_asset_content(id(ASSET), file(), &options, "frozen-key", id(USER))
        .await
        .unwrap();
    assert_eq!(result.content.revision, 4);
    assert_eq!(result.id, id(ASSET));
    server.abort();
}

#[tokio::test]
async fn conflict_errors_preserve_details_and_never_retry_or_change_access_anchor() {
    for code in [
        "idempotency_conflict",
        "revision_conflict",
        "content_changed",
        "identity_changed",
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(&format!("/api/v1/assets/{ASSET}/content"), put(
            move |State(calls): State<Arc<AtomicUsize>>, headers: HeaderMap| async move {
                write_headers(&headers);
                calls.fetch_add(1, Ordering::SeqCst);
                (StatusCode::CONFLICT, Json(json!({"request_id":"conflict-request","error":{"code":code,"message":"changed","retryable":false,"details":{"current_revision":5,"completed_revision":4}}})))
            }
        )).with_state(calls.clone());
        let (client, server) = serve(app).await;
        let error = client
            .replace_asset_content(
                id(ASSET),
                file(),
                &ReplaceOptions::new(context(), 3),
                "frozen-key",
                id(USER),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.status, Some(409));
        assert_eq!(error.request_id.as_deref(), Some("conflict-request"));
        assert!(!error.retryable);
        assert_eq!(error.details.unwrap()["current_revision"], 5);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.abort();
    }
}

#[tokio::test]
async fn malformed_or_stale_success_is_a_protocol_error() {
    for change in ["id", "revision", "missing_revision"] {
        let mut value = asset(4);
        match change {
            "id" => value["id"] = json!(USER),
            "revision" => value["content"]["revision"] = json!(3),
            _ => {
                value["content"].as_object_mut().unwrap().remove("revision");
            }
        }
        let app = Router::new().route(
            &format!("/api/v1/assets/{ASSET}/content"),
            put(move || {
                let value = value.clone();
                async move { envelope(value) }
            }),
        );
        let (client, server) = serve(app).await;
        let error = client
            .replace_asset_content(
                id(ASSET),
                file(),
                &ReplaceOptions::new(context(), 3),
                "key",
                id(USER),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "INVALID_RESPONSE");
        server.abort();
    }
}

#[tokio::test]
async fn write_validation_rejects_invalid_keys_files_and_options_before_network() {
    let client = Client::new("http://127.0.0.1:1", TOKEN).unwrap();
    for key in [
        "",
        "with space",
        "with\ttab",
        "newline\n",
        "é",
        "\u{7f}",
        &"x".repeat(129),
    ] {
        let error = client
            .upload_asset(file(), &options(), key, id(USER))
            .await
            .unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT", "{key:?}");
        let error = client
            .replace_asset_content(
                id(ASSET),
                file(),
                &ReplaceOptions::new(context(), 1),
                key,
                id(USER),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT", "{key:?}");
    }
    for invalid in [
        AssetUploadFile::new("x.png", vec![]),
        AssetUploadFile::new("\n.png", vec![1]),
        AssetUploadFile::new(" ", vec![1]),
    ] {
        assert_eq!(
            client
                .upload_asset(invalid, &options(), "key", id(USER))
                .await
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );
    }
    let mut invalid = file();
    invalid.mime_type = Some("not a mime\n".into());
    assert_eq!(
        client
            .upload_asset(invalid, &options(), "key", id(USER))
            .await
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
    let mut invalid = options();
    invalid.retention = AssetRetention::Pending;
    assert_eq!(
        client
            .upload_asset(file(), &invalid, "key", id(USER))
            .await
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
    let mut reset = ReplaceOptions::new(context(), 1);
    reset.reset_model_state = true;
    let mut ambiguous = ReplaceOptions::new(context(), 1);
    ambiguous.context.dentry_id = Some(id(ANCHOR));
    ambiguous.context.canvas_space_id = Some(id(ANCHOR));
    for options in [ReplaceOptions::new(context(), 0), reset, ambiguous] {
        assert_eq!(
            client
                .replace_asset_content(id(ASSET), file(), &options, "key", id(USER))
                .await
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );
    }
}

#[test]
fn size_limit_is_exactly_100_mib_for_upload_and_replace() {
    let mut file = AssetUploadFile::new("large.glb", vec![0; MAX_ASSET_UPLOAD_BYTES + 1]);
    assert_eq!(file.validate().unwrap_err().code, "INVALID_INPUT");
    file.bytes.pop();
    file.validate().unwrap();
}

#[tokio::test]
async fn resolve_context_preserves_explicit_workspace_and_canvas() {
    let app = Router::new().route(
        "/api/v1/asset-context",
        get(
            |Query(query): Query<HashMap<String, String>>, headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                if query.is_empty() {
                    envelope(json!({"org_id":ORG}))
                } else {
                    assert_eq!(query.len(), 2);
                    assert_eq!(query["org_id"], ORG);
                    assert_eq!(query["canvas_space_id"], ANCHOR);
                    envelope(json!({"org_id":ORG,"canvas_space_id":ANCHOR}))
                }
            },
        ),
    );
    let (client, server) = serve(app).await;
    assert_eq!(
        client
            .resolve_asset_context(None, None)
            .await
            .unwrap()
            .org_id,
        id(ORG)
    );
    let result = client
        .resolve_asset_context(Some(id(ORG)), Some(id(ANCHOR)))
        .await
        .unwrap();
    assert_eq!(result.org_id, id(ORG));
    assert_eq!(result.canvas_space_id, Some(id(ANCHOR)));
    assert!(result.dentry_id.is_none());
    server.abort();
}

#[tokio::test]
async fn denied_context_does_not_fall_back_to_personal_org() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route("/api/v1/asset-context", get(|State(calls): State<Arc<AtomicUsize>>, Query(query): Query<HashMap<String, String>>| async move {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(query["org_id"], ORG);
        (StatusCode::FORBIDDEN, Json(json!({"request_id":"denied","error":{"code":"forbidden","message":"denied","retryable":false}})))
    })).with_state(calls.clone());
    let (client, server) = serve(app).await;
    assert_eq!(
        client
            .resolve_asset_context(Some(id(ORG)), None)
            .await
            .unwrap_err()
            .status,
        Some(403)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn context_response_cannot_silently_switch_workspace() {
    let app = Router::new().route(
        "/api/v1/asset-context",
        get(|| async { envelope(json!({"org_id":USER})) }),
    );
    let (client, server) = serve(app).await;
    assert_eq!(
        client
            .resolve_asset_context(Some(id(ORG)), None)
            .await
            .unwrap_err()
            .code,
        "INVALID_RESPONSE"
    );
    server.abort();
}

#[tokio::test]
async fn asset_query_preserves_filters_snapshot_cursor_and_unknown_response_fields() {
    let app = Router::new().route("/api/v1/assets/query", post(|Json(query): Json<Value>| async move {
        assert_eq!(query, json!({"context":{"org_id":ORG,"canvas_space_id":ANCHOR},"types":["document"],"document_type":"pdf","query":"report","include_public":true,"cursor":"page-2","limit":20}));
        envelope(json!({"items":[asset(7)],"next_cursor":"page-3","snapshot":"snapshot-1","truncated":false,"sync_cursor":"baseline-not-latest","future_page":123}))
    }));
    let (client, server) = serve(app).await;
    let mut query = AssetQuery::new(id(ORG));
    query.context.canvas_space_id = Some(id(ANCHOR));
    query.types = vec![AssetType::Document];
    query.document_type = Some(DocumentType::Pdf);
    query.query = Some("report".into());
    query.include_public = true;
    query.cursor = Some("page-2".into());
    query.limit = 20;
    let page = client.assets(&query).await.unwrap();
    assert_eq!(page.sync_cursor.as_deref(), Some("baseline-not-latest"));
    assert_eq!(page.next_cursor.as_deref(), Some("page-3"));
    assert_eq!(page.snapshot, "snapshot-1");
    assert_eq!(page.extra["future_page"], 123);
    assert_eq!(page.items[0].content.revision, 7);
    server.abort();
}

#[tokio::test]
async fn invalid_query_or_access_context_is_rejected_locally() {
    let client = Client::new("http://127.0.0.1:1", TOKEN).unwrap();
    for limit in [0, 201] {
        let mut query = AssetQuery::new(id(ORG));
        query.limit = limit;
        assert_eq!(
            client.assets(&query).await.unwrap_err().code,
            "INVALID_INPUT"
        );
    }
    let mut query = AssetQuery::new(id(ORG));
    query.query = Some("字".repeat(257));
    assert_eq!(
        client.assets(&query).await.unwrap_err().code,
        "INVALID_INPUT"
    );
    query.query = Some("字".repeat(256));
    query.validate().unwrap();
    query.cursor = Some("x".repeat(257));
    assert!(query.validate().is_err());
    let mut context = context();
    context.dentry_id = Some(id(ANCHOR));
    context.canvas_space_id = Some(id(ANCHOR));
    assert_eq!(
        client
            .presentation_info_in(id(ASSET), &context, AssetRepresentation::Preview)
            .await
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
    assert_eq!(
        client
            .asset_detail(id(ASSET), &context)
            .await
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
}

#[tokio::test]
async fn presentation_uses_read_access_and_preserves_delivery_representation_and_versions() {
    let app = Router::new()
        .route(&format!("/api/v1/assets/{ASSET}/presentation"), get(|Query(query): Query<HashMap<String, String>>| async move {
            assert_eq!(query["org_id"], ORG); assert_eq!(query["dentry_id"], ANCHOR);
            assert!(!query.contains_key("canvas_space_id"));
            let representation = &query["representation"];
            envelope(json!({"url":format!("/api/v1/assets/{ASSET}/presentation/content?representation={representation}&source_version=source%2F1&object_version=object%2B1&org_id={ORG}&dentry_id={ANCHOR}"),"delivery":"authenticated_stream","filename":"preview.png","content":asset(7)["content"],"source_version":"source/1","object_version":"object+1","future_download":true}))
        }))
        .route(&format!("/api/v1/assets/{ASSET}/presentation/content"), get(|Query(query): Query<HashMap<String, String>>, headers: HeaderMap| async move {
            assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
            assert_eq!(query["source_version"], "source/1"); assert_eq!(query["object_version"], "object+1");
            assert!(matches!(query["representation"].as_str(), "preview" | "content"));
            "presentation bytes"
        }));
    let (client, server) = serve(app).await;
    let mut context = context();
    context.dentry_id = Some(id(ANCHOR));
    for representation in [AssetRepresentation::Preview, AssetRepresentation::Content] {
        let info = client
            .presentation_info_in(id(ASSET), &context, representation)
            .await
            .unwrap();
        assert_eq!(info.extra["future_download"], true);
        assert!(
            info.url
                .contains(&format!("representation={}", representation.as_str()))
        );
        assert_eq!(
            client.download(&info).await.unwrap().text().await.unwrap(),
            "presentation bytes"
        );
    }
    server.abort();
}

#[tokio::test]
async fn unavailable_preview_does_not_fall_back_to_original_file() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(&format!("/api/v1/assets/{ASSET}/presentation"), get(|State(calls): State<Arc<AtomicUsize>>| async move {
        calls.fetch_add(1, Ordering::SeqCst);
        (StatusCode::NOT_FOUND, Json(json!({"request_id":"preview","error":{"code":"presentation_unavailable","message":"no poster","retryable":false}})))
    })).with_state(calls.clone());
    let (client, server) = serve(app).await;
    assert_eq!(
        client
            .presentation_info_in(id(ASSET), &context(), AssetRepresentation::Preview)
            .await
            .unwrap_err()
            .code,
        "presentation_unavailable"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn typed_asset_detail_keeps_unknown_type_and_nested_metadata() {
    let app = Router::new().route(
        &format!("/api/v1/assets/{ASSET}"),
        get(|Query(query): Query<HashMap<String, String>>| async move {
            assert_eq!(query["org_id"], ORG);
            let mut value = asset(0);
            value["type"] = json!("future_type");
            value["content"]["checksum"] =
                json!({"algorithm":"sha256","value":"ab".repeat(32),"future_checksum":true});
            envelope(value)
        }),
    );
    let (client, server) = serve(app).await;
    let asset = client.asset_detail(id(ASSET), &context()).await.unwrap();
    assert_eq!(asset.asset_type, AssetType::Other("future_type".into()));
    let output = serde_json::to_value(asset).unwrap();
    assert_eq!(output["type"], "future_type");
    assert_eq!(output["future_asset"], json!({"v":2}));
    assert_eq!(output["content"]["checksum"]["future_checksum"], true);
    assert_eq!(output["capabilities"]["future_permission"], true);
    assert_eq!(output["attributes"]["future_attribute"], json!([1, 2]));
    server.abort();
}

#[test]
fn typed_destinations_are_independent_and_persistent_by_default() {
    for (destination, wire) in [
        (
            Destination::Drive {
                org_id: id(ORG),
                parent_id: None,
            },
            json!({"kind":"drive","org_id":ORG}),
        ),
        (
            Destination::Canvas {
                org_id: id(ORG),
                canvas_space_id: id(ANCHOR),
            },
            json!({"kind":"canvas","org_id":ORG,"canvas_space_id":ANCHOR}),
        ),
        (
            Destination::Unbundled { org_id: id(ORG) },
            json!({"kind":"unbundled","org_id":ORG}),
        ),
        (
            Destination::Space {
                org_id: id(ORG),
                space_id: id(ANCHOR),
            },
            json!({"kind":"space","org_id":ORG,"space_id":ANCHOR}),
        ),
    ] {
        assert_eq!(serde_json::to_value(&destination).unwrap(), wire);
        let mut options: UploadOptions =
            serde_json::from_value(json!({"destination":wire})).unwrap();
        assert_eq!(options.retention, AssetRetention::Persistent);
        assert_eq!(options.conflict, UploadConflict::Rename);
        options.validate().unwrap();
        options.retention = AssetRetention::Pending;
        assert_eq!(
            options.validate().is_ok(),
            matches!(destination, Destination::Unbundled { .. })
        );
    }
}

#[test]
fn absent_checksum_stays_unknown_and_page_sync_cursor_is_optional() {
    let parsed: Asset = serde_json::from_value(asset(1)).unwrap();
    assert!(parsed.content.checksum.is_none());
    assert!(serde_json::to_value(parsed).unwrap()["content"]["checksum"].is_null());
    for cursor in [None, Some("snapshot-baseline")] {
        let mut wire =
            json!({"items":[],"snapshot":"snapshot","next_cursor":null,"truncated":false});
        if let Some(cursor) = cursor {
            wire["sync_cursor"] = json!(cursor);
        }
        let resources: ResourcePage = serde_json::from_value(wire.clone()).unwrap();
        let assets: AssetPage = serde_json::from_value(wire).unwrap();
        assert_eq!(resources.sync_cursor.as_deref(), cursor);
        assert_eq!(assets.sync_cursor.as_deref(), cursor);
    }
}
