use crate::tool_types::{invalid, validate_locale, validate_tool_id};
use crate::{
    Client, Error, Result, ToolIdentity, ToolListOptions, ToolMutation, ToolSchema, ToolScope,
    ToolVersions, ToolView,
};
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

impl Client {
    pub async fn tools(&self, view: ToolView, options: &ToolListOptions) -> Result<Value> {
        validate_locale(&options.locale)?;
        if options.limit.is_some_and(|n| n == 0 || n > 500) {
            return Err(invalid("Tool list limit must be between 1 and 500"));
        }
        if view != ToolView::Created && (options.limit.is_some() || options.offset.is_some()) {
            return Err(invalid("Pagination is only supported for --view created"));
        }
        if view != ToolView::Store && options.category_id.is_some() {
            return Err(invalid("Category filtering requires --view store"));
        }
        self.request(
            Method::POST,
            &format!("skills/{}/list", view.as_str()),
            &[],
            Some(serde_json::to_value(options).map_err(|_| Error::protocol())?),
            None,
        )
        .await
    }
    pub async fn tool(&self, id: &str, org: Option<Uuid>, locale: &str) -> Result<Value> {
        validate_tool_id(id)?;
        validate_locale(locale)?;
        let missing = match self
            .request(
                Method::POST,
                "skills/store/detail",
                &[],
                Some(json!({"skillId":id,"organizationId":org,"locale":locale})),
                None,
            )
            .await
        {
            Ok(value) => return Ok(value),
            Err(error) if error.status == Some(404) && error.code == "SKILL_NOT_FOUND" => error,
            Err(error) => return Err(error),
        };
        // Unpublished author drafts are absent from store/detail. The created endpoint is
        // scoped by both current account and organization; never search another workspace.
        let mut offset = 0u32;
        loop {
            let page = self
                .tools(
                    ToolView::Created,
                    &ToolListOptions {
                        organization_id: org,
                        locale: locale.to_owned(),
                        limit: Some(100),
                        offset: Some(offset),
                        ..Default::default()
                    },
                )
                .await?;
            let skills = page["skills"].as_array().ok_or_else(Error::protocol)?;
            let total = page["total"].as_u64().ok_or_else(Error::protocol)?;
            if let Some(skill) = skills.iter().find(|skill| skill["id"].as_str() == Some(id)) {
                return Ok(json!({"skill":skill}));
            }
            offset = offset
                .checked_add(u32::try_from(skills.len()).map_err(|_| Error::protocol())?)
                .ok_or_else(Error::protocol)?;
            if u64::from(offset) >= total {
                return Err(missing);
            }
            // An incomplete listing must not become a false not-found result.
            if skills.is_empty() {
                return Err(Error::protocol());
            }
        }
    }
    /// Author management: formal releases are distinct from immutable execution artifacts.
    pub async fn tool_versions(&self, id: &str, org: Option<Uuid>) -> Result<ToolVersions> {
        validate_tool_id(id)?;
        let value = self
            .request(
                Method::POST,
                &format!("skills/{id}/versions"),
                &[],
                Some(json!({"skillId":id,"organizationId":org})),
                None,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| Error::protocol())
    }
    /// Abandon only the specified author draft. Published releases are unaffected.
    /// Single attempt: callers must reconcile an unknown outcome before retrying.
    pub async fn abandon_tool_draft(
        &self,
        id: &str,
        revision_id: &str,
        org: Option<Uuid>,
    ) -> Result<Value> {
        validate_tool_id(id)?;
        validate_tool_id(revision_id).map_err(|_| invalid("Invalid draft revision ID"))?;
        self.request(
            Method::POST,
            &format!("skills/{id}/draft/abandon"),
            &[],
            Some(json!({"skillId":id,"revisionId":revision_id,"organizationId":org})),
            None,
        )
        .await
    }
    pub async fn set_tool_scope(
        &self,
        id: &str,
        org: Option<Uuid>,
        scope: ToolScope,
    ) -> Result<Value> {
        validate_tool_id(id)?;
        if scope == ToolScope::Team && org.is_none() {
            return Err(invalid("Team scope requires an organization"));
        }
        self.request(
            Method::POST,
            "skills/creator/update",
            &[],
            Some(json!({"skillId":id,"organizationId":org,"accessScope":scope})),
            None,
        )
        .await
    }
    /// One mutation attempt. Caller must reconcile unknown outcomes instead of blindly retrying.
    pub async fn mutate_tool(
        &self,
        id: &str,
        org: Option<Uuid>,
        action: ToolMutation,
    ) -> Result<Value> {
        validate_tool_id(id)?;
        let mut body = json!({"skillId":id,"organizationId":org});
        let path = match action {
            ToolMutation::Add => "skills/save",
            ToolMutation::Remove => "skills/remove",
            ToolMutation::Enable => "skills/enable",
            ToolMutation::Disable => "skills/disable",
            ToolMutation::Star | ToolMutation::Unstar => {
                body["starred"] = json!(matches!(action, ToolMutation::Star));
                "skills/set-star"
            }
            ToolMutation::Delete => "skills/creator/delete",
        };
        self.request(Method::POST, path, &[], Some(body), None)
            .await
    }
    /// Formal version strings are resolved by the server, never by the owner-only versions API.
    /// Always return the schema of the immutable artifact that the caller must submit.
    pub async fn tool_schema(
        &self,
        canvas: Uuid,
        org: Uuid,
        tool: &ToolIdentity,
        locale: &str,
    ) -> Result<ToolSchema> {
        tool.validate()?;
        validate_locale(locale)?;
        let mut query = vec![
            ("canvas_id", canvas.to_string()),
            ("org_id", org.to_string()),
            ("locale", locale.to_owned()),
        ];
        if let Some(id) = &tool.version_id {
            query.push(("version_id", id.clone()));
        }
        if let Some(version) = &tool.version {
            query.push(("version", version.clone()));
        }
        let path = format!("v2/skills/{}/input-schema", tool.skill_id);
        let value = self.request(Method::GET, &path, &query, None, None).await?;
        let mut schema: ToolSchema =
            serde_json::from_value(value).map_err(|_| Error::protocol())?;
        if schema.skill_id != tool.skill_id {
            return Err(Error::protocol());
        }
        let pinned = schema.pinned_identity()?;
        if let Some(expected) = &tool.version_id {
            if pinned.version_id.as_ref() != Some(expected) {
                return Err(Error::protocol());
            }
        } else {
            // The live metadata can differ from its current artifact's snapshot. Re-read pinned.
            query.retain(|(key, _)| *key != "version");
            query.push((
                "version_id",
                pinned.version_id.clone().ok_or_else(Error::protocol)?,
            ));
            let value = self.request(Method::GET, &path, &query, None, None).await?;
            schema = serde_json::from_value(value).map_err(|_| Error::protocol())?;
            if schema.skill_id != tool.skill_id || schema.version_id != pinned.version_id {
                return Err(Error::protocol());
            }
        }
        Ok(schema)
    }
}
