use crate::{
    AccessContext, Asset, AssetPage, AssetQuery, AssetRepresentation, AssetUploadFile, Client,
    DownloadInfo, Error, ReplaceOptions, Result, UploadOptions, UploadResult,
};
use reqwest::{
    Method,
    multipart::{Form, Part},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use uuid::Uuid;

fn parse<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::protocol())
}

fn form(file: AssetUploadFile, options: &impl Serialize, key: &str) -> Result<Form> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(Error::new(
            "INVALID_INPUT",
            "Idempotency key must contain 1..128 printable ASCII characters without spaces",
        ));
    }
    file.validate()?;
    let options = serde_json::to_string(options).map_err(|_| Error::protocol())?;
    let mut part = Part::bytes(file.bytes).file_name(file.filename);
    if let Some(mime_type) = file.mime_type {
        part = part
            .mime_str(&mime_type)
            .map_err(|_| Error::new("INVALID_INPUT", "Invalid asset MIME type"))?;
    }
    Ok(Form::new().part("file", part).text("options", options))
}

impl Client {
    /// 单次提交；重试由调用方冻结文件、目标、options、身份与 key。
    pub async fn upload_asset(
        &self,
        file: AssetUploadFile,
        options: &UploadOptions,
        key: &str,
        expected_user: Uuid,
    ) -> Result<UploadResult> {
        options.validate()?;
        let form = form(file, options, key)?;
        let result: UploadResult = parse(
            self.expecting_user(expected_user)
                .multipart_request_method(Method::POST, "api/v1/assets", form, Some(key))
                .await?,
        )?;
        if result
            .resource
            .as_ref()
            .and_then(|resource| resource.asset_id)
            .is_some_and(|id| id != result.asset.id)
            || (!result.skipped && result.asset.content.revision == 0)
        {
            return Err(Error::protocol());
        }
        Ok(result)
    }

    /// 保持资产 ID；冲突原样返回，不重读、覆盖或自动重试。
    pub async fn replace_asset_content(
        &self,
        id: Uuid,
        file: AssetUploadFile,
        options: &ReplaceOptions,
        key: &str,
        expected_user: Uuid,
    ) -> Result<Asset> {
        options.validate()?;
        let form = form(file, options, key)?;
        let asset: Asset = parse(
            self.expecting_user(expected_user)
                .multipart_request_method(
                    Method::PUT,
                    &format!("api/v1/assets/{id}/content"),
                    form,
                    Some(key),
                )
                .await?,
        )?;
        if asset.id != id || asset.content.revision <= options.expected_revision {
            return Err(Error::protocol());
        }
        Ok(asset)
    }

    /// 解析个人组织或画布访问上下文；显式提示被拒绝时不尝试其他入口。
    pub async fn resolve_asset_context(
        &self,
        org_id: Option<Uuid>,
        canvas_space_id: Option<Uuid>,
    ) -> Result<AccessContext> {
        let mut query = vec![];
        if let Some(id) = org_id {
            query.push(("org_id", id.to_string()));
        }
        if let Some(id) = canvas_space_id {
            query.push(("canvas_space_id", id.to_string()));
        }
        let context: AccessContext = parse(
            self.request(Method::GET, "api/v1/asset-context", &query, None, None)
                .await?,
        )?;
        context.query()?;
        if org_id.is_some_and(|id| id != context.org_id)
            || context.canvas_space_id != canvas_space_id
            || context.dentry_id.is_some()
        {
            return Err(Error::protocol());
        }
        Ok(context)
    }

    pub async fn asset_detail(&self, id: Uuid, context: &AccessContext) -> Result<Asset> {
        let asset: Asset = parse(self.asset_in(id, context).await?)?;
        if asset.id != id {
            return Err(Error::protocol());
        }
        Ok(asset)
    }

    /// 返回快照页，sync_cursor 与该快照绑定；调用方负责分页。
    pub async fn assets(&self, query: &AssetQuery) -> Result<AssetPage> {
        query.validate()?;
        parse(
            self.request(
                Method::POST,
                "api/v1/assets/query",
                &[],
                Some(serde_json::to_value(query).map_err(|_| Error::protocol())?),
                None,
            )
            .await?,
        )
    }

    /// 获取展示描述；使用 download() 消费返回 URL，保留 representation 和版本参数。
    pub async fn presentation_info_in(
        &self,
        id: Uuid,
        context: &AccessContext,
        representation: AssetRepresentation,
    ) -> Result<DownloadInfo> {
        let mut query = context.query()?;
        query.push(("representation", representation.as_str().into()));
        parse(
            self.request(
                Method::GET,
                &format!("api/v1/assets/{id}/presentation"),
                &query,
                None,
                None,
            )
            .await?,
        )
    }
}

impl Client {
    /// Resolve through the requested resource/shortcut's authorization, never retry through another anchor.
    pub async fn resource_presentation_info(
        &self,
        org: Uuid,
        resource: Uuid,
        representation: AssetRepresentation,
    ) -> Result<DownloadInfo> {
        let detail = self.resource(org, resource).await?;
        let asset = detail
            .asset
            .ok_or_else(|| Error::new("NOT_PRESENTABLE", "Resource has no displayable asset"))?;
        let id = asset
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(Error::protocol)?;
        let info = self
            .presentation_info_in(
                id,
                &AccessContext {
                    org_id: org,
                    dentry_id: Some(resource),
                    canvas_space_id: None,
                },
                representation,
            )
            .await?;
        info.check_revision(&asset)?;
        Ok(info)
    }
}

impl Client {
    /// 按已知 ID 有界解析，返回逐项结果；原件/展示权限由服务端分别裁决。
    pub async fn resolve_assets(
        &self,
        context: &AccessContext,
        ids: &[uuid::Uuid],
        download: bool,
    ) -> crate::Result<serde_json::Value> {
        if ids.is_empty() || ids.len() > 100 {
            return Err(crate::Error::new(
                "INVALID_INPUT",
                "Resolve accepts 1..100 asset IDs",
            ));
        }
        let purposes = if download {
            vec!["presentation", "download"]
        } else {
            vec!["presentation"]
        };
        self.request(
            reqwest::Method::POST,
            "api/v1/assets/resolve",
            &[],
            Some(serde_json::json!({"context":context,"asset_ids":ids,"purposes":purposes})),
            None,
        )
        .await
    }
}
