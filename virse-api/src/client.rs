use crate::{DownloadInfo, Error, Result, SubmitTask, Task};
use reqwest::{
    Method,
    header::{AUTHORIZATION, HeaderValue},
};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct Client {
    pub(crate) base: Url,
    pub(crate) authorization: HeaderValue,
    pub(crate) http: reqwest::Client,
    token: String,
    expected_user: Option<Uuid>,
}

pub fn normalize_backend(value: &str) -> Result<String> {
    let mut url =
        Url::parse(value).map_err(|_| Error::new("INVALID_INPUT", "Invalid backend URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(Error::new(
            "INVALID_INPUT",
            "Backend must be an HTTP(S) origin without credentials, query or path",
        ));
    }
    url.set_path("/");
    Ok(url.to_string())
}

impl Client {
    pub fn new(backend: &str, token: &str) -> Result<Self> {
        let base = Url::parse(&normalize_backend(backend)?).map_err(|_| Error::protocol())?;
        if !token.starts_with("virse_sk_") || token.trim() != token {
            return Err(Error::new(
                "AUTH_REQUIRED",
                "A virse_sk_ API token is required",
            ));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| Error::new("AUTH_REQUIRED", "Invalid API token"))?;
        authorization.set_sensitive(true);
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .user_agent(concat!("virse-api/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::transport())?;
        Ok(Self {
            base,
            authorization,
            http,
            token: token.into(),
            expected_user: None,
        })
    }

    pub(crate) fn expecting_user(&self, user: Uuid) -> Self {
        let mut client = self.clone();
        client.expected_user = Some(user);
        client
    }
    pub(crate) fn url(&self, path: &str) -> Result<Url> {
        self.base
            .join(path)
            .map_err(|_| Error::new("INVALID_INPUT", "Invalid API path"))
    }
    pub(crate) fn scrub(&self, mut error: Error) -> Error {
        error.message = error.message.replace(&self.token, "[REDACTED]");
        error.code = error.code.replace(&self.token, "[REDACTED]");
        error.request_id = error
            .request_id
            .map(|s| s.replace(&self.token, "[REDACTED]"));
        if let Some(details) = error.details.take() {
            let sanitized = details.to_string().replace(&self.token, "[REDACTED]");
            error.details = serde_json::from_str(&sanitized).ok().map(Box::new);
        }
        error
    }
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
        key: Option<&str>,
    ) -> Result<Value> {
        let empty_write = matches!(
            method,
            Method::POST | Method::PUT | Method::PATCH | Method::DELETE
        );
        let mut request = self
            .http
            .request(method, self.url(path)?)
            .header(AUTHORIZATION, self.authorization.clone())
            .query(query)
            .timeout(Duration::from_secs(120));
        if let Some(user) = self.expected_user {
            request = request.header("X-Virse-Expected-User", user.to_string());
        }
        if let Some(body) = body {
            request = request.json(&body);
        } else if empty_write {
            // Some gateways reject a body-less POST/DELETE without an explicit length.
            request = request.header("Content-Length", "0").body("");
        }
        if let Some(key) = key {
            if key.is_empty() || key.len() > 200 || HeaderValue::from_str(key).is_err() {
                return Err(Error::new("INVALID_INPUT", "Invalid idempotency key"));
            }
            request = request.header("Idempotency-Key", key);
        }
        let response = request.send().await.map_err(|_| Error::transport())?;
        self.read_json_response(response).await
    }
    pub(crate) async fn multipart_request(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
        key: Option<&str>,
    ) -> Result<Value> {
        self.multipart_request_method(Method::POST, path, form, key)
            .await
    }
    pub(crate) async fn multipart_request_method(
        &self,
        method: Method,
        path: &str,
        form: reqwest::multipart::Form,
        key: Option<&str>,
    ) -> Result<Value> {
        let mut request = self
            .http
            .request(method, self.url(path)?)
            .header(AUTHORIZATION, self.authorization.clone())
            .timeout(Duration::from_secs(300))
            .multipart(form);
        if let Some(user) = self.expected_user {
            request = request.header("X-Virse-Expected-User", user.to_string());
        }
        if let Some(key) = key {
            if key.is_empty() || key.len() > 200 || HeaderValue::from_str(key).is_err() {
                return Err(Error::new("INVALID_INPUT", "Invalid idempotency key"));
            }
            request = request.header("Idempotency-Key", key);
        }
        let response = request.send().await.map_err(|_| Error::transport())?;
        self.read_json_response(response).await
    }
    pub(crate) async fn read_json_response(&self, response: reqwest::Response) -> Result<Value> {
        let status = response.status().as_u16();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = response.json::<Value>().await.map_err(|_| Error {
            status: Some(status),
            request_id: request_id.clone(),
            retryable: status >= 500,
            ..Error::protocol()
        })?;
        decode(status, request_id, body).map_err(|e| self.scrub(e))
    }
    pub async fn me(&self) -> Result<Value> {
        self.request(Method::GET, "auth/me", &[], None, None).await
    }
    /// Uses the permission-aware resource directory, not the legacy global canvas list.
    pub async fn canvases(&self, org: Uuid, limit: u16, cursor: Option<&str>) -> Result<Value> {
        self.request(Method::POST, "api/v1/resources/query", &[], Some(json!({"context":{"org_id":org},"kinds":["canvas"],"recursive":true,"limit":limit,"cursor":cursor})), None).await
    }
    pub async fn canvas(&self, canvas: Uuid) -> Result<Value> {
        self.request(
            Method::GET,
            &format!("canvas/projects/{canvas}"),
            &[("include", "elements".into()), ("expand", "none".into())],
            None,
            None,
        )
        .await
    }
    pub async fn operations(&self, canvas: Uuid) -> Result<Value> {
        self.request(
            Method::GET,
            &format!("v2/canvas/projects/{canvas}/generations/operations"),
            &[],
            None,
            None,
        )
        .await
    }
    /// A single attempt. Callers control retries and MUST reuse the same key and body.
    pub async fn submit(&self, canvas: Uuid, body: &SubmitTask, key: &str) -> Result<Task> {
        body.validate()?;
        let value = self
            .request(
                Method::POST,
                &format!("v2/canvas/projects/{canvas}/generations"),
                &[],
                Some(serde_json::to_value(body).map_err(|_| Error::protocol())?),
                Some(key),
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::protocol())
    }
    pub async fn task(&self, canvas: Uuid, task: Uuid) -> Result<Task> {
        let value = self
            .request(
                Method::GET,
                &format!("v2/canvas/projects/{canvas}/generations/{task}"),
                &[],
                None,
                None,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::protocol())
    }
    pub async fn cancel(&self, canvas: Uuid, task: Uuid) -> Result<Task> {
        let value = self
            .request(
                Method::POST,
                &format!("v2/canvas/projects/{canvas}/generations/{task}/cancel"),
                &[],
                None,
                None,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::protocol())
    }
    pub async fn asset(&self, asset: Uuid, org: Uuid, canvas: Uuid) -> Result<Value> {
        self.request(
            Method::GET,
            &format!("api/v1/assets/{asset}"),
            &[
                ("org_id", org.to_string()),
                ("canvas_space_id", canvas.to_string()),
            ],
            None,
            None,
        )
        .await
    }
    pub async fn download_info(
        &self,
        asset: Uuid,
        org: Uuid,
        canvas: Uuid,
    ) -> Result<DownloadInfo> {
        let value = self
            .request(
                Method::GET,
                &format!("api/v1/assets/{asset}/download"),
                &[
                    ("org_id", org.to_string()),
                    ("canvas_space_id", canvas.to_string()),
                ],
                None,
                None,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::protocol())
    }
    /// Returns a streaming response. No token is attached to external URLs, even after redirects.
    pub async fn download(&self, info: &DownloadInfo) -> Result<reqwest::Response> {
        self.download_range(info, None, None).await
    }
    /// Range response may be 206 or a full 200 after If-Range mismatch; caller must not blindly append.
    pub async fn download_range(
        &self,
        info: &DownloadInfo,
        start: Option<u64>,
        if_range: Option<&str>,
    ) -> Result<reqwest::Response> {
        if !matches!(
            info.delivery.as_str(),
            "authenticated_stream" | "signed_url" | "external_url"
        ) {
            return Err(Error::protocol());
        }
        let mut url = self.base.join(&info.url).map_err(|_| Error::protocol())?;
        let mut authenticated = info.delivery == "authenticated_stream";
        if authenticated && url.origin() != self.base.origin() {
            return Err(Error::new(
                "UNTRUSTED_DOWNLOAD",
                "Authenticated download URL must use the backend origin",
            ));
        }
        for _ in 0..6 {
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(Error::new("UNTRUSTED_DOWNLOAD", "Invalid download URL"));
            }
            let mut request = self.http.get(url.clone());
            if let Some(start) = start {
                request = request.header("Range", format!("bytes={start}-"));
            }
            if let Some(value) = if_range {
                let header = HeaderValue::from_str(value)
                    .map_err(|_| Error::new("INVALID_INPUT", "Invalid If-Range validator"))?;
                request = request.header("If-Range", header);
            }
            if authenticated {
                request = request.header(AUTHORIZATION, self.authorization.clone());
            }
            let response = request.send().await.map_err(|_| Error::transport())?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(Error::protocol)?;
                let next = url.join(location).map_err(|_| Error::protocol())?;
                if url.scheme() == "https" && next.scheme() != "https" {
                    return Err(Error::new("UNTRUSTED_DOWNLOAD", "HTTPS downgrade rejected"));
                }
                authenticated &= next.origin() == url.origin();
                url = next;
                continue;
            }
            if !response.status().is_success() {
                // Protected streams use the API error envelope. Preserve revision conflicts
                // without attempting to interpret arbitrary external-storage error bodies.
                if authenticated {
                    return match self.read_json_response(response).await {
                        Err(error) => Err(error),
                        Ok(_) => Err(Error::protocol()),
                    };
                }
                return Err(Error {
                    status: Some(response.status().as_u16()),
                    ..Error::new("DOWNLOAD_FAILED", "Download request failed")
                });
            }
            return Ok(response);
        }
        Err(Error::new("DOWNLOAD_FAILED", "Too many redirects"))
    }
}

