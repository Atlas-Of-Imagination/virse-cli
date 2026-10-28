//! Durable single-file writes. A key pins the identity, resolved destination,
//! filename, options and bytes before the first HTTP write. Even successful
//! receipts are replayed against the server, never treated as a local binding.
use crate::{resource_paths::Resolver, storage::Store};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
use virse_api::{
    AccessContext, AssetUploadFile, Client, Error, ReplaceOptions, Result, UploadOptions,
};

const MAX_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    Rename,
    Skip,
    Error,
}

#[derive(Args)]
pub struct UploadArgs {
    pub file: PathBuf,
    /// Existing resource directory, as an absolute path or @UUID.
    pub destination: String,
    #[arg(long, value_enum, default_value = "rename")]
    pub conflict: Conflict,
    /// Reuse the original key to recover an uncertain upload.
    #[arg(long)]
    pub idempotency_key: Option<String>,
    /// Send the saved snapshot, even if the original file was changed or deleted.
    #[arg(long, requires = "idempotency_key")]
    pub resume: bool,
}

#[derive(Args)]
pub struct ReplaceArgs {
    /// Resource path or @UUID, not an asset ID.
    pub target: String,
    pub file: PathBuf,
    /// Content revision from a previous server read; never inferred by this command.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub expected_revision: u64,
    /// Explicitly discard model editing state when replacing a GLB.
    #[arg(long, requires = "expected_state_revision")]
    pub reset_model_state: bool,
    /// Model state revision from a previous server read (independent of content revision).
    #[arg(long, requires = "reset_model_state")]
    pub expected_state_revision: Option<u64>,
    #[arg(long)]
    pub idempotency_key: Option<String>,
    /// Send the saved snapshot, even if the original file was changed or deleted.
    #[arg(long, requires = "idempotency_key")]
    pub resume: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    backend: String,
    user: Uuid,
    org: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
struct Receipt {
    format: u32,
    key: String,
    identity: Identity,
    intent: Value,
    filename: String,
    sha256: String,
    size: u64,
    /// None means upload; Some pins the replace asset identity.
    asset_id: Option<Uuid>,
    options: Value,
}

struct Journal {
    root: PathBuf,
    receipt_path: PathBuf,
    stage_path: PathBuf,
    _lock: File,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new("INVALID_INPUT", message)
}

fn journal_error(message: impl Into<String>) -> Error {
    Error::new("ASSET_RECEIPT_ERROR", message)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Open the parent by descriptor so a swapped symlink cannot redirect the file
/// open. NONBLOCK prevents a FIFO supplied as FILE from hanging the command.
fn open_file(path: &Path, create: bool, private: bool) -> Result<File> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let directory = crate::tool_project::open_directory(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("File path must have a filename"))?;
    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags, openat};
        let mut flags = OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        flags |= if create {
            OFlags::RDWR | OFlags::CREATE
        } else {
            OFlags::RDONLY
        };
        File::from(
            openat(&directory, name, flags, Mode::from_raw_mode(0o600))
                .map_err(|_| invalid("File must be readable, regular, and not a symlink"))?,
        )
    };
    #[cfg(not(unix))]
    let file = {
        let _ = directory;
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(invalid("Symlink files are not allowed"));
            }
            Err(e) if !(create && e.kind() == std::io::ErrorKind::NotFound) => {
                return Err(crate::storage::io_error(e));
            }
            _ => {}
        }
        fs::OpenOptions::new()
            .read(true)
            .write(create)
            .create(create)
            .open(path)
            .map_err(crate::storage::io_error)?
    };
    let metadata = file.metadata().map_err(crate::storage::io_error)?;
    if !metadata.is_file() {
        return Err(invalid("Upload and receipt paths must be regular files"));
    }
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
            return Err(journal_error(
                "Receipt files must be private (0600) and not hard-linked",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    Ok(file)
}

fn read_file(path: &Path, limit: u64, private: bool) -> Result<Vec<u8>> {
    let mut file = open_file(path, false, private)?;
    let before = file.metadata().map_err(crate::storage::io_error)?;
    if before.len() > limit {
        return Err(invalid(
            "File exceeds the 100 MiB upload limit (or receipt size limit)",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(crate::storage::io_error)?;
    let after = file.metadata().map_err(crate::storage::io_error)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("File exceeds the 100 MiB upload limit"));
    }
    let mut changed = before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        changed |= before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec();
    }
    if changed {
        return Err(invalid(
            "File changed while being read; retry after the local writer finishes",
        ));
    }
    Ok(bytes)
}

fn atomic_save(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| journal_error("Invalid receipt path"))?;
    let directory = crate::tool_project::open_directory(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(crate::storage::io_error)?;
    temp.write_all(bytes).map_err(crate::storage::io_error)?;
    temp.as_file()
        .sync_all()
        .map_err(crate::storage::io_error)?;
    temp.persist(path)
        .map_err(|e| crate::storage::io_error(e.error))?;
    #[cfg(unix)]
    directory.sync_all().map_err(crate::storage::io_error)?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

/// Create missing state directories without following symlink components.
fn private_directory(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_err() {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if parent != path {
            private_directory(parent)?;
        }
        let directory = crate::tool_project::open_directory(parent)?;
        #[cfg(unix)]
        let created = rustix::fs::mkdirat(
            &directory,
            path.file_name()
                .ok_or_else(|| journal_error("Invalid state directory"))?,
            rustix::fs::Mode::from_raw_mode(0o700),
        )
        .map_err(std::io::Error::from);
        #[cfg(not(unix))]
        let created = fs::create_dir(path);
        match created {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(crate::storage::io_error(e)),
        }
        #[cfg(unix)]
        directory.sync_all().map_err(crate::storage::io_error)?;
        #[cfg(not(unix))]
        let _ = directory;
    }
    crate::tool_project::open_directory(path)?;
    Ok(())
}

impl Journal {
    fn open(store: &Store, key: &str, identity: &Identity) -> Result<Self> {
        let root = store.root.join("asset-writes");
        private_directory(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            crate::tool_project::open_directory(&root)?
                .set_permissions(fs::Permissions::from_mode(0o700))
                .map_err(crate::storage::io_error)?;
        }
        let scope = digest(
            json!([identity.backend, identity.user, identity.org, key])
                .to_string()
                .as_bytes(),
        );
        let lock = open_file(&root.join(format!("{scope}.lock")), true, true)?;
        #[cfg(unix)]
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |_| {
                Error::new(
                    "ASSET_WRITE_BUSY",
                    "Another process is using this upload key",
                )
            },
        )?;
        #[cfg(not(unix))]
        lock.try_lock().map_err(|_| {
            Error::new(
                "ASSET_WRITE_BUSY",
                "Another process is using this upload key",
            )
        })?;
        Ok(Self {
            receipt_path: root.join(format!("{scope}.json")),
            stage_path: root.join(format!("{scope}.bytes")),
            root,
            _lock: lock,
        })
    }

    fn load(&self) -> Result<Option<Receipt>> {
        match fs::symlink_metadata(&self.receipt_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(crate::storage::io_error(e)),
            Ok(_) => serde_json::from_slice(&read_file(&self.receipt_path, 1024 * 1024, true)?)
                .map(Some)
                .map_err(|_| journal_error("Invalid receipt; refusing to overwrite it")),
        }
    }

    fn prepare(&self, receipt: &Receipt, bytes: &[u8]) -> Result<()> {
        atomic_save(&self.stage_path, bytes)?;
        atomic_save(
            &self.receipt_path,
            &serde_json::to_vec(receipt).map_err(|_| journal_error("Cannot encode receipt"))?,
        )
    }

    fn bytes(&self, receipt: &Receipt) -> Result<Vec<u8>> {
        let bytes = read_file(&self.stage_path, MAX_BYTES, true)?;
        if bytes.len() as u64 != receipt.size || digest(&bytes) != receipt.sha256 {
            return Err(journal_error(
                "Staged file checksum mismatch; refusing altered bytes",
            ));
        }
        Ok(bytes)
    }
}

fn source_path(path: &Path) -> Result<PathBuf> {
    // Do not canonicalize: retries after deletion must identify the same intent.
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(crate::storage::io_error)?
            .join(path)
    };
    if path
        .components()
        .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(invalid("Use a file path without '..' components"));
    }
    Ok(path)
}

