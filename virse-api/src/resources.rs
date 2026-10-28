use crate::{
    AccessContext, Client, DownloadInfo, Error, ResourceBatch, ResourceDetail, ResourceItem,
    ResourceMutation, ResourcePage, ResourceQuery, Result,
};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

fn parse<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::protocol())
}
impl Client {
    pub async fn resources(&self, query: &ResourceQuery) -> Result<ResourcePage> {
        if query.context.dentry_id.is_some()
            || query.context.canvas_space_id.is_some()
            || !(1..=200).contains(&query.limit)
            || query
                .access_levels
                .iter()
                .any(|level| !(1..=4).contains(level))
            || !matches!(
                query.sort.as_str(),
                "custom" | "name" | "created_at" | "modified_at" | "size"
            )
            || !matches!(query.direction.as_str(), "asc" | "desc")
        {
            return Err(Error::new(
                "INVALID_INPUT",
                "Invalid resource query context or limit",
            ));
        }
        parse(
            self.request(
                Method::POST,
                "api/v1/resources/query",
                &[],
                Some(json!(query)),
                None,
            )
            .await?,
        )
    }
    pub async fn resource(&self, org: Uuid, id: Uuid) -> Result<ResourceDetail> {
        parse(
            self.request(
                Method::GET,
                &format!("api/v1/resources/{id}"),
                &[("org_id", org.to_string())],
                None,
                None,
            )
            .await?,
        )
    }
    pub async fn resource_summary(&self, query: &ResourceQuery) -> Result<Value> {
        if query.cursor.is_some() {
            return Err(Error::new(
                "INVALID_INPUT",
                "Summary does not accept a cursor",
            ));
        }
        self.request(
            Method::POST,
            "api/v1/resources/summary",
            &[],
            Some(json!(query)),
            None,
        )
        .await
    }
    pub async fn asset_in(&self, id: Uuid, context: &AccessContext) -> Result<Value> {
        self.request(
            Method::GET,
            &format!("api/v1/assets/{id}"),
            &context.query()?,
            None,
            None,
        )
        .await
    }
    pub async fn download_info_in(
        &self,
        id: Uuid,
        context: &AccessContext,
    ) -> Result<DownloadInfo> {
        parse(
            self.request(
                Method::GET,
                &format!("api/v1/assets/{id}/download"),
                &context.query()?,
                None,
                None,
            )
            .await?,
        )
    }
    /// Resolve through the requested resource's authorization, including shortcuts.
    pub async fn resource_download_info(&self, org: Uuid, id: Uuid) -> Result<DownloadInfo> {
        let detail = self.resource(org, id).await?;
        let asset = detail.asset.ok_or_else(|| {
            Error::new("NOT_DOWNLOADABLE", "This resource has no downloadable file")
        })?;
        let asset_id = asset
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(Error::protocol)?;
        let info = self
            .download_info_in(
                asset_id,
                &AccessContext {
                    org_id: org,
                    dentry_id: Some(id),
                    canvas_space_id: None,
                },
            )
            .await?;
        info.check_revision(&asset)?;
        Ok(info)
    }
    /// Single attempt; replay this exact request/key, not a fresh path resolution.
    pub async fn mutate_resources(
        &self,
        mutation: &ResourceMutation,
        key: &str,
        expected_user: Uuid,
    ) -> Result<ResourceBatch> {
        mutation.validate()?;
        if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (33..=126).contains(&b)) {
            return Err(Error::new(
                "INVALID_INPUT",
                "Idempotency key must contain 1..128 printable ASCII characters without spaces",
            ));
        }
        let value = self
            .expecting_user(expected_user)
            .request(
                Method::POST,
                "api/v1/resources/batch",
                &[],
                Some(json!(mutation)),
                Some(key),
            )
            .await?;
        let batch: ResourceBatch = parse(value)?;
        let expected: std::collections::HashSet<_> = mutation.ids.iter().copied().collect();
        let received: std::collections::HashSet<_> =
            batch.items.iter().map(|item| item.id).collect();
        if expected != received
            || batch.items.len() != expected.len()
            || batch.items.iter().any(|item| {
                !matches!(item.outcome.as_str(), "succeeded" | "failed")
                    || (item.outcome == "failed") != item.error.is_some()
            })
        {
            return Err(Error::protocol());
        }
        Ok(batch)
    }
    /// The current folder endpoint has no idempotency contract; do not blindly retry.
    pub async fn create_folder(
        &self,
        org: Uuid,
        parent: Option<Uuid>,
        name: &str,
    ) -> Result<ResourceItem> {
        if name.trim().is_empty() || name.chars().count() > 255 {
            return Err(Error::new(
                "INVALID_INPUT",
                "Folder name must contain 1..255 characters",
            ));
        }
        let mut value = self
            .request(
                Method::POST,
                "versa/folder/create",
                &[],
                Some(json!({"org_id":org,"parent_id":parent,"name":name})),
                None,
            )
            .await?;
        let folder: ResourceItem = parse(value["dentry"].take())?;
        if !folder.is_folder() || folder.parent_id != parent {
            return Err(Error::protocol());
        }
        Ok(folder)
    }
    pub async fn resource_trash(
        &self,
        org: Uuid,
        scope: Option<Uuid>,
    ) -> Result<Vec<ResourceItem>> {
        let mut value = self
            .request(
                Method::POST,
                "versa/trash/list",
                &[],
                Some(json!({"org_id":org,"scope_dentry_id":scope})),
                None,
            )
            .await?;
        parse(value["items"].take())
    }
}
