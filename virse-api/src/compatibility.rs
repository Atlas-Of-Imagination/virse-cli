use crate::{Client, Error, Result};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

pub const CONTRACT_VERSION: u32 = 1;
pub(crate) fn is_upgrade_response(
    response: &tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
) -> bool {
    response.status().as_u16() == 409
        && response
            .body()
            .as_ref()
            .and_then(|b| serde_json::from_slice::<Value>(b).ok())
            .is_some_and(|v| {
                v["code"] == "CLIENT_UPGRADE_REQUIRED"
                    || v["error"]["code"] == "CLIENT_UPGRADE_REQUIRED"
            })
}
/// 发行检查不带业务 Token，不跟随重定向，失败不影响业务。
pub async fn latest_cli_version() -> Option<String> {
    let check = async {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("virse-cli-update")
            .build()
            .ok()?;
        let response = http
            .get("https://api.github.com/repos/Atlas-Of-Imagination/virse-cli/releases/latest")
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let value: Value = response.json().await.ok()?;
        if value["draft"] != false || value["prerelease"] != false {
            return None;
        }
        value["tag_name"].as_str().map(str::to_owned)
    };
    tokio::time::timeout(Duration::from_secs(2), check)
        .await
        .ok()
        .flatten()
}
pub fn upgrade_error(minimum: Option<u64>) -> Error {
    let mut error = Error::new(
        "CLIENT_UPGRADE_REQUIRED",
        "Please update Virse CLI to continue.",
    );
    error.status = Some(409);
    error.details = Some(Box::new(json!({"minimum_contract_version": minimum})));
    error
}
pub(crate) fn check_text(text: &str) -> Result<()> {
    if let Ok(value) = serde_json::from_str::<Value>(text)
        && (value["code"] == "CLIENT_UPGRADE_REQUIRED"
            || value["type"] == "client_upgrade_required")
    {
        return Err(upgrade_error(value["minimum_contract_version"].as_u64()));
    }
    Ok(())
}
pub(crate) fn check_close(frame: Option<&CloseFrame>) -> Result<()> {
    if frame.is_some_and(|f| u16::from(f.code) == 4409) {
        return Err(upgrade_error(None));
    }
    Ok(())
}
impl Client {
    pub(crate) async fn ensure_compatibility(&self) -> Result<()> {
        if self.cli_contract.is_some() {
            self.compatibility_checked
                .get_or_try_init(|| self.check_compatibility())
                .await?;
        }
        Ok(())
    }
    pub async fn check_compatibility(&self) -> Result<()> {
        let checked = async {
            let response = self
                .http
                .get(self.url("client-compatibility")?)
                .send()
                .await
                .map_err(|_| Error::transport())?;
            if !response.status().is_success() {
                return Err(Error::protocol());
            }
            let value: Value = response.json().await.map_err(|_| Error::protocol())?;
            let minimum = value["minimum_contract_version"]
                .as_u64()
                .filter(|v| *v > 0 && *v <= 999_999_999)
                .ok_or_else(Error::protocol)?;
            if !value["require_web_version"].is_boolean()
                || value["policy_id"]
                    .as_str()
                    .is_none_or(|s| s.is_empty() || s.len() > 128)
            {
                return Err(Error::protocol());
            }
            Ok(minimum)
        };
        let minimum = tokio::time::timeout(Duration::from_secs(5), checked)
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| {
                Error::new(
                    "COMPATIBILITY_CHECK_FAILED",
                    "Cannot verify backend compatibility; no business request was sent.",
                )
            })?;
        if self.cli_contract.is_some_and(|v| u64::from(v) < minimum) {
            return Err(upgrade_error(Some(minimum)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_upgrade_is_terminal() {
        let error =
            check_text(r#"{"type":"client_upgrade_required","minimum_contract_version":2}"#)
                .unwrap_err();
        assert_eq!(error.code, "CLIENT_UPGRADE_REQUIRED");
        assert!(!error.retryable);
        let frame = CloseFrame {
            code: 4409.into(),
            reason: "upgrade".into(),
        };
        assert!(check_close(Some(&frame)).is_err());
        assert!(check_text(r#"{"type":"pong"}"#).is_ok());
    }
}
