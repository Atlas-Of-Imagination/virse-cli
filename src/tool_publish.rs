//! Durable authoring workflow. Each remote write uses a previously persisted key,
//! frozen package and CAS base. Receipts live outside the source directory.
use crate::tool_project::{self, Package, digest, io_error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;
use virse_api::{Client, Error, Result, ToolPackageBase, ToolSource};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringIdentity {
    pub backend: String,
    pub user: Uuid,
    pub org: Uuid,
}
impl AuthoringIdentity {
    pub fn new(backend: &str, user: Uuid, org: Uuid) -> Result<Self> {
        if user.is_nil() || org.is_nil() {
            return Err(Error::new(
                "INVALID_INPUT",
                "Authoring requires a concrete account and organization",
            ));
        }
        Ok(Self {
            backend: virse_api::normalize_backend(backend)?,
            user,
            org,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Binding {
    skill_id: Uuid,
    /// Exported source may be historical. It is never the optimistic-lock base.
    source_version_id: Uuid,
    source_release_id: Option<Uuid>,
    base_version_id: Uuid,
    metadata_hash: String,
    published: bool,
    access_scope: String,
    author_metadata: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    IdentityCreated,
    Built,
    Releasing,
    Uploaded,
    DraftSaved,
    Validated,
    Published,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    #[serde(default)]
    protocol: u32,
    id: Uuid,
    checksum: String,
    input_checksum: String,
    metadata: Value,
    config: Value,
    scope: Option<String>,
    base: Option<Binding>,
    phase: Phase,
    upload_key: String,
    publish_key: String,
    uploaded: Option<Value>,
    draft: Option<Value>,
    result: Option<Value>,
    #[serde(default)]
    rejected: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
struct State {
    format: u32,
    identity: AuthoringIdentity,
    directory: PathBuf,
    binding: Option<Binding>,
    receipt: Option<Receipt>,
    #[serde(default)]
    installation_pending: bool,
    #[serde(default)]
    pending_identity: Option<Value>,
}
struct Journal {
    root: PathBuf,
    state_path: PathBuf,
    _lock: File,
    state: State,
}
fn local_error(message: impl Into<String>) -> Error {
    Error::new("TOOL_RECEIPT_ERROR", message)
}
fn field_uuid(value: &Value, key: &str) -> Result<Uuid> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| {
            Error::new(
                "INVALID_RESPONSE",
                format!("Tool response is missing valid {key}"),
            )
        })
}
fn field_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::new(
                "INVALID_RESPONSE",
                format!("Tool response is missing {key}"),
            )
        })
}
fn binding(
    skill: &Value,
    source_version: Option<Uuid>,
    source_release: Option<Uuid>,
) -> Result<Binding> {
    let id = field_uuid(skill, "id")?;
    let version = field_uuid(skill, "versionId")?;
    let hash = field_string(skill, "metadataHash")?;
    let release = skill
        .get("releaseId")
        .filter(|v| !v.is_null())
        .map(|_| field_uuid(skill, "releaseId"))
        .transpose()?;
    let access_scope = field_string(skill, "accessScope")?;
    if !matches!(access_scope.as_str(), "personal" | "team") {
        return Err(Error::new(
            "INVALID_RESPONSE",
            "Unexpected tool accessScope",
        ));
    }
    Ok(Binding {
        skill_id: id,
        base_version_id: version,
        metadata_hash: hash,
        source_version_id: source_version.unwrap_or(version),
        source_release_id: source_release.or(release),
        published: release.is_some(),
        access_scope,
        author_metadata: skill.clone(),
    })
}
fn check_owner(skill: &Value, identity: &AuthoringIdentity) -> Result<()> {
    if field_uuid(skill, "creatorUserId")? != identity.user {
        return Err(Error::new(
            "SKILL_NOT_OWNER",
            "Tool source does not belong to the authenticated account",
        ));
    }
    Ok(())
}
fn secure_file(path: &Path, create: bool) -> Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    if create {
        options.write(true).create(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(
            rustix::fs::OFlags::NOFOLLOW.bits() as i32 | rustix::fs::OFlags::NONBLOCK.bits() as i32,
        );
    }
    let file = options.open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(local_error("Receipt paths must be regular files"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata().map_err(io_error)?.nlink() != 1 {
            return Err(local_error("Hard-linked receipt files are not allowed"));
        }
    }
    Ok(file)
}
fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    secure_file(path, false)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err(local_error("Receipt or staged package exceeds size limit"));
    }
    Ok(bytes)
}
fn atomic_save(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| local_error("Invalid receipt directory"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    temp.write_all(bytes).map_err(io_error)?;
    temp.as_file().sync_all().map_err(io_error)?;
    temp.persist(path).map_err(|e| io_error(e.error))?;
    #[cfg(unix)]
    File::open(parent)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)?;
    Ok(())
}
impl Journal {
    fn open(
        state_root: &Path,
        directory: &Path,
        identity: &AuthoringIdentity,
        existing: bool,
    ) -> Result<Self> {
        let directory = if existing {
            tool_project::open_directory(directory)?;
            fs::canonicalize(directory).map_err(io_error)?
        } else {
            let parent = directory
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            tool_project::open_directory(parent)?;
            fs::canonicalize(parent).map_err(io_error)?.join(
                directory
                    .file_name()
                    .ok_or_else(|| local_error("Invalid destination directory"))?,
            )
        };
        let requested_root = if state_root.is_absolute() {
            state_root.to_owned()
        } else {
            std::env::current_dir().map_err(io_error)?.join(state_root)
        };
        let ancestor = requested_root
            .ancestors()
            .find(|p| p.exists())
            .ok_or_else(|| local_error("Cannot locate receipt directory parent"))?;
        tool_project::open_directory(ancestor)?;
        let resolved_root = fs::canonicalize(ancestor).map_err(io_error)?.join(
            requested_root
                .strip_prefix(ancestor)
                .map_err(|_| local_error("Invalid receipt path"))?,
        );
        if resolved_root.starts_with(&directory) {
            return Err(Error::new(
                "TOOL_RECEIPT_LOCATION",
                "Authoring state directory must be outside the source project",
            ));
        }
        let root = resolved_root.join("tool-authoring");
        fs::create_dir_all(&root).map_err(io_error)?;
        tool_project::open_directory(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
        }
        let scope = digest(
            &serde_json::to_vec(&(identity, &directory))
                .map_err(|_| local_error("Cannot encode authoring identity"))?,
        );
        let lock = secure_file(&root.join(format!("{scope}.lock")), true)?;
        #[cfg(unix)]
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |_| {
                Error::new(
                    "TOOL_PROJECT_BUSY",
                    "Another authoring command is using this project; retry after it finishes",
                )
            },
        )?;
        #[cfg(not(unix))]
        lock.try_lock().map_err(|_| {
            Error::new(
                "TOOL_PROJECT_BUSY",
                "Another authoring command is using this project",
            )
        })?;
        let state_path = root.join(format!("{scope}.json"));
        let state = match fs::symlink_metadata(&state_path) {
            Ok(_) => serde_json::from_slice::<State>(&read_limited(&state_path, 4 * 1024 * 1024)?)
                .map_err(|_| local_error("Invalid authoring receipt; refusing to overwrite it"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State {
                format: 1,
                identity: identity.clone(),
                directory: directory.clone(),
                binding: None,
                receipt: None,
                installation_pending: false,
                pending_identity: None,
            },
            Err(e) => return Err(io_error(e)),
        };
        if state.format != 1 || state.identity != *identity || state.directory != directory {
            return Err(local_error(
                "Receipt identity/backend/account/org/project mismatch",
            ));
        }
        if existing && state.installation_pending {
            return Err(Error::new(
                "TOOL_PULL_INCOMPLETE",
                "A prior pull did not durably finish installing this directory. Pull into a new directory before publishing",
            ));
        }
        Ok(Self {
            root,
            state_path,
            _lock: lock,
            state,
        })
    }
    fn save(&self) -> Result<()> {
        atomic_save(
            &self.state_path,
            &serde_json::to_vec_pretty(&self.state)
                .map_err(|_| local_error("Cannot encode authoring receipt"))?,
        )
    }
    fn stage_path(&self, id: Uuid) -> PathBuf {
        self.root.join(format!("package-{id}.zip"))
    }
    fn stage(&self, receipt: &Receipt, package: &[u8]) -> Result<()> {
        let path = self.stage_path(receipt.id);
        let mut temp = tempfile::NamedTempFile::new_in(&self.root).map_err(io_error)?;
        temp.write_all(package).map_err(io_error)?;
        temp.as_file().sync_all().map_err(io_error)?;
        temp.persist_noclobber(path)
            .map_err(|e| io_error(e.error))?;
        #[cfg(unix)]
        File::open(&self.root)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)?;
        Ok(())
    }
    fn package(&self, receipt: &Receipt) -> Result<Vec<u8>> {
        let bytes = read_limited(&self.stage_path(receipt.id), tool_project::MAX_ZIP)?;
        if digest(&bytes) != receipt.checksum {
            return Err(local_error(
                "Staged package checksum mismatch; refusing to send altered bytes",
            ));
        }
        Ok(bytes)
    }
    fn checkpoint(&mut self, receipt: &Receipt) -> Result<()> {
        self.state.receipt = Some(receipt.clone());
        self.save()
    }
}
fn uncertain(mut error: Error, journal: &Journal, receipt: &Receipt, key: &str) -> Error {
    if error.retryable
        || error.status.is_some_and(|s| s >= 500)
        || matches!(error.code.as_str(), "TRANSPORT_ERROR" | "INVALID_RESPONSE")
    {
        let cause = error.code.clone();
        error.code = "TOOL_PUBLISH_UNCERTAIN".into();
        error.message="Publication may have been accepted. Re-run the same command to resume its durable receipt; use --resume to use the saved package after local edits".into();
        error.idempotency_key = Some(key.into());
        error.details = Some(Box::new(
            json!({"phase":receipt.phase,"receipt":journal.state_path,"cause":cause,"packageChecksum":receipt.checksum}),
        ));
        error.retryable = true;
    }
    error
}
fn failed(error: Error, journal: &mut Journal, receipt: &Receipt, key: &str) -> Error {
    if matches!(error.status, Some(400 | 403 | 404 | 409 | 413 | 422)) {
        let mut rejected = receipt.clone();
        rejected.rejected = Some(error.code.clone());
        if let Err(storage_error) = journal.checkpoint(&rejected) {
            return storage_error;
        }
        let revision = receipt
            .draft
            .as_ref()
            .and_then(|d| d.get("revisionId"))
            .cloned();
        let skill = receipt
            .base
            .as_ref()
            .map(|b| b.skill_id)
            .or_else(|| journal.state.binding.as_ref().map(|b| b.skill_id));
        let mut error = error;
        error.details = Some(Box::new(
            json!({"serverDetails":error.details,"receipt":journal.state_path,"phase":receipt.phase,"skillId":skill,"revisionId":revision,
            "recovery":"Correct the rejected input and run publish again. For a stale base, explicitly run tool draft abandon <tool-id> <revision-id> when a revision exists, then tool pull <tool-id> --output <new-directory> and reapply changes. Unknown writes must be resumed with their original receipt."}),
        ));
        error
    } else {
        uncertain(error, journal, receipt, key)
    }
}
fn check_metadata(package: &mut Package, base: &Binding) -> Result<()> {
    if !base.published {
        return Ok(());
    }
    for (wire, dto) in [
        ("scenario", "scenario"),
        ("name_en", "nameEn"),
        ("description_en", "descriptionEn"),
        ("detail_en", "detailEn"),
        ("tags", "tags"),
        ("scenes", "scenes"),
        ("case_public_link", "caseCanvasUrl"),
        ("category_ids", "categoryIds"),
    ] {
        if !package.explicit_fields.iter().any(|k| k == wire) {
            continue;
        }
        if let Some(desired) = package.metadata.get(wire) {
            let existing = base
                .author_metadata
                .get(dto)
                .cloned()
                .unwrap_or(Value::Null);
            let equivalent_empty = existing.is_null()
                && (desired.as_str() == Some("")
                    || desired.as_array().is_some_and(|a| a.is_empty()));
            if *desired != existing && !equivalent_empty {
                return Err(Error::new(
                    "TOOL_METADATA_UPDATE_UNSUPPORTED",
                    format!(
                        "Published updates cannot change meta.json {dto}; only name, description, detail and covers are supported. Restore this field to the current server value before publishing"
                    ),
                ));
            }
        }
    }
    // Omitted covers preserve the pinned remote value; an explicit empty string
    // is a clear operation (including historical sourceMetadata null covers).
    for (wire, dto) in [
        ("cover_url", "coverUrl"),
        ("cover_input_url", "coverInputUrl"),
    ] {
        if !package.metadata.as_object().unwrap().contains_key(wire) {
            package.config[dto] = base
                .author_metadata
                .get(dto)
                .cloned()
                .unwrap_or(Value::Null);
        }
    }
    Ok(())
}
fn receipt(package: Package, base: Option<Binding>, scope: Option<&str>) -> Receipt {
    Receipt {
        protocol: 2,
        id: Uuid::new_v4(),
        input_checksum: package.fingerprint(),
        checksum: package.checksum,
        metadata: package.metadata,
        config: package.config,
        scope: scope.map(str::to_owned),
        base,
        phase: Phase::Prepared,
        upload_key: Uuid::new_v4().to_string(),
        publish_key: Uuid::new_v4().to_string(),
        uploaded: None,
        draft: None,
        result: None,
        rejected: None,
    }
}
fn bound_response(
    value: &Value,
    identity: &AuthoringIdentity,
    expected: Option<Uuid>,
) -> Result<Binding> {
    let skill = value
        .get("skill")
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Authoring response is missing skill"))?;
    check_owner(skill, identity)?;
    let b = binding(skill, None, None)?;
    if expected.is_some_and(|id| id != b.skill_id) {
        return Err(Error::new(
            "INVALID_RESPONSE",
            "Authoring response changed tool identity",
        ));
    }
    Ok(b)
}

