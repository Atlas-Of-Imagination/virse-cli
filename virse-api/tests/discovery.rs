use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    routing::{get, patch, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use virse_api::*;
const ORG: &str = "00000000-0000-0000-0000-000000000001";
const RES: &str = "00000000-0000-0000-0000-000000000002";
const CANVAS: &str = "00000000-0000-0000-0000-000000000003";
const ASSET: &str = "00000000-0000-0000-0000-000000000004";
fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}
async fn serve(app: Router) -> (Client, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        "virse_sk_test_discovery",
    )
    .unwrap();
    (
        client,
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    )
}
fn catalog() -> Value {
    json!({"schema_version":2,"catalog_version":"test","models":[{"model_key":"test-3d","mode":"3d","display_name":"Test","capabilities":{"mesh":true},"price":{"amount":"0.123456789"}}],"defaults":{"3d":"test-3d"},"creator_models":[]})
}
#[tokio::test]
async fn space_discovery_requires_no_org_and_preserves_server_roles() {
    let (client,server)=serve(Router::new().route("/get_organization_by_user",post(|headers:HeaderMap,Json(body):Json<Value>|async move{
        assert_eq!(headers["authorization"],"Bearer virse_sk_test_discovery");assert_eq!(body,json!({"relation_types":["has_owner","has_member"]}));
        Json(json!({"status":1,"organizations":[{"id":ORG,"name":"Team","org_type":"team","my_role":"admin","relation_type":"has_owner","allow_external_canvas_sharing":false}]}))
    }))).await;
    let result = client.organizations().await.unwrap();
    assert_eq!(result.organizations[0].my_role.as_deref(), Some("admin"));
    assert_eq!(
        result.organizations[0].extra["allow_external_canvas_sharing"],
        false
    );
    server.abort();
}
#[tokio::test]
async fn catalog_requests_all_four_modes_and_keeps_pricing_and_context() {
    let (client, server) = serve(Router::new().route(
        "/generation/models/catalog",
        post(|Json(body): Json<Value>| async move {
            assert_eq!(body["organization_id"], ORG);
            assert_eq!(body["schema_version"], 2);
            assert_eq!(body["modes"], json!(["image", "text", "video", "3d"]));
            assert_eq!(body["include_user_preferences"], true);
            assert_eq!(body["include_disabled"], true);
            Json(catalog())
        }),
    ))
    .await;
    let result = client.model_catalog(id(ORG), &[], true).await.unwrap();
    assert_eq!(result.models[0].extra["price"]["amount"], "0.123456789");
    assert_eq!(result.extra["creator_models"], json!([]));
    server.abort();
}
#[tokio::test]
async fn favorite_model_key_is_a_path_segment_not_query_or_fragment() {
    let (client, server) = serve(Router::new().fallback_service(patch(
        |uri: Uri, Json(body): Json<Value>| async move {
            assert!(uri.query().is_none());
            assert_eq!(
                uri.path(),
                "/generation/models/provider%2Ftest%3Fkey%23x/favorite"
            );
            assert_eq!(body, json!({"organization_id":ORG,"is_favorite":true}));
            Json(json!({"model_key":"provider/test?key#x","is_favorite":true,"favorite_rank":123}))
        },
    )))
    .await;
    assert!(
        client
            .set_model_favorite(id(ORG), "provider/test?key#x", true)
            .await
            .unwrap()
            .is_favorite
    );
    server.abort();
}
#[tokio::test]
async fn balance_retains_decimal_strings_and_legacy_failures_are_not_zero_balances() {
    let (client,server)=serve(Router::new().route("/billing/balance",post(|Json(body):Json<Value>|async move{
        assert_eq!(body,json!({"organization_id":ORG}));Json(json!({"status":1,"org_id":ORG,"total_balance":"9999999999.00000001","member_budget":{"no_limit":true,"credit_remaining":null},"free_license":{"active":false}}))
    }))).await;
    let result = client.balance(id(ORG)).await.unwrap();
    assert_eq!(result.extra["total_balance"], "9999999999.00000001");
    assert!(result.extra["member_budget"]["credit_remaining"].is_null());
    server.abort();
    let (client, server) = serve(Router::new().route(
        "/billing/balance",
        post(|| async { Json(json!({"status":0,"message":"NOT_ORG_MEMBER","total_balance":0})) }),
    ))
    .await;
    assert_eq!(client.balance(id(ORG)).await.unwrap_err().status, Some(403));
    server.abort();
}
#[tokio::test]
async fn canvas_creation_and_contents_use_resource_identity_and_v2_content_contract() {
    let (client,server)=serve(Router::new().route("/versa/canvas/create",post(|Json(body):Json<Value>|async move{
        assert_eq!(body,json!({"org_id":ORG,"parent_id":RES,"name":"Draft"}));Json(json!({"status":1,"canvas_space_id":CANVAS,"dentry":{"id":ASSET,"name":"Draft","kind":"canvas","target_id":CANVAS}}))
    })).route("/versa/canvas/contents",post(|Json(body):Json<Value>|async move{
        assert_eq!(body["contract_version"],2);assert_eq!(body["dentry_id"],ASSET);assert_eq!(body["offset"],2);assert_eq!(body["types"],json!(["document"]));assert_eq!(body["document_types"],json!(["pdf"]));assert_eq!(body["sort_by"],"size_bytes");
        Json(json!({"status":1,"canvas_id":CANVAS,"items":[{"content_id":"content","element_id":"element"}],"total":8,"offset":2,"limit":1,"has_more":true}))
    }))).await;
    assert_eq!(
        client
            .create_canvas(id(ORG), Some(id(RES)), "Draft")
            .await
            .unwrap()
            .canvas_space_id,
        id(CANVAS)
    );
    let mut query = CanvasContentsQuery::new(id(ORG), id(ASSET));
    query.offset = 2;
    query.limit = 1;
    query.types = vec!["document".into()];
    query.document_types = vec!["pdf".into()];
    query.sort_by = "size_bytes".into();
    assert!(client.canvas_contents(&query).await.unwrap().has_more);
    server.abort();
}
#[tokio::test]
async fn uncertain_canvas_create_is_sent_once_and_response_identity_is_checked() {
    let count = Arc::new(Mutex::new(0));
    let (client, server) = serve(
        Router::new()
            .route(
                "/versa/canvas/create",
                post(|State(count): State<Arc<Mutex<u32>>>| async move {
                    *count.lock().unwrap() += 1;
                    StatusCode::SERVICE_UNAVAILABLE
                }),
            )
            .with_state(count.clone()),
    )
    .await;
    assert!(client.create_canvas(id(ORG), None, "Draft").await.is_err());
    assert_eq!(*count.lock().unwrap(), 1);
    server.abort();
    let (client,server)=serve(Router::new().route("/versa/canvas/create",post(||async{Json(json!({"status":1,"canvas_space_id":CANVAS,"dentry":{"id":ASSET,"name":"Draft","kind":"canvas","target_id":RES}}))}))).await;
    assert_eq!(
        client
            .create_canvas(id(ORG), None, "Draft")
            .await
            .unwrap_err()
            .code,
        "INVALID_RESPONSE"
    );
    server.abort();
}
#[tokio::test]
async fn preview_uses_requested_dentry_and_rejects_revision_race_without_original_download() {
    let (client,server)=serve(Router::new().route(&format!("/api/v1/resources/{RES}"),get(||async{Json(json!({"request_id":"r","data":{"resource":{"id":RES,"kind":"shortcut","name":"Shared"},"kind":"asset_image","asset":{"id":ASSET,"content":{"revision":2}},"detail":{}}}))}))
        .route(&format!("/api/v1/assets/{ASSET}/presentation"),get(|uri:Uri|async move{
            assert!(uri.query().unwrap().contains(&format!("dentry_id={RES}")));assert!(uri.query().unwrap().contains("representation=preview"));
            Json(json!({"request_id":"p","data":{"url":"/preview","filename":"preview.jpg","delivery":"authenticated_stream","content":{"revision":3,"metadata_state":"partial"}}}))
        }))).await;
    assert_eq!(
        client
            .resource_presentation_info(id(ORG), id(RES), AssetRepresentation::Preview)
            .await
            .unwrap_err()
            .code,
        "content_changed"
    );
    server.abort();
}
#[test]
fn shortcut_builders_use_native_source_contract_and_do_not_invent_input_fields() {
    let output = OutputTarget {
        output_index: 0,
        element_id: Some(id(CANVAS)),
        create_element: None,
    };
    let text = SubmitTask::text(
        "text-model",
        "Write",
        vec![],
        Some("enhance prompt"),
        output.clone(),
    )
    .unwrap();
    assert_eq!(text.operation, "text_generate");
    assert!(!text.input.contains_key("image_count"));
    assert!(!text.input.contains_key("parameters"));
    let source = ReferenceSource::Asset {
        asset_type: "image".into(),
        asset_id: id(ASSET),
    };
    let upscale = SubmitTask::image_process(
        ImageProcess::Upscale { times: 4 },
        "upscaler",
        source.clone(),
        output.clone(),
    )
    .unwrap();
    assert_eq!(upscale.input["source"]["asset_id"], ASSET);
    assert_eq!(upscale.input["upscale_times"], 4);
    assert!(!upscale.input.contains_key("original_image_id"));
    assert!(
        SubmitTask::image_process(
            ImageProcess::Upscale { times: 0 },
            "upscaler",
            source,
            output.clone()
        )
        .is_err()
    );
    assert!(
        SubmitTask::image_process(
            ImageProcess::RemoveBackground,
            "remove",
            ReferenceSource::Text {
                value: "bad".into()
            },
            output
        )
        .is_err()
    );
}