fn decode(status: u16, request_id: Option<String>, mut body: Value) -> Result<Value> {
    if !(200..300).contains(&status) || body.get("status").and_then(Value::as_i64) == Some(0) {
        let detail = body
            .get("error")
            .or_else(|| body.get("detail"))
            .unwrap_or(&body);
        let code = detail
            .get("code")
            .and_then(Value::as_str)
            .or_else(|| detail.as_str())
            .unwrap_or("API_ERROR");
        let message = detail
            .get("message")
            .or_else(|| body.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("API request rejected");
        return Err(Error {
            code: code.into(),
            message: message.into(),
            status: Some(status),
            request_id: request_id.or_else(|| {
                body.get("request_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
            retryable: detail
                .get("retryable")
                .and_then(Value::as_bool)
                .unwrap_or(status == 429 || status >= 500),
            details: detail
                .get("details")
                .filter(|v| !v.is_null())
                .cloned()
                .map(Box::new),
            task_id: None,
            idempotency_key: None,
        });
    }
    // Legacy pack_response merges data at the top level; asset envelopes nest data.
    if body.get("status").and_then(Value::as_i64) == Some(1) {
        if let Some(map) = body.as_object_mut() {
            map.remove("status");
            map.remove("message");
        }
    } else if body.get("request_id").is_some() && body.get("data").is_some() {
        return Ok(body["data"].take());
    }
    Ok(body)
}