fn filename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty() && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\')))
        .map(str::to_owned)
        .ok_or_else(|| invalid("File must have a valid UTF-8 filename without control characters"))
}

fn key(value: &Option<String>, resume: bool) -> Result<String> {
    if resume && value.is_none() {
        return Err(invalid("--resume requires the original --idempotency-key"));
    }
    let key = value.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(invalid(
            "Idempotency key must contain 1..128 printable ASCII characters without spaces",
        ));
    }
    Ok(key)
}

async fn identity(client: &Client, backend: &str, org: Uuid) -> Result<Identity> {
    let user = client.me().await?["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .filter(|id| !id.is_nil())
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Identity has no valid user ID"))?;
    if org.is_nil() {
        return Err(invalid("A concrete organization is required"));
    }
    Ok(Identity {
        backend: virse_api::normalize_backend(backend)?,
        user,
        org,
    })
}

fn recover(
    journal: &Journal,
    key: &str,
    identity: &Identity,
    intent: &Value,
    source: &Path,
    resume: bool,
) -> Result<Option<Receipt>> {
    let Some(receipt) = journal.load()? else {
        if resume {
            return Err(journal_error(
                "No saved upload exists for this key; cannot resume",
            ));
        }
        return Ok(None);
    };
    if receipt.format != 1
        || receipt.key != key
        || receipt.identity != *identity
        || receipt.intent != *intent
        || receipt.filename != filename(source)?
    {
        return Err(Error::new(
            "ASSET_INTENT_CHANGED",
            "This key is pinned to a different account, backend, organization, file, target or options; use the original command to recover it",
        ));
    }
    if !resume {
        let bytes = read_file(source, MAX_BYTES, false)?;
        if bytes.len() as u64 != receipt.size || digest(&bytes) != receipt.sha256 {
            return Err(Error::new(
                "ASSET_SOURCE_CHANGED",
                "Local file differs from the saved upload. Use --resume with this key to explicitly send the original staged snapshot",
            ));
        }
    }
    // Verify even completed receipts: the server must evaluate the original bytes.
    journal.bytes(&receipt)?;
    Ok(Some(receipt))
}

fn revision(value: &Value) -> Result<u64> {
    value
        .pointer("/content/revision")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or_else(|| Error::new("INVALID_RESPONSE", "Asset has no valid content revision"))
}

fn conflict(code: &str, expected: u64, current: u64) -> Error {
    Error {
        status: Some(409),
        details: Some(Box::new(
            json!({"expected_revision":expected,"current_revision":current}),
        )),
        ..Error::new(
            code,
            "Supplied revision differs from the server read; read and review the current content before a new write",
        )
    }
}

pub async fn upload(
    args: &UploadArgs,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    json_output: bool,
) -> Result<u8> {
    let key = key(&args.idempotency_key, args.resume)?;
    let source = source_path(&args.file)?;
    let intent = json!({"operation":"upload","file":source,"destination":args.destination,"conflict":args.conflict});
    let identity = identity(client, backend, org).await?;
    let journal = Journal::open(store, &key, &identity)?;
    eprintln!("Operation key: {key}");
    let receipt = match recover(&journal, &key, &identity, &intent, &source, args.resume)? {
        Some(receipt) => {
            eprintln!("Recovery: using the saved destination and file snapshot");
            receipt
        }
        None => {
            eprintln!("Preparing: checking local file and resolving destination");
            let filename = filename(&source)?;
            let bytes = read_file(&source, MAX_BYTES, false)?;
            if bytes.is_empty() {
                return Err(invalid("Upload file must not be empty"));
            }
            let parent = Resolver::new(client, org).folder(&args.destination).await?;
            let options = json!({"destination":{"org_id":org,"kind":"drive","parent_id":parent},"conflict":args.conflict,"retention":"persistent"});
            let _: UploadOptions = serde_json::from_value(options.clone())
                .map_err(|_| invalid("Invalid upload options"))?;
            let receipt = Receipt {
                format: 1,
                key,
                identity,
                intent,
                filename,
                sha256: digest(&bytes),
                size: bytes.len() as u64,
                asset_id: None,
                options,
            };
            journal.prepare(&receipt, &bytes)?;
            eprintln!("Prepared: original bytes and request saved durably");
            receipt
        }
    };
    send(client, &journal, &receipt, json_output).await
}

pub async fn replace(
    args: &ReplaceArgs,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    json_output: bool,
) -> Result<u8> {
    if args.expected_revision == 0
        || args.reset_model_state != args.expected_state_revision.is_some()
    {
        return Err(invalid(
            "Supply a positive --expected-revision; model reset requires --reset-model-state and --expected-state-revision together",
        ));
    }
    let key = key(&args.idempotency_key, args.resume)?;
    let source = source_path(&args.file)?;
    let intent = json!({"operation":"replace","file":source,"target":args.target,"expected_revision":args.expected_revision,"reset_model_state":args.reset_model_state,"expected_state_revision":args.expected_state_revision});
    let identity = identity(client, backend, org).await?;
    let journal = Journal::open(store, &key, &identity)?;
    eprintln!("Operation key: {key}");
    let receipt = match recover(&journal, &key, &identity, &intent, &source, args.resume)? {
        Some(receipt) => {
            eprintln!("Recovery: using the saved target, revisions and file snapshot");
            receipt
        }
        None => {
            eprintln!("Preparing: checking local file and reading target revisions");
            let filename = filename(&source)?;
            let bytes = read_file(&source, MAX_BYTES, false)?;
            if bytes.is_empty() {
                return Err(invalid("Upload file must not be empty"));
            }
            let item = Resolver::new(client, org).item(&args.target).await?;
            // Resource detail resolves shortcuts through the requested dentry's
            // authorization; its list projection may have no asset_id at all.
            let detail = client.resource(org, item.id).await?;
            if detail.resource.id != item.id {
                return Err(Error::new(
                    "INVALID_RESPONSE",
                    "Server returned a different resource identity",
                ));
            }
            let asset = detail
                .asset
                .ok_or_else(|| invalid("Replace requires a resource backed by an asset"))?;
            let asset_id = asset
                .get("id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .filter(|id| !id.is_nil())
                .ok_or_else(|| {
                    Error::new("INVALID_RESPONSE", "Resource has no valid asset identity")
                })?;
            let context = AccessContext {
                org_id: org,
                dentry_id: Some(item.id),
                canvas_space_id: None,
            };
            let current = revision(&asset)?;
            if current != args.expected_revision {
                return Err(conflict(
                    "revision_conflict",
                    args.expected_revision,
                    current,
                ));
            }
            if let Some(expected) = args.expected_state_revision {
                if asset.get("type").and_then(Value::as_str) != Some("model3d") {
                    return Err(invalid(
                        "--reset-model-state only applies to model3d assets",
                    ));
                }
                let current = asset
                    .pointer("/attributes/main_view_revision")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        Error::new(
                            "INVALID_RESPONSE",
                            "Asset read has no model state revision; cannot infer it",
                        )
                    })?;
                if current != expected {
                    return Err(conflict("state_revision_conflict", expected, current));
                }
            }
            let options = json!({"context":context,"expected_revision":args.expected_revision,"reset_model_state":args.reset_model_state,"expected_state_revision":args.expected_state_revision});
            let _: ReplaceOptions = serde_json::from_value(options.clone())
                .map_err(|_| invalid("Invalid replace options"))?;
            let receipt = Receipt {
                format: 1,
                key,
                identity,
                intent,
                filename,
                sha256: digest(&bytes),
                size: bytes.len() as u64,
                asset_id: Some(asset_id),
                options,
            };
            journal.prepare(&receipt, &bytes)?;
            eprintln!("Prepared: original bytes, target and revisions saved durably");
            receipt
        }
    };
    send(client, &journal, &receipt, json_output).await
}