async fn author_with_protocol(
    client: &Client,
    directory: &Path,
    state_root: &Path,
    identity: &AuthoringIdentity,
    scope: Option<&str>,
    resume: bool,
    mode: (bool, u32),
) -> Result<Value> {
    let (build_only, protocol) = mode;
    if scope.is_some_and(|s| !matches!(s, "personal" | "team")) {
        return Err(Error::new(
            "INVALID_INPUT",
            "Scope must be personal or team",
        ));
    }
    let mut journal = Journal::open(state_root, directory, identity, true)?;
    let pending = journal.state.receipt.as_ref().is_some_and(|r| {
        r.phase != Phase::Published && r.phase != Phase::Built && (r.rejected.is_none() || resume)
    });
    let mut r = if pending {
        let mut r = journal.state.receipt.clone().unwrap();
        if scope.is_some() && scope != r.scope.as_deref() {
            return Err(Error::new(
                "TOOL_PUBLISH_PENDING",
                "Scope differs from the pending receipt; resume with its original scope",
            ));
        }
        let changed = if resume {
            false
        } else {
            let mut package = tool_project::pack(directory)?;
            if let Some(base) = &r.base {
                check_metadata(&mut package, base)?;
            }
            package.fingerprint() != r.input_checksum
        };
        if changed {
            return Err(Error::new(
                "TOOL_PUBLISH_PENDING",
                "Local package changed during a pending publication; use --resume to publish the exact staged package",
            ));
        }
        r.rejected = None;
        journal.checkpoint(&r)?;
        r
    } else {
        if resume {
            return Err(Error::new(
                "TOOL_PUBLISH_NOT_PENDING",
                "No pending publication to resume",
            ));
        }
        let mut package = tool_project::pack(directory)?;
        if let Some(base) = &journal.state.binding {
            check_metadata(&mut package, base)?;
        }
        let previous = journal.state.receipt.clone();
        if let Some(prior) = previous.as_ref().filter(|p| {
            p.protocol == 2
                && p.phase == Phase::Built
                && p.input_checksum == package.fingerprint()
                && p.scope.as_deref() == scope
        }) {
            let mut prior = prior.clone();
            let bytes = journal.package(&prior)?;
            return run_v2(
                client,
                &mut journal,
                &mut prior,
                identity,
                bytes,
                build_only,
            )
            .await;
        }

        if let Some(mut prior) = previous.clone().filter(|p| {
            p.rejected.is_some()
                && p.input_checksum == package.fingerprint()
                && matches!(p.phase, Phase::Uploaded | Phase::Validated)
        }) {
            // The upload is confirmed. Correct a rejected scope/finalize request
            // without creating a second tool or replacing the accepted ZIP.
            prior.scope = scope.map(str::to_owned);
            prior.publish_key = Uuid::new_v4().to_string();
            prior.rejected = None;
            journal.checkpoint(&prior)?;
            prior
        } else {
            let bytes = package.bytes.clone();
            let mut r = receipt(package, journal.state.binding.clone(), scope);
            r.protocol = protocol;
            if let Some(prior) = previous.filter(|p| p.rejected.is_some()) {
                // Retain ownership of a known local revision when correcting it.
                r.draft = prior.draft;
            }
            journal.stage(&r, &bytes)?;
            journal.checkpoint(&r)?;
            r
        }
    };
    // Checking the staged checksum on every invocation detects receipt corruption
    // even when the remaining phase does not upload a package.
    let package = journal.package(&r)?;
    if r.protocol == 2 {
        return run_v2(client, &mut journal, &mut r, identity, package, build_only).await;
    }
    if build_only {
        return Err(Error::new(
            "TOOL_LEGACY_PUBLICATION_PENDING",
            "Resume the old publication before building with V2",
        ));
    }
    let published_base = r.base.as_ref().is_some_and(|b| b.published);
    if r.phase == Phase::Prepared {
        if published_base {
            let base = r.base.as_ref().unwrap();
            let mut options = json!({"skillId":base.skill_id,"organizationId":identity.org,"baseVersionId":base.base_version_id,"expectedMetadataHash":base.metadata_hash,"config":r.config});
            if let Some(draft) = &r.draft {
                options["revisionId"] = draft["revisionId"].clone();
                options["expectedRevision"] = draft["serverRevision"].clone();
            }
            let result = client
                .tool_save_package_draft(base.skill_id, package, &options, &r.upload_key)
                .await
                .map_err(|e| failed(e, &mut journal, &r, &r.upload_key))?;
            let draft = result.get("draft").ok_or_else(|| {
                uncertain(
                    Error::new("INVALID_RESPONSE", "Draft response is missing draft"),
                    &journal,
                    &r,
                    &r.upload_key,
                )
            })?;
            field_uuid(draft, "revisionId")
                .map_err(|e| failed(e, &mut journal, &r, &r.upload_key))?;
            if draft
                .get("serverRevision")
                .and_then(Value::as_u64)
                .is_none_or(|n| n == 0)
                || field_uuid(draft, "baseVersionId")? != base.base_version_id
            {
                return Err(uncertain(
                    Error::new("INVALID_RESPONSE", "Draft revision/base mismatch"),
                    &journal,
                    &r,
                    &r.upload_key,
                ));
            }
            r.draft = Some(draft.clone());
            r.phase = Phase::DraftSaved;
            journal.checkpoint(&r)?;
        } else {
            let result = if let Some(base) = &r.base {
                client
                    .tool_replace_package(
                        identity.org,
                        base.skill_id,
                        &ToolPackageBase {
                            version_id: base.base_version_id,
                            metadata_hash: base.metadata_hash.clone(),
                        },
                        package,
                        &r.metadata,
                        &r.upload_key,
                    )
                    .await
            } else {
                client
                    .tool_create_package(identity.org, package, &r.metadata, &r.upload_key)
                    .await
            }
            .map_err(|e| failed(e, &mut journal, &r, &r.upload_key))?;
            bound_response(&result, identity, r.base.as_ref().map(|b| b.skill_id))
                .map_err(|e| failed(e, &mut journal, &r, &r.upload_key))?;
            journal.state.binding = Some(bound_response(
                &result,
                identity,
                r.base.as_ref().map(|b| b.skill_id),
            )?);
            r.uploaded = Some(result);
            r.phase = Phase::Uploaded;
            journal.checkpoint(&r)?;
        }
    }
    if r.phase == Phase::DraftSaved {
        let base = r.base.as_ref().unwrap();
        let draft = r.draft.as_ref().unwrap();
        let revision = field_uuid(draft, "revisionId")?;
        let body =
            json!({"skillId":base.skill_id,"organizationId":identity.org,"revisionId":revision});
        let result = client
            .tool_validate_package_draft(base.skill_id, &body)
            .await
            .map_err(|e| failed(e, &mut journal, &r, &r.upload_key))?;
        if result.get("revisionId").and_then(Value::as_str) != Some(revision.to_string().as_str()) {
            return Err(Error::new(
                "INVALID_RESPONSE",
                "Validation response changed revision identity",
            ));
        }
        if result.get("valid").and_then(Value::as_bool) != Some(true) {
            return Err(failed(
                Error {
                    status: Some(422),
                    details: Some(Box::new(result)),
                    ..Error::new(
                        "TOOL_REMOTE_VALIDATION_FAILED",
                        "Server rejected the staged draft; source and live publication were not replaced",
                    )
                },
                &mut journal,
                &r,
                &r.upload_key,
            ));
        }
        r.phase = Phase::Validated;
        journal.checkpoint(&r)?;
    }
    if matches!(r.phase, Phase::Uploaded | Phase::Validated) {
        let result = if r.phase == Phase::Uploaded {
            let current = bound_response(r.uploaded.as_ref().unwrap(), identity, r.base.as_ref().map(|b|b.skill_id))?;
            let mut body = json!({"skillId":current.skill_id,"organizationId":identity.org,"source":"uploaded_package","expectedVersionId":current.base_version_id,"expectedMetadataHash":current.metadata_hash});
            if let Some(scope) = &r.scope { body["accessScope"] = json!(scope); }
            client.tool_finalize_package(&body, &r.publish_key).await
        } else {
            let base = r.base.as_ref().unwrap();
            let draft = r.draft.as_ref().unwrap();
            let mut body = json!({"skillId":base.skill_id,"organizationId":identity.org,"revisionId":draft["revisionId"],"expectedRevision":draft["serverRevision"],"expectedVersionId":base.base_version_id,"expectedMetadataHash":base.metadata_hash});
            if let Some(scope) = &r.scope { body["accessScope"] = json!(scope); }
            client.tool_publish_package_draft(base.skill_id, &body, &r.publish_key).await
        }.map_err(|e| failed(e, &mut journal, &r, &r.publish_key))?;
        let expected = r.base.as_ref().map(|b| b.skill_id).or_else(|| {
            r.uploaded
                .as_ref()
                .and_then(|v| v.get("skill"))
                .and_then(|v| v.get("id"))
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        });
        let binding = bound_response(&result, identity, expected)
            .map_err(|e| failed(e, &mut journal, &r, &r.publish_key))?;
        if !binding.published
            || field_uuid(&result, "versionId")
                .map_err(|e| uncertain(e, &journal, &r, &r.publish_key))?
                != binding.base_version_id
            || result.get("releaseId").and_then(Value::as_str)
                != binding
                    .source_release_id
                    .as_ref()
                    .map(Uuid::to_string)
                    .as_deref()
        {
            return Err(uncertain(
                Error::new(
                    "INVALID_RESPONSE",
                    "Publication response has inconsistent artifact/release identity",
                ),
                &journal,
                &r,
                &r.publish_key,
            ));
        }
        journal.state.binding = Some(binding);
        r.result = Some(result);
        r.phase = Phase::Published;
        journal.checkpoint(&r)?;
    }
    r.result
        .ok_or_else(|| local_error("Publication receipt has no result"))
}

