use crate::{
    media_args::CanvasAdd,
    print,
    resource_paths::{Resolver, resource_id},
    storage::{PlacementIdentity, Store},
};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;
use virse_api::{
    AccessContext, Client, CreateAssetElement, ElementLayout, Error, ExistingAsset, Point, Result,
    Size,
};

pub async fn run(
    args: &CanvasAdd,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    canvas: Uuid,
    json_output: bool,
) -> Result<u8> {
    let key = args
        .idempotency_key
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if key.is_empty() || key.len() > 200 || !key.is_ascii() || key.chars().any(char::is_control) {
        return Err(Error::new("INVALID_INPUT", "Invalid idempotency key"));
    }
    let layout = ElementLayout {
        position: (args.x.is_some() || args.y.is_some()).then(|| Point {
            x: args.x.unwrap_or(0.0),
            y: args.y.unwrap_or(0.0),
        }),
        size: match (args.width, args.height) {
            (Some(width), Some(height)) => Some(Size { width, height }),
            (None, None) => None,
            _ => return Err(Error::new("INVALID_INPUT", "Specify both width and height")),
        },
    };
    layout.validate()?;
    if args
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty() || name.chars().count() > 255)
    {
        return Err(Error::new(
            "INVALID_INPUT",
            "Element name must contain 1..255 characters",
        ));
    }
    let user = client.me().await?["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Identity has no user ID"))?;
    let identity = PlacementIdentity {
        backend,
        user,
        org,
        canvas,
        key: &key,
    };
    let intent =
        json!({"resource":args.resource,"asset":args.asset,"name":args.name,"layout":layout});
    let body = match store.placement_command(&identity, &intent, None)? {
        Some(body) => body,
        None => {
            let (asset, name) = match (&args.resource, args.asset) {
                (Some(source), None) => {
                    let id = match resource_id(source) {
                        Some(id) => id,
                        None => Resolver::new(client, org).item(source).await?.id,
                    };
                    let detail = client.resource(org, id).await?;
                    let asset = detail.asset.ok_or_else(|| {
                        Error::new(
                            "UNSUPPORTED_ASSET",
                            "Choose an image, video, GLB, Word or PDF resource",
                        )
                    })?;
                    (asset, args.name.clone().or(Some(detail.resource.name)))
                }
                (None, Some(id)) => (
                    client
                        .asset_in(id, &AccessContext::organization(org))
                        .await?,
                    args.name.clone(),
                ),
                _ => return Err(Error::new("INVALID_INPUT", "Choose resource or asset")),
            };
            let asset: ExistingAsset = serde_json::from_value(asset)
                .map_err(|_| Error::new("INVALID_RESPONSE", "Asset metadata is incomplete"))?;
            if args.asset.is_some_and(|id| id != asset.id) {
                return Err(Error::new(
                    "INVALID_RESPONSE",
                    "Asset ID does not match the requested source",
                ));
            }
            let body = CreateAssetElement::from_asset(org, &asset, name, layout)?;
            store
                .placement_command(&identity, &intent, Some(&body))?
                .ok_or_else(|| Error::new("CONFIG_ERROR", "Cannot save canvas placement"))?
        }
    };
    eprintln!("Placement key: {key}");
    for attempt in 0..4 {
        match client.create_asset_element(canvas, &body, &key, user).await {
            Ok(created) => {
                print(&created, json_output)?;
                return Ok(0);
            }
            Err(error) if error.code == "CANVAS_HTTP_COMMANDS_NOT_ENABLED" => return Err(error),
            Err(error) if error.retryable || error.code == "INVALID_RESPONSE" => {
                if attempt == 3 {
                    return Err(Error {
                        idempotency_key: Some(key.clone().into_boxed_str()),
                        retryable: true,
                        ..Error::new(
                            "CANVAS_PLACEMENT_UNCERTAIN",
                            "Placement result unknown. Repeat the original command with the same --idempotency-key",
                        )
                    });
                }
                tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}
