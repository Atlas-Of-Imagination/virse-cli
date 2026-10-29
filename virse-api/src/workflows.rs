//! Existing resource, graph, discussion and task APIs. Callers own write recovery.
use crate::{AssetUploadFile, Client, Error, Result, Task};
use reqwest::{
    Method,
    multipart::{Form, Part},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

fn invalid(message: &str) -> Error {
    Error::new("INVALID_INPUT", message)
}
pub(crate) fn operation_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(invalid(
            "Key requires 1..128 printable non-whitespace ASCII characters",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Element,
    Group,
    Edge,
}
impl EntityKind {
    pub fn singular(self) -> &'static str {
        match self {
            Self::Element => "element",
            Self::Group => "group",
            Self::Edge => "edge",
        }
    }
    pub fn plural(self) -> &'static str {
        match self {
            Self::Element => "elements",
            Self::Group => "groups",
            Self::Edge => "edges",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasCommand {
    pub action: String,
    pub id: Uuid,
    #[serde(default = "empty_object")]
    pub value: Value,
    #[serde(default)]
    pub expected_revisions: std::collections::BTreeMap<String, u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasTransaction {
    pub org_id: Uuid,
    pub operations: Vec<CanvasCommand>,
}
impl CanvasTransaction {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.operations.len()) {
            return Err(invalid("Canvas transaction requires 1..100 operations"));
        }
        for op in &self.operations {
            if !matches!(
                op.action.as_str(),
                "element.create"
                    | "element.update"
                    | "element.delete"
                    | "group.create"
                    | "group.update"
                    | "group.delete"
                    | "edge.create"
                    | "edge.update"
                    | "edge.delete"
            ) || !op.value.is_object()
            {
                return Err(invalid("Invalid canvas operation"));
            }
            if !op.action.ends_with(".create") && op.expected_revisions.is_empty() {
                return Err(invalid(
                    "Writes to existing entities require server revisions",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQuery {
    pub task_id: Option<Uuid>,
    pub element_id: Option<Uuid>,
    pub artifact_version_id: Option<Uuid>,
    pub status: Option<String>,
    pub operation: Option<String>,
    pub offset: u32,
    pub limit: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPage {
    pub tasks: Vec<Task>,
    pub offset: u32,
    pub limit: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyTrashPage {
    pub purged: Vec<Uuid>,
    pub failed: Vec<Value>,
    pub total: usize,
    pub truncated: bool,
}
impl Client {
    /// Idempotent set state, but no request receipt. No transport retries.
    pub async fn mark_resource(&self, org: Uuid, id: Uuid, mark: &str, on: bool) -> Result<Value> {
        if !matches!(mark, "pinned" | "quick_access") {
            return Err(invalid("Unknown resource mark"));
        }
        self.request(
            Method::POST,
            "versa/mark",
            &[],
            Some(json!({"org_id":org,"dentry_id":id,"mark_type":mark,"on":on})),
            None,
        )
        .await
    }
    pub async fn resource_favorites(&self, org: Uuid) -> Result<Value> {
        self.request(
            Method::POST,
            "versa/quick_access/list",
            &[],
            Some(json!({"org_id":org,"include_details":true})),
            None,
        )
        .await
    }
    pub async fn reorder_resources(
        &self,
        org: Uuid,
        parent: Option<Uuid>,
        ids: &[Uuid],
    ) -> Result<Value> {
        if ids.is_empty()
            || ids.len() > 500
            || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
        {
            return Err(invalid("Order requires 1..500 distinct resource IDs"));
        }
        self.request(
            Method::POST,
            "versa/reorder",
            &[],
            Some(json!({"org_id":org,"parent_id":parent,"ordered_ids":ids})),
            None,
        )
        .await
    }
    pub async fn hide_shared_resource(&self, org: Uuid, id: Uuid) -> Result<Value> {
        self.request(
            Method::POST,
            "versa/shared_with_me/remove",
            &[],
            Some(json!({"org_id":org,"dentry_id":id})),
            None,
        )
        .await
    }
    pub async fn save_canvas_resource(
        &self,
        org: Uuid,
        canvas: Uuid,
        element: Uuid,
        parent: Option<Uuid>,
        key: &str,
        user: Uuid,
    ) -> Result<Value> {
        operation_key(key)?;
        self.expecting_user(user).request(Method::POST,"versa/canvas/save_to_drive",&[],Some(json!({"org_id":org,"canvas_space_id":canvas,"element_id":element,"target_parent_id":parent})),Some(key)).await
    }
    pub async fn empty_trash_page(
        &self,
        org: Uuid,
        key: &str,
        user: Uuid,
    ) -> Result<EmptyTrashPage> {
        operation_key(key)?;
        let value = self
            .expecting_user(user)
            .request(
                Method::POST,
                "versa/trash/empty",
                &[],
                Some(json!({"org_id":org})),
                Some(key),
            )
            .await?;
        let page: EmptyTrashPage = serde_json::from_value(value).map_err(|_| Error::protocol())?;
        if page.total != page.purged.len() + page.failed.len() || page.total > 200 {
            return Err(Error::protocol());
        }
        Ok(page)
    }
    pub async fn canvas_entity(
        &self,
        canvas: Uuid,
        org: Uuid,
        kind: EntityKind,
        id: Uuid,
    ) -> Result<Value> {
        let value = self
            .request(
                Method::GET,
                &format!("v2/canvas/projects/{canvas}/{}/{id}", kind.plural()),
                &[("org_id", org.to_string())],
                None,
                None,
            )
            .await?;
        let entity = value
            .get(kind.singular())
            .ok_or_else(Error::protocol)?
            .clone();
        if entity
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            != Some(id)
        {
            return Err(Error::protocol());
        }
        Ok(entity)
    }
    pub async fn canvas_entities(
        &self,
        canvas: Uuid,
        org: Uuid,
        kind: EntityKind,
        offset: u32,
        limit: u16,
    ) -> Result<Value> {
        if !(1..=200).contains(&limit) {
            return Err(invalid("Page limit must be 1..200"));
        }
        self.request(
            Method::GET,
            &format!("v2/canvas/projects/{canvas}/{}", kind.plural()),
            &[
                ("org_id", org.to_string()),
                ("offset", offset.to_string()),
                ("limit", limit.to_string()),
            ],
            None,
            None,
        )
        .await
    }
    pub async fn canvas_transaction(
        &self,
        canvas: Uuid,
        body: &CanvasTransaction,
        key: &str,
        user: Uuid,
    ) -> Result<Value> {
        operation_key(key)?;
        body.validate()?;
        let result = self
            .expecting_user(user)
            .request(
                Method::POST,
                &format!("v2/canvas/projects/{canvas}/transactions"),
                &[],
                Some(json!(body)),
                Some(key),
            )
            .await?;
        let results = result
            .get("results")
            .and_then(Value::as_array)
            .ok_or_else(Error::protocol)?;
        if result
            .get("mutation_id")
            .and_then(Value::as_str)
            .is_none_or(|s| s.is_empty())
            || results.len() != body.operations.len()
            || results.iter().zip(&body.operations).any(|(row, op)| {
                row.get("id")
                    .and_then(Value::as_str)
                    .and_then(|s| Uuid::parse_str(s).ok())
                    != Some(op.id)
            })
        {
            return Err(Error::protocol());
        }
        Ok(result)
    }
    pub async fn canvas_paste(&self, canvas: Uuid, plan: &Value, key: &str) -> Result<Value> {
        operation_key(key)?;
        if !plan.is_object() {
            return Err(invalid("Paste plan must be an object"));
        }
        let mut body = plan.clone();
        body["idempotency_key"] = json!(key);
        self.request(
            Method::POST,
            &format!("canvas/projects/{canvas}/paste"),
            &[],
            Some(body),
            Some(key),
        )
        .await
    }
    pub async fn recently_deleted(&self, canvas: Uuid) -> Result<Value> {
        self.request(
            Method::GET,
            &format!("canvas/projects/{canvas}/recently-deleted"),
            &[],
            None,
            None,
        )
        .await
    }
    pub async fn restore_canvas_media(&self, canvas: Uuid, id: Uuid, key: &str) -> Result<Value> {
        operation_key(key)?;
        self.request(
            Method::POST,
            &format!("canvas/projects/{canvas}/recently-deleted/{id}/restore"),
            &[],
            Some(json!({"clientMutationId":key})),
            None,
        )
        .await
    }
    pub async fn generation_tasks(&self, canvas: Uuid, query: &TaskQuery) -> Result<TaskPage> {
        if !(1..=100).contains(&query.limit) {
            return Err(invalid("Task page limit must be 1..100"));
        }
        let mut params = vec![
            ("offset", query.offset.to_string()),
            ("limit", query.limit.to_string()),
        ];
        for (name, value) in [
            ("task_id", query.task_id),
            ("element_id", query.element_id),
            ("artifact_version_id", query.artifact_version_id),
        ] {
            if let Some(id) = value {
                params.push((name, id.to_string()));
            }
        }
        if let Some(status) = &query.status {
            params.push(("status", status.clone()));
        }
        if let Some(op) = &query.operation {
            params.push(("operation", op.clone()));
        }
        let value = self
            .request(
                Method::GET,
                &format!("v2/canvas/projects/{canvas}/generations"),
                &params,
                None,
                None,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::protocol())
    }
    pub async fn retry_generation(&self, canvas: Uuid, id: Uuid, key: &str) -> Result<Task> {
        operation_key(key)?;
        let value = self
            .request(
                Method::POST,
                &format!("v2/canvas/projects/{canvas}/generations/{id}/retry"),
                &[],
                None,
                Some(key),
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::protocol())
    }
    /// client_mutation_id is an echo identifier, not a durable idempotency guarantee.
    pub async fn discussions(&self, canvas: Uuid, status: &str) -> Result<Value> {
        if !matches!(status, "all" | "open" | "resolved") {
            return Err(invalid("Invalid discussion status"));
        }
        self.request(
            Method::GET,
            &format!("canvas/projects/{canvas}/discussions"),
            &[("status", status.into())],
            None,
            None,
        )
        .await
    }
    pub async fn discussion(&self, canvas: Uuid, id: Uuid) -> Result<Value> {
        self.request(
            Method::GET,
            &format!("canvas/projects/{canvas}/discussions/{id}"),
            &[],
            None,
            None,
        )
        .await
    }
    pub async fn create_discussion(&self, canvas: Uuid, body: &Value) -> Result<Value> {
        self.request(
            Method::POST,
            &format!("canvas/projects/{canvas}/discussions"),
            &[],
            Some(body.clone()),
            None,
        )
        .await
    }
    pub async fn update_discussion(&self, canvas: Uuid, id: Uuid, body: &Value) -> Result<Value> {
        self.request(
            Method::PATCH,
            &format!("canvas/projects/{canvas}/discussions/{id}"),
            &[],
            Some(body.clone()),
            None,
        )
        .await
    }
    pub async fn delete_discussion(&self, canvas: Uuid, id: Uuid) -> Result<Value> {
        self.request(
            Method::DELETE,
            &format!("canvas/projects/{canvas}/discussions/{id}"),
            &[],
            None,
            None,
        )
        .await
    }
    pub async fn mark_discussion_read(&self, canvas: Uuid, id: Uuid) -> Result<Value> {
        self.request(
            Method::POST,
            &format!("canvas/projects/{canvas}/discussions/{id}/read"),
            &[],
            None,
            None,
        )
        .await
    }
    pub async fn write_comment(
        &self,
        canvas: Uuid,
        discussion: Uuid,
        comment: Option<Uuid>,
        content: &str,
    ) -> Result<Value> {
        if content.trim().is_empty() || content.trim().chars().count() > 200 {
            return Err(invalid("Comment must contain 1..200 characters"));
        }
        let path = format!(
            "canvas/projects/{canvas}/discussions/{discussion}/comments{}",
            comment.map(|id| format!("/{id}")).unwrap_or_default()
        );
        self.request(
            if comment.is_some() {
                Method::PUT
            } else {
                Method::POST
            },
            &path,
            &[],
            Some(json!({"content":content})),
            None,
        )
        .await
    }
    pub async fn delete_comment(
        &self,
        canvas: Uuid,
        discussion: Uuid,
        comment: Uuid,
    ) -> Result<Value> {
        self.request(
            Method::DELETE,
            &format!("canvas/projects/{canvas}/discussions/{discussion}/comments/{comment}"),
            &[],
            None,
            None,
        )
        .await
    }
    pub async fn upload_tool_cover(&self, file: AssetUploadFile) -> Result<Value> {
        file.validate()?;
        if file.bytes.len() > 10 * 1024 * 1024 {
            return Err(invalid("Tool cover exceeds 10 MiB"));
        }
        let form = Form::new().part("cover", Part::bytes(file.bytes).file_name(file.filename));
        let result = self
            .multipart_request("skills/cover/upload", form, None)
            .await?;
        if result
            .get("coverUrl")
            .and_then(Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(Error::protocol());
        }
        Ok(result)
    }
}

impl Default for TaskQuery {
    fn default() -> Self {
        Self {
            task_id: None,
            element_id: None,
            artifact_version_id: None,
            status: None,
            operation: None,
            offset: 0,
            limit: 50,
        }
    }
}

fn empty_object() -> Value {
    json!({})
}