fn may_have_succeeded(error: &Error) -> bool {
    if matches!(
        error.code.as_str(),
        "idempotency_conflict"
            | "revision_conflict"
            | "content_changed"
            | "state_revision_conflict"
    ) {
        return false;
    }
    if error.code == "idempotency_in_progress" {
        return true;
    }
    if error
        .status
        .is_some_and(|s| (400..500).contains(&s) && s != 408 && s != 429)
    {
        return false;
    }
    error.retryable
        || error
            .status
            .is_some_and(|s| s >= 500 || s == 408 || s == 429)
        || matches!(error.code.as_str(), "TRANSPORT_ERROR" | "INVALID_RESPONSE")
}

fn uncertain(error: Error, journal: &Journal, receipt: &Receipt) -> Error {
    Error {
        status: error.status,
        request_id: error.request_id,
        retryable: true,
        idempotency_key: Some(receipt.key.clone().into_boxed_str()),
        details: Some(Box::new(
            json!({"cause":error.code,"server_details":error.details,"receipt":journal.receipt_path,"staged_directory":journal.root}),
        )),
        ..Error::new(
            "ASSET_WRITE_UNCERTAIN",
            "Result unknown. Repeat the same command with the original --idempotency-key; add --resume to send the saved snapshot if the local file changed or was deleted. Do not start a new key",
        )
    }
}

