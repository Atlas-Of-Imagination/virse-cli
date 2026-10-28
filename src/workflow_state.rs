use crate::storage::{Store, digest};
use serde_json::{Value, json};
use uuid::Uuid;
use virse_api::{Client, Error, Result};

pub async fn user(client: &Client) -> Result<Uuid> {
    client.me().await?["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Identity has no user ID"))
}
pub fn key(input: &Option<String>) -> Result<String> {
    let key = input.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(Error::new(
            "INVALID_INPUT",
            "Key requires 1..128 printable ASCII characters without whitespace",
        ));
    }
    Ok(key)
}
pub fn freeze(
    store: &Store,
    scope: &Value,
    intent: &Value,
    request: Option<Value>,
) -> Result<Option<Value>> {
    let name = format!("workflow-{}.json", digest(&scope.to_string()));
    let _lock = store.lock()?;
    let old: Value = store.read(&name)?;
    if !old.is_null() {
        if old["intent"] != *intent {
            return Err(Error::new(
                "INVALID_INPUT",
                "This key belongs to another command; repeat the original intent",
            ));
        }
        return Ok(Some(old["request"].clone()));
    }
    if let Some(request) = request {
        store.save(
            &name,
            &json!({"scope":scope,"intent":intent,"request":request}),
        )?;
        return Ok(Some(request));
    }
    Ok(None)
}
pub fn uncertain(mut error: Error, message: &str, key: Option<&str>) -> Error {
    if matches!(
        error.code.as_str(),
        "CANVAS_HTTP_COMMANDS_NOT_ENABLED"
            | "GENERATION_TASKS_NOT_ENABLED"
            | "GENERATION_ADMISSION_PAUSED"
    ) {
        if let Some(key) = key {
            error.idempotency_key = Some(key.into());
        }
        return error;
    }
    if error.retryable
        || error.status.is_some_and(|s| s >= 500 || s == 408)
        || matches!(
            error.code.as_str(),
            "INVALID_RESPONSE" | "internal" | "idempotency_in_progress"
        )
    {
        error.details = Some(Box::new(
            json!({"cause":error.code,"server_details":error.details}),
        ));
        error.code = "WORKFLOW_UNCERTAIN".into();
        error.retryable = key.is_some();
        error.message = message.into();
    }
    if let Some(key) = key {
        error.idempotency_key = Some(key.into());
    }
    error
}
pub async fn json_file(path: &std::path::Path) -> Result<Value> {
    use std::io::Read;
    let file = crate::resource_upload::open_file(path, false, false)?;
    let mut bytes = vec![];
    file.take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(crate::storage::io_error)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(Error::new("INVALID_INPUT", "JSON input exceeds 2 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::new("INVALID_INPUT", "Invalid JSON file"))
}
pub fn revisions(entity: &Value) -> Result<std::collections::BTreeMap<String, u64>> {
    let mut rev: std::collections::BTreeMap<String, u64> =
        serde_json::from_value(entity.get("revisions").cloned().unwrap_or(Value::Null))
            .map_err(|_| Error::new("INVALID_RESPONSE", "Missing server revisions"))?;
    if let Some(name) = entity.get("name_revision").and_then(Value::as_u64) {
        rev.insert("name".into(), name);
    }
    if rev.is_empty() {
        return Err(Error::new(
            "INVALID_RESPONSE",
            "Server has no revisions for this entity",
        ));
    }
    Ok(rev)
}

pub fn exclusive(store: &Store, scope: &Value) -> Result<std::fs::File> {
    {
        let _lock = store.lock()?;
    }
    let file = crate::resource_upload::open_file(
        &store
            .root
            .join(format!("workflow-lock-{}.lock", digest(&scope.to_string()))),
        true,
        true,
    )?;
    file.try_lock()
        .map_err(|_| Error::new("WORKFLOW_BUSY", "Another process is running this operation"))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_name_revision_is_taken_from_same_server_snapshot() {
        let entity = json!({"name_revision":7,"revisions":{"layout":2,"content":3}});
        let revisions = revisions(&entity).unwrap();
        assert_eq!(revisions["name"], 7);
        assert_eq!(revisions["layout"], 2);
    }
}
