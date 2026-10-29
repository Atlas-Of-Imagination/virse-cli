//! Existing aesthetic authoring operations; no filesystem or conversation orchestration.
use crate::{Client, Error, Result};
use reqwest::Method;
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub enum AestheticOperation {
    Create,
    Details,
    SaveResource,
    References,
    TestCanvas,
    DeleteTestCanvas,
    StartDraft,
    Draft,
    Context,
    Revise,
    SaveDraft,
    Restore,
    Snapshot,
}
impl Client {
    pub async fn aesthetic(
        &self,
        operation: AestheticOperation,
        id: Option<Uuid>,
        body: Value,
    ) -> Result<Value> {
        use AestheticOperation::*;
        let identifier = || {
            id.map(|v| v.to_string())
                .ok_or_else(|| Error::new("INVALID_INPUT", "This operation requires an ID"))
        };
        let (method, path) = match operation {
            Create => (Method::POST, "".into()),
            Details => (Method::POST, "/details".into()),
            SaveResource => (Method::POST, "/save".into()),
            References => (Method::POST, "/references".into()),
            TestCanvas => (Method::POST, "/tuning/test-canvases".into()),
            DeleteTestCanvas => (
                Method::POST,
                format!("/tuning/test-canvases/{}/delete", identifier()?),
            ),
            StartDraft => (Method::POST, "/tuning/sessions".into()),
            Draft | Context | Revise | SaveDraft | Restore | Snapshot => {
                let action = match operation {
                    Draft => "",
                    Context => "/context",
                    Revise => "/revise",
                    SaveDraft => "/save",
                    Restore => "/restore",
                    Snapshot => "/snapshot",
                    _ => unreachable!(),
                };
                (
                    if matches!(operation, Draft | Context) {
                        Method::GET
                    } else {
                        Method::POST
                    },
                    format!("/tuning/sessions/{}{action}", identifier()?),
                )
            }
        };
        let data = if method == Method::GET {
            None
        } else {
            Some(body)
        };
        let value = self
            .request(method, &format!("v2/aesthetics{path}"), &[], data, None)
            .await?;
        if value.get("update_status") == Some(&Value::Bool(false))
            || value
                .get("status")
                .is_some_and(|s| s.as_i64() == Some(0) || s.as_bool() == Some(false))
        {
            return Err(Error::new(
                "AESTHETIC_REQUEST_REJECTED",
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Aesthetic request rejected"),
            ));
        }
        Ok(value)
    }
    pub async fn start_aesthetic_training(
        &self,
        canvas: Uuid,
        org: Uuid,
        request: Value,
        key: &str,
    ) -> Result<Value> {
        if key.is_empty() || key.len() > 200 {
            return Err(Error::new("INVALID_INPUT", "Invalid idempotency key"));
        }
        self.request(
            Method::POST,
            &format!("v2/aesthetics/canvases/{canvas}/training-runs"),
            &[],
            Some(serde_json::json!({"org_id":org,"request":request})),
            Some(key),
        )
        .await
    }
}