fn source_binding(source: &ToolSource, identity: &AuthoringIdentity) -> Result<Binding> {
    check_owner(&source.skill, identity)?;
    let result = binding(&source.skill, Some(source.version_id), source.release_id)?;
    if source.metadata_hash != result.metadata_hash {
        return Err(Error::new(
            "INVALID_RESPONSE",
            "Source metadata CAS hash disagrees with current tool",
        ));
    }
    Ok(result)
}
pub async fn pull(
    client: &Client,
    skill: Uuid,
    directory: &Path,
    release: Option<Uuid>,
    state_root: &Path,
    identity: &AuthoringIdentity,
) -> Result<Value> {
    if fs::symlink_metadata(directory).is_ok() {
        return Err(Error::new(
            "DESTINATION_EXISTS",
            "Pull refuses to overwrite an existing destination",
        ));
    }
    let mut journal = Journal::open(state_root, directory, identity, false)?;
    if journal
        .state
        .receipt
        .as_ref()
        .is_some_and(|r| r.phase != Phase::Published)
    {
        return Err(Error::new(
            "TOOL_PUBLISH_PENDING",
            "Destination has a pending publish receipt; choose another directory",
        ));
    }
    let source = client.tool_source(identity.org, skill, release).await?;
    let binding = source_binding(&source, identity)?;
    let mut files = tool_project::unpack(&source.package, &source.package_checksum)?;
    // Use the selected artifact snapshot, never today's live metadata. Legacy
    // missing values stay absent and fall back to the historical SKILL.md.
    let source_meta = source.source_metadata.as_object().ok_or_else(|| {
        Error::new(
            "INVALID_RESPONSE",
            "Source export is missing selected artifact sourceMetadata",
        )
    })?;
    let mut meta = serde_json::Map::new();
    for key in [
        "name",
        "description",
        "detail",
        "scenario",
        "nameEn",
        "descriptionEn",
        "detailEn",
        "tags",
        "scenes",
        "coverUrl",
        "coverInputUrl",
        "casePublicLink",
    ] {
        if let Some(value) = source_meta.get(key).filter(|v| !v.is_null()) {
            meta.insert(key.to_owned(), value.clone());
        }
    }
    for key in ["coverUrl", "coverInputUrl"] {
        if source_meta.get(key).is_some_and(Value::is_null) {
            meta.insert(key.into(), json!(""));
        }
    }
    meta.insert("version".into(), json!(source.version));
    files.insert(
        "meta.json".into(),
        serde_json::to_vec_pretty(&Value::Object(meta))
            .map_err(|_| local_error("Cannot encode source metadata"))?,
    );
    if !files.contains_key("interface.json")
        && let Some(schema) = source_meta.get("inputSchema").filter(|v| !v.is_null())
    {
        files.insert(
            "interface.json".into(),
            serde_json::to_vec_pretty(schema)
                .map_err(|_| local_error("Cannot encode source interface"))?,
        );
    }
    // Persist the binding first. A crash may leave only a reusable binding, never
    // extracted files which would accidentally publish as a new unrelated tool.
    journal.state.binding = Some(binding);
    journal.state.receipt = None;
    journal.state.installation_pending = true;
    journal.save()?;
    tool_project::install_files(directory, &files)?;
    journal.state.installation_pending = false;
    journal.save()?;
    Ok(
        json!({"directory":directory,"skillId":skill,"sourceVersionId":source.version_id,"sourceReleaseId":source.release_id,"version":source.version,"baseVersionId":journal.state.binding.as_ref().unwrap().base_version_id,"packageChecksum":source.package_checksum,"files":files.keys().collect::<Vec<_>>()}),
    )
}

