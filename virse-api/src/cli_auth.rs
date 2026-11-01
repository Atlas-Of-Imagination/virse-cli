//! Unauthenticated browser-authorization transport. No local files or browser handling.
use crate::{Client, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthorizationProof {
    pub request_id: String,
    pub device_secret: String,
    pub code_verifier: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthorizationStart {
    pub request_id: String,
    pub device_secret: String,
    pub user_code: String,
    pub authorization_url: String,
    pub expires_in: u64,
    pub interval: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthorizedKey {
    pub access_token: String,
    pub key_id: String,
    pub user_id: String,
}
#[derive(Clone)]
pub struct CliAuthClient {
    base: url::Url,
    http: reqwest::Client,
}
impl CliAuthClient {
    pub fn new(backend: &str) -> Result<Self> {
        Ok(Self {
            base: url::Url::parse(&crate::normalize_backend(backend)?)
                .map_err(|_| Error::protocol())?,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|_| Error::transport())?,
        })
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let response = self
            .http
            .post(self.base.join(path).map_err(|_| Error::protocol())?)
            .json(&body)
            .send()
            .await
            .map_err(|_| Error::transport())?;
        let status = response.status().as_u16();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let decoded = response.json::<Value>().await;
        if !(200..300).contains(&status) {
            // 网关可能返回纯文本/HTML；先保留 HTTP 故障，不让 JSON 解析掩盖上游不可用。
            let fallback = match status {
                401 => "AUTH_REQUIRED",
                403 => "AUTHORIZATION_DENIED",
                429 => "AUTH_RATE_LIMITED",
                500..=599 => "AUTH_SERVICE_UNAVAILABLE",
                300..=399 => "AUTHORIZATION_REDIRECT_REJECTED",
                _ => "AUTHORIZATION_FAILED",
            };
            let code = decoded
                .as_ref()
                .ok()
                .and_then(|value| value["detail"]["code"].as_str())
                .filter(|code| {
                    !code.is_empty()
                        && code.len() <= 128
                        && code
                            .bytes()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
                })
                .unwrap_or(fallback);
            return Err(Error {
                status: Some(status),
                request_id,
                retryable: status == 429 || status >= 500,
                ..Error::new(
                    code,
                    format!(
                        "CLI authorization request failed (HTTP {status}){}",
                        if status >= 500 {
                            "; backend or upstream service unavailable"
                        } else {
                            ""
                        }
                    ),
                )
            });
        }
        decoded.map_err(|_| Error {
            status: Some(status),
            request_id,
            ..Error::new(
                "INVALID_RESPONSE",
                format!("Authorization endpoint returned a non-JSON response (HTTP {status})"),
            )
        })
    }

    pub async fn start(
        &self,
        mode: &str,
        challenge: &str,
        state: &str,
        redirect: Option<&str>,
        device_name: &str,
    ) -> Result<AuthorizationStart> {
        let value=self.post("auth/cli/start",json!({"mode":mode,"code_challenge":challenge,"state":state,"redirect_uri":redirect,"device_name":device_name})).await?;
        let start: AuthorizationStart =
            serde_json::from_value(value).map_err(|_| Error::protocol())?;
        let url = url::Url::parse(&start.authorization_url).map_err(|_| Error::protocol())?;
        if !matches!(url.scheme(), "https" | "http")
            || (url.scheme() == "http"
                && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
            || !url.username().is_empty()
            || url.password().is_some()
            || start.expires_in == 0
            || start.expires_in > 600
            || start.interval < 5
        {
            return Err(Error::protocol());
        }
        Ok(start)
    }
    pub async fn exchange(&self, proof: &AuthorizationProof) -> Result<Value> {
        self.post("auth/cli/token", json!(proof)).await
    }
    pub async fn acknowledge(&self, proof: &AuthorizationProof) -> Result<()> {
        self.post("auth/cli/ack", json!(proof)).await?;
        Ok(())
    }
}
impl Client {
    pub async fn revoke_api_token(&self, id: &str) -> Result<()> {
        let id: uuid::Uuid = id
            .parse()
            .map_err(|_| Error::new("INVALID_INPUT", "Invalid key ID"))?;
        self.request(
            reqwest::Method::DELETE,
            &format!("api-keys/{id}"),
            &[],
            None,
            None,
        )
        .await?;
        Ok(())
    }
}
