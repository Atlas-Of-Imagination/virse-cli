//! Local tool authoring protocol. Callers supply bytes, identities and retry keys;
//! this module never reads files or environment variables.
use crate::{Client, Error, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{
    Method,
    multipart::{Form, Part},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const TOOL_PACKAGE_MAX_BYTES: usize = 20 * 1024 * 1024;

/// Server-issued immutable artifact plus opaque metadata CAS token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPackageBase {
    pub version_id: Uuid,
    pub metadata_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSource {
    pub skill: Value,
    pub source_metadata: Value,
    pub version_id: Uuid,
    pub release_id: Option<Uuid>,
    pub version: String,
    pub metadata_hash: String,
    pub package_checksum: String,
    pub package_file_name: String,
    #[serde(skip)]
    pub package: Vec<u8>,
}

fn metadata_fields(metadata: &Value) -> Result<&serde_json::Map<String, Value>> {
    let fields = metadata
        .as_object()
        .ok_or_else(|| Error::new("INVALID_INPUT", "Package metadata must be an object"))?;
    for (name, value) in fields {
        match name.as_str() {
            "name" | "description" | "detail" | "scenario" | "version" | "name_en"
            | "description_en" | "detail_en" | "cover_url" | "cover_input_url"
            | "case_public_link" => {
                if !value.is_string() {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        format!("Metadata {name} must be a string"),
                    ));
                }
            }
            "tags" | "scenes" | "category_ids" => {
                if value
                    .as_array()
                    .is_none_or(|v| v.iter().any(|x| !x.is_string()))
                {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        format!("Metadata {name} must be a string array"),
                    ));
                }
            }
            "input_schema" | "distribution_config" => {
                if !value.is_object() {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        format!("Metadata {name} must be an object"),
                    ));
                }
            }
            _ => {
                return Err(Error::new(
                    "INVALID_INPUT",
                    format!("Unknown or protected package metadata field: {name}"),
                ));
            }
        }
    }
    Ok(fields)
}
fn package_part(bytes: Vec<u8>) -> Result<Part> {
    if bytes.is_empty() || bytes.len() > TOOL_PACKAGE_MAX_BYTES {
        return Err(Error::new(
            "PACKAGE_SIZE_LIMIT",
            "ZIP must contain 1..20 MiB of bytes",
        ));
    }
    Part::bytes(bytes)
        .file_name("package.zip")
        .mime_str("application/zip")
        .map_err(|_| Error::new("INVALID_INPUT", "Invalid ZIP content type"))
}

impl Client {
    /// Single attempt; retry only with exactly the same bytes, fields and key.
    pub async fn tool_create_package(
        &self,
        org: Uuid,
        package: Vec<u8>,
        metadata: &Value,
        key: &str,
    ) -> Result<Value> {
        let mut form = Form::new()
            .part("package_file", package_part(package)?)
            .text("organization_id", org.to_string())
            .text("package_source", "uploaded_package");
        let fields = metadata_fields(metadata)?;
        for (name, value) in fields {
            form = form.text(
                name.clone(),
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            );
        }
        self.multipart_request("skills/creator/create", form, Some(key))
            .await
    }
    pub async fn tool_replace_package(
        &self,
        org: Uuid,
        skill: Uuid,
        base: &ToolPackageBase,
        package: Vec<u8>,
        metadata: &Value,
        key: &str,
    ) -> Result<Value> {
        let mut form = Form::new()
            .part("package_file", package_part(package)?)
            .text("organization_id", org.to_string())
            .text("skill_id", skill.to_string())
            .text("package_source", "uploaded_package")
            .text("expected_version_id", base.version_id.to_string())
            .text("expected_metadata_hash", base.metadata_hash.clone());
        for (name, value) in metadata_fields(metadata)? {
            if name != "version" {
                form = form.text(
                    name.clone(),
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string()),
                );
            }
        }
        self.multipart_request("skills/creator/replace", form, Some(key))
            .await
    }
    pub async fn tool_save_package_draft(
        &self,
        skill: Uuid,
        package: Vec<u8>,
        options: &Value,
        key: &str,
    ) -> Result<Value> {
        let form = Form::new()
            .part("package_file", package_part(package)?)
            .text("options", options.to_string());
        self.multipart_request(&format!("skills/{skill}/draft"), form, Some(key))
            .await
    }
    pub async fn tool_finalize_package(&self, body: &Value, key: &str) -> Result<Value> {
        self.request(
            Method::POST,
            "skills/creator/finalize",
            &[],
            Some(body.clone()),
            Some(key),
        )
        .await
    }
    pub async fn tool_validate_package_draft(&self, skill: Uuid, body: &Value) -> Result<Value> {
        self.request(
            Method::POST,
            &format!("skills/{skill}/draft/validate"),
            &[],
            Some(body.clone()),
            None,
        )
        .await
    }
    pub async fn tool_publish_package_draft(
        &self,
        skill: Uuid,
        body: &Value,
        key: &str,
    ) -> Result<Value> {
        self.request(
            Method::POST,
            &format!("skills/{skill}/draft/publish"),
            &[],
            Some(body.clone()),
            Some(key),
        )
        .await
    }
    /// Owner-only export. Top-level IDs describe exported source; skill.versionId
    /// and metadataHash describe the latest CAS base, including historical exports.
    pub async fn tool_source(
        &self,
        org: Uuid,
        skill: Uuid,
        release: Option<Uuid>,
    ) -> Result<ToolSource> {
        let mut body = json!({"skillId":skill,"organizationId":org});
        if let Some(id) = release {
            body["releaseId"] = json!(id);
        }
        let mut value = self
            .request(
                Method::POST,
                &format!("skills/{skill}/source"),
                &[],
                Some(body),
                None,
            )
            .await?;
        let encoded = value
            .get_mut("packageBase64")
            .map(Value::take)
            .ok_or_else(Error::protocol)?;
        let encoded = encoded.as_str().ok_or_else(Error::protocol)?;
        if encoded.len() > TOOL_PACKAGE_MAX_BYTES.div_ceil(3) * 4 {
            return Err(Error::new(
                "PACKAGE_SIZE_LIMIT",
                "Exported ZIP exceeds 20 MiB",
            ));
        }
        let package = STANDARD.decode(encoded).map_err(|_| Error::protocol())?;
        if package.is_empty() || package.len() > TOOL_PACKAGE_MAX_BYTES {
            return Err(Error::new(
                "PACKAGE_SIZE_LIMIT",
                "Exported ZIP size is invalid",
            ));
        }
        let mut source: ToolSource =
            serde_json::from_value(value).map_err(|_| Error::protocol())?;
        if source.skill.get("id").and_then(Value::as_str) != Some(skill.to_string().as_str())
            || release.is_some_and(|id| source.release_id != Some(id))
        {
            return Err(Error::new(
                "INVALID_RESPONSE",
                "Source identity does not match requested tool/release",
            ));
        }
        source.package = package;
        Ok(source)
    }
}