async fn run_v2(
    client: &Client,
    journal: &mut Journal,
    r: &mut Receipt,
    identity: &AuthoringIdentity,
    package: Vec<u8>,
    build_only: bool,
) -> Result<Value> {
    if r.phase == Phase::Prepared {
        if let Some(base) = &r.base {
            r.uploaded = Some(json!({"skill":base.author_metadata}));
        } else if let Some(created) = &journal.state.pending_identity {
            r.uploaded = Some(created.clone());
        } else {
            let body = json!({"organization_id":identity.org,"name":r.metadata["name"],
                "description":r.metadata["description"],"detail":r.metadata["detail"]});
            let key = format!("identity-{}", r.upload_key);
            let response = client
                .tool_create_identity(body, &key)
                .await
                .map_err(|e| failed(e, journal, r, &key))?;
            let skill = response.get("skill").ok_or_else(|| {
                uncertain(
                    Error::new("INVALID_RESPONSE", "Identity response is missing skill"),
                    journal,
                    r,
                    &key,
                )
            })?;
            check_owner(skill, identity).map_err(|e| failed(e, journal, r, &key))?;
            field_uuid(skill, "id").map_err(|e| failed(e, journal, r, &key))?;
            journal.state.pending_identity = Some(response.clone());
            r.uploaded = Some(response);
        }
        r.phase = Phase::IdentityCreated;
        journal.checkpoint(r)?;
    }
    if r.phase == Phase::IdentityCreated {
        let skill = &r
            .uploaded
            .as_ref()
            .ok_or_else(|| local_error("Missing created tool"))?["skill"];
        let id = field_uuid(skill, "id")?;
        let hash = field_string(skill, "metadataHash")?;
        let response = client
            .tool_submit_artifact(
                identity.org,
                id,
                package,
                &r.metadata,
                Some(&hash),
                &r.upload_key,
            )
            .await
            .map_err(|e| failed(e, journal, r, &r.upload_key))?;
        check_owner(&response["skill"], identity)
            .map_err(|e| failed(e, journal, r, &r.upload_key))?;
        if field_uuid(&response["skill"], "id")? != id
            || field_string(&response["artifact"], "checksum")? != r.checksum
        {
            return Err(uncertain(
                Error::new("INVALID_RESPONSE", "Artifact identity/checksum mismatch"),
                journal,
                r,
                &r.upload_key,
            ));
        }
        let artifact_id = field_uuid(&response["artifact"], "id")?;
        let mut skill = response["skill"].clone();
        if skill["versionId"].is_null() {
            skill["versionId"] = json!(artifact_id);
        }
        journal.state.binding = Some(binding(&skill, Some(artifact_id), None)?);
        r.uploaded = Some(response);
        r.phase = Phase::Built;
        journal.checkpoint(r)?;
    }
    if build_only {
        return r
            .uploaded
            .clone()
            .ok_or_else(|| local_error("Missing artifact"));
    }
    if matches!(r.phase, Phase::Built | Phase::Releasing) {
        r.phase = Phase::Releasing;
        journal.checkpoint(r)?;
        let built = r
            .uploaded
            .as_ref()
            .ok_or_else(|| local_error("Missing artifact"))?;
        let id = field_uuid(&built["skill"], "id")?;
        let artifact_id = field_uuid(&built["artifact"], "id")?;
        let base = &built["artifact"]["publicationBase"];
        let mut body = json!({"organization_id":identity.org,"artifact_id":artifact_id,
            "expected_release_id":base["releaseId"],"expected_metadata_hash":base["metadataHash"]});
        if let Some(scope) = &r.scope {
            body["access_scope"] = json!(scope);
        }
        let mut result = client
            .tool_release_artifact(id, body, &r.publish_key)
            .await
            .map_err(|e| failed(e, journal, r, &r.publish_key))?;
        let bound = bound_response(&result, identity, Some(id))
            .map_err(|e| failed(e, journal, r, &r.publish_key))?;
        if !bound.published {
            return Err(uncertain(
                Error::new("INVALID_RESPONSE", "Missing release"),
                journal,
                r,
                &r.publish_key,
            ));
        }
        result["validation"] = built["artifact"]["validation"].clone();
        journal.state.binding = Some(bound);
        r.result = Some(result);
        r.phase = Phase::Published;
        journal.checkpoint(r)?;
    }
    r.result
        .clone()
        .ok_or_else(|| local_error("Missing publication result"))
}