async fn send(client: &Client, journal: &Journal, receipt: &Receipt, compact: bool) -> Result<u8> {
    let operation = receipt.intent["operation"].as_str();
    let expected_org = json!(receipt.identity.org);
    let consistent = match receipt.asset_id {
        Some(id) => {
            !id.is_nil()
                && operation == Some("replace")
                && receipt.options["context"]["org_id"] == expected_org
                && receipt.options["context"]["dentry_id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .is_some_and(|id| !id.is_nil())
                && receipt.options["expected_revision"] == receipt.intent["expected_revision"]
                && receipt.options["reset_model_state"] == receipt.intent["reset_model_state"]
                && receipt.options["expected_state_revision"]
                    == receipt.intent["expected_state_revision"]
        }
        None => {
            operation == Some("upload")
                && receipt.options["destination"]["org_id"] == expected_org
                && receipt.options["destination"]["kind"] == "drive"
                && receipt.options["retention"] == "persistent"
                && receipt.options["conflict"] == receipt.intent["conflict"]
        }
    };
    if !consistent {
        return Err(journal_error(
            "Saved options do not match the receipt identity and intent",
        ));
    }
    // Decode once before dispatch: corrupt local options cannot become a write.
    let upload_options: Option<UploadOptions> = if receipt.asset_id.is_none() {
        Some(
            serde_json::from_value(receipt.options.clone())
                .map_err(|_| journal_error("Invalid saved upload options"))?,
        )
    } else {
        None
    };
    let replace_options: Option<ReplaceOptions> = if receipt.asset_id.is_some() {
        Some(
            serde_json::from_value(receipt.options.clone())
                .map_err(|_| journal_error("Invalid saved replace options"))?,
        )
    } else {
        None
    };
    for attempt in 0..3 {
        let file = AssetUploadFile {
            filename: receipt.filename.clone(),
            bytes: journal.bytes(receipt).map_err(|error| {
                if attempt > 0 {
                    uncertain(error, journal, receipt)
                } else {
                    error
                }
            })?,
            mime_type: None,
        };
        eprintln!(
            "Sending: attempt {} with the saved idempotency key; awaiting server persistence",
            attempt + 1
        );
        let request = async {
            let value = match receipt.asset_id {
                Some(id) => {
                    let asset = client
                        .replace_asset_content(
                            id,
                            file,
                            replace_options.as_ref().unwrap(),
                            &receipt.key,
                            receipt.identity.user,
                        )
                        .await?;
                    json!({"asset":asset,"skipped":false})
                }
                None => {
                    let result = client
                        .upload_asset(
                            file,
                            upload_options.as_ref().unwrap(),
                            &receipt.key,
                            receipt.identity.user,
                        )
                        .await?;
                    json!(result)
                }
            };
            let rev = value["asset"]["content"]["revision"]
                .as_u64()
                .filter(|n| *n > 0 || value["skipped"] == true)
                .ok_or_else(|| {
                    Error::new("INVALID_RESPONSE", "Asset has no valid content revision")
                })?;
            if let Some(expected_id) = receipt.asset_id
                && (value["asset"]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    != Some(expected_id)
                    || rev <= replace_options.as_ref().unwrap().expected_revision)
            {
                return Err(Error::new(
                    "INVALID_RESPONSE",
                    "Replace response did not preserve asset identity and advance its revision",
                ));
            }
            let mut value = value;
            value["idempotency_key"] = json!(receipt.key);
            value["revision"] = json!(rev);
            Ok(value)
        };
        let result = tokio::select! {
            result = request => result,
            _ = tokio::signal::ctrl_c() => return Err(uncertain(Error::new("INTERRUPTED", "Interrupted while sending"), journal, receipt)),
        };
        match result {
            Ok(value) => {
                if value["skipped"] == true {
                    eprintln!(
                        "Skipped: existing name retained; local bytes are not bound to the remote asset"
                    );
                } else {
                    eprintln!("Complete: server confirmed durable asset content");
                }
                crate::print(&value, compact)?;
                return Ok(0);
            }
            Err(error) if may_have_succeeded(&error) => {
                if attempt == 2 {
                    return Err(uncertain(error, journal, receipt));
                }
                eprintln!("Recovery: response uncertain; retrying the same saved request");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(250 << attempt)) => {},
                    _ = tokio::signal::ctrl_c() => return Err(uncertain(error, journal, receipt)),
                }
            }
            Err(mut error) => {
                error.idempotency_key = Some(receipt.key.clone().into_boxed_str());
                return Err(error);
            }
        }
    }
    unreachable!()
}
