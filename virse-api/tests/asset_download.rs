use axum::{Json, Router, extract::Query, http::StatusCode, routing::get};
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;
use virse_api::{Client, DownloadInfo};

async fn serve(app: Router) -> (Client, tokio::task::JoinHandle<()>) {
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", socket.local_addr().unwrap()),
        "virse_sk_download_test",
    )
    .unwrap();
    (
        client,
        tokio::spawn(async move {
            axum::serve(socket, app).await.unwrap();
        }),
    )
}

#[tokio::test]
async fn resource_download_rejects_metadata_descriptor_revision_race() {
    let resource = Uuid::new_v4();
    let asset = Uuid::new_v4();
    let org = Uuid::new_v4();
    let app = Router::new()
        .route(&format!("/api/v1/resources/{resource}"), get(move || async move {
            Json(json!({"request_id":"read", "data": {
                "resource":{"id":resource,"name":"a.png","kind":"asset_image"},
                "kind":"asset_image","asset":{"id":asset,"content":{"revision":3}},"detail":{}
            }}))
        }))
        .route(&format!("/api/v1/assets/{asset}/download"), get(move |Query(q): Query<HashMap<String,String>>| async move {
            assert_eq!(q["dentry_id"], resource.to_string());
            assert_eq!(q["org_id"], org.to_string());
            Json(json!({"request_id":"download", "data":{
                "url":"/bytes?source_version=v4&object_version=8","filename":"a.png","delivery":"authenticated_stream",
                "content":{"revision":4,"metadata_state":"partial"}
            }}))
        }));
    let (client, server) = serve(app).await;
    let error = client
        .resource_download_info(org, resource)
        .await
        .unwrap_err();
    assert_eq!(error.code, "content_changed");
    assert_eq!(error.details.unwrap()["expected_revision"], 3);
    server.abort();
}

#[tokio::test]
async fn protected_stream_preserves_version_query_and_conflict_details() {
    let (client, server) = serve(Router::new().route("/bytes", get(|Query(q):Query<HashMap<String,String>>| async move {
        assert_eq!(q["source_version"], "v3");
        assert_eq!(q["object_version"], "8");
        (StatusCode::CONFLICT, Json(json!({"request_id":"conflict", "error":{
            "code":"content_changed","message":"Updated","retryable":false,"details":{"current_revision":4}
        }})))
    }))).await;
    let info: DownloadInfo = serde_json::from_value(json!({
        "url":"/bytes?source_version=v3&object_version=8","filename":"a.png","delivery":"authenticated_stream"
    })).unwrap();
    let error = client.download(&info).await.unwrap_err();
    assert_eq!(error.code, "content_changed");
    assert_eq!(error.status, Some(409));
    assert_eq!(error.request_id.as_deref(), Some("conflict"));
    assert_eq!(error.details.unwrap()["current_revision"], 4);
    server.abort();
}

#[test]
fn descriptor_roundtrip_keeps_versions_and_checks_required_revision() {
    let value = json!({"url":"/bytes","filename":"a.png","delivery":"signed_url",
        "source_version":"v3","object_version":"8","content":{"revision":3,"metadata_state":"partial"}});
    let info: DownloadInfo = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(info.source_version(), Some("v3"));
    assert_eq!(info.object_version(), Some("8"));
    assert_eq!(info.content().unwrap().unwrap().revision, 3);
    assert_eq!(serde_json::to_value(&info).unwrap(), value);
    info.check_revision(&json!({"content":{"revision":3}}))
        .unwrap();
    assert!(
        info.check_revision(&json!({"content":{"revision":null}}))
            .is_err()
    );
    let legacy: DownloadInfo =
        serde_json::from_value(json!({"url":"/bytes","filename":"a","delivery":"external_url"}))
            .unwrap();
    legacy.check_revision(&Value::Null).unwrap();
    assert!(
        legacy
            .check_revision(&json!({"content":{"revision":3}}))
            .is_err()
    );
}