pub async fn publish_artifact(
    client: &Client,
    identity: &AuthoringIdentity,
    skill: Uuid,
    artifact: Uuid,
    scope: Option<&str>,
) -> Result<Value> {
    let detail = client.tool_artifact(identity.org, skill, artifact).await?;
    check_owner(&detail["skill"], identity)?;
    if field_uuid(&detail["artifact"], "id")? != artifact
        || field_uuid(&detail["skill"], "id")? != skill
    {
        return Err(Error::new("INVALID_RESPONSE", "Artifact identity mismatch"));
    }
    let base = &detail["artifact"]["publicationBase"];
    let mut body = json!({"organization_id":identity.org,"artifact_id":artifact,
        "expected_release_id":base["releaseId"],"expected_metadata_hash":base["metadataHash"]});
    if let Some(scope) = scope {
        body["access_scope"] = json!(scope);
    }
    // Immutable artifact carries its original baseline; repeated invocation keeps the same request.
    let key = format!(
        "artifact-{}",
        digest(
            &serde_json::to_vec(&json!([identity, skill, body]))
                .map_err(|e| local_error(e.to_string()))?
        )
    );
    client.tool_release_artifact(skill, body, &key).await
}

pub async fn author(
    client: &Client,
    directory: &Path,
    state_root: &Path,
    identity: &AuthoringIdentity,
    scope: Option<&str>,
    resume: bool,
    build_only: bool,
) -> Result<Value> {
    author_with_protocol(
        client,
        directory,
        state_root,
        identity,
        scope,
        resume,
        (build_only, 2),
    )
    .await
}

// Compatibility harness seeds pre-V2 receipts, then exercises the same recovery path.
// Production only creates protocol 2 receipts; deserialized old receipts keep protocol 0.
#[cfg(test)]
#[allow(dead_code)]
pub async fn publish(
    client: &Client,
    directory: &Path,
    state_root: &Path,
    identity: &AuthoringIdentity,
    scope: Option<&str>,
    resume: bool,
) -> Result<Value> {
    author_with_protocol(
        client,
        directory,
        state_root,
        identity,
        scope,
        resume,
        (false, 0),
    )
    .await
}
