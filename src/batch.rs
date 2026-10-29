//! Local upload batches. Files are snapshotted before dispatch; remote folder creation is never guessed.
use crate::{print, resource_paths::Resolver, storage::Store, workflow_state};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use virse_api::{
    AssetUploadFile, Client, Destination, Error, Result, UploadConflict, UploadOptions,
};
const MAX_FILE: u64 = 100 * 1024 * 1024;
const MAX_BATCH: u64 = 2 * 1024 * 1024 * 1024;
#[derive(Clone, Copy, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    Rename,
    Skip,
    Error,
}
#[derive(Args)]
pub struct Import {
    #[arg(required=true,num_args=1..)]
    pub paths: Vec<PathBuf>,
    #[arg(long, default_value = "/")]
    pub parent: String,
    #[arg(long, value_enum, default_value = "rename")]
    pub conflict: Conflict,
    /// Prepare a durable batch but do not dispatch it yet.
    #[arg(long)]
    pub defer: bool,
    #[arg(long,default_value_t=1000,value_parser=clap::value_parser!(u32).range(1..=10000))]
    pub max_items: u32,
}
#[derive(Subcommand)]
pub enum Batch {
    List,
    /// Remove local snapshots/receipts of a terminal batch; remote resources are untouched.
    Forget {
        id: Uuid,
        #[arg(long)]
        yes: bool,
    },
    Get {
        id: Uuid,
    },
    Run {
        id: Uuid,
    },
    Resume {
        id: Uuid,
    },
    Pause {
        id: Uuid,
    },
    Cancel {
        id: Uuid,
    },
    /// Retry failed items; completed items are never reset. Unknown folders require bind-folder.
    Retry {
        id: Uuid,
        #[arg(long)]
        item: Vec<Uuid>,
        #[arg(long)]
        include_cancelled: bool,
    },
    /// Resolve an uncertain directory creation by explicitly selecting its actual folder ID.
    /// Resolve an uncertain file result using a matching server checksum and frozen parent.
    BindFile {
        id: Uuid,
        item: Uuid,
        resource: Uuid,
    },
    BindFolder {
        id: Uuid,
        item: Uuid,
        folder: Uuid,
    },
}
#[derive(Clone, Serialize, Deserialize)]
struct Item {
    id: Uuid,
    source: String,
    name: String,
    parent: Option<Uuid>,
    kind: String,
    status: String,
    hash: Option<String>,
    size: u64,
    key: String,
    resource_id: Option<Uuid>,
    remote_name: Option<String>,
    error: Option<Value>,
    result: Option<Value>,
}
#[derive(Serialize, Deserialize)]
struct Job {
    version: u32,
    id: Uuid,
    backend: String,
    user: Uuid,
    org: Uuid,
    parent: Option<Uuid>,
    conflict: Conflict,
    status: String,
    items: Vec<Item>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Control {
    Run,
    Pause,
    Cancel,
}
struct Files {
    dir: PathBuf,
}
fn fail(s: &str) -> Error {
    Error::new("BATCH_STATE_ERROR", s)
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && !matches!(name, "." | "..")
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn private_dir(path: &Path) -> Result<()> {
    crate::resource_upload::private_directory(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(crate::storage::io_error)?;
    }
    Ok(())
}
fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| fail("Invalid state path"))?)
            .map_err(crate::storage::io_error)?;
    serde_json::to_writer(&mut temp, value).map_err(|_| fail("Cannot encode batch state"))?;
    temp.flush().map_err(crate::storage::io_error)?;
    temp.as_file()
        .sync_all()
        .map_err(crate::storage::io_error)?;
    temp.persist(path)
        .map_err(|e| crate::storage::io_error(e.error))?;
    Ok(())
}
impl Files {
    fn at(store: &Store, id: Uuid) -> Result<Self> {
        let dir = store.root.join("batches").join(id.to_string());
        crate::tool_project::open_directory(&dir)?;
        Ok(Self { dir })
    }
    fn read(&self) -> Result<Job> {
        let data =
            crate::resource_upload::read_file(&self.dir.join("job.json"), 64 * 1024 * 1024, true)?;
        let job: Job = serde_json::from_slice(&data).map_err(|_| fail("Invalid job.json"))?;
        if job.version != 1
            || job.items.len() > 10000
            || self.dir.file_name().and_then(|s| s.to_str()) != Some(&job.id.to_string())
        {
            return Err(fail("Batch identity or version is invalid"));
        }
        let mut seen = std::collections::HashSet::new();
        for item in &job.items {
            if !valid_name(&item.name)
                || !matches!(item.kind.as_str(), "file" | "folder")
                || !matches!(
                    item.status.as_str(),
                    "queued"
                        | "running"
                        | "unknown"
                        | "failed"
                        | "succeeded"
                        | "skipped"
                        | "blocked"
                        | "cancelled"
                )
                || item.key != format!("import-{}", item.id)
                || item.parent.is_some_and(|id| !seen.contains(&id))
                || !seen.insert(item.id)
                || item.size > MAX_FILE
            {
                return Err(fail("Invalid batch item identity, hierarchy or state"));
            }
        }
        Ok(job)
    }
    fn save(&self, job: &Job) -> Result<()> {
        write(&self.dir.join("job.json"), job)
    }
    fn control(&self) -> Result<Control> {
        let data = crate::resource_upload::read_file(&self.dir.join("control.json"), 1024, true)?;
        serde_json::from_slice(&data).map_err(|_| fail("Invalid batch control"))
    }
    fn set_control(&self, value: Control) -> Result<()> {
        write(&self.dir.join("control.json"), &value)
    }
    fn lock(&self) -> Result<File> {
        let file = crate::resource_upload::open_file(&self.dir.join("worker.lock"), true, true)?;
        file.try_lock()
            .map_err(|_| Error::new("BATCH_BUSY", "A worker is already running this batch"))?;
        Ok(file)
    }
    fn bytes(&self, item: &Item) -> Result<Vec<u8>> {
        if !valid_name(&item.name) {
            return Err(fail("Unsafe stored filename"));
        }
        let path = self
            .dir
            .join("files")
            .join(item.id.to_string())
            .join(&item.name);
        let bytes = crate::resource_upload::read_file(&path, MAX_FILE, true)?;
        if bytes.len() as u64 != item.size
            || item.hash.as_deref() != Some(&format!("{:x}", Sha256::digest(&bytes)))
        {
            return Err(fail("Staged bytes changed; refusing to upload"));
        }
        Ok(bytes)
    }
}
fn summary(files: &Files, job: &Job) -> Result<Value> {
    let active = files.lock().is_err();
    Ok(
        json!({"batch_id":job.id,"backend":job.backend,"user_id":job.user,"org_id":job.org,"status":job.status,"requested_control":files.control()?,"worker_active":active,"items":job.items}),
    )
}
pub fn offline(command: &Batch, store: &Store, compact: bool) -> Option<Result<u8>> {
    let action = || -> Result<u8> {
        match command {
            Batch::List => {
                let mut jobs = vec![];
                let root = store.root.join("batches");
                if root.exists() {
                    crate::tool_project::open_directory(&root)?;
                    for entry in fs::read_dir(root).map_err(crate::storage::io_error)? {
                        let entry = entry.map_err(crate::storage::io_error)?;
                        if let Some(id) = entry
                            .file_name()
                            .to_str()
                            .and_then(|s| Uuid::parse_str(s).ok())
                        {
                            let files = Files::at(store, id)?;
                            let job = files.read()?;
                            jobs.push(json!({"batch_id":id,"backend":job.backend,"user_id":job.user,"org_id":job.org,"status":job.status,"requested_control":files.control()?,"items":job.items.len()}));
                        }
                    }
                }
                print(&json!({"batches":jobs}), compact)?;
            }
            Batch::Forget { id, yes } => {
                if !yes {
                    return Err(Error::new(
                        "INVALID_INPUT",
                        "Removing local recovery snapshots requires --yes",
                    ));
                }
                let files = Files::at(store, *id)?;
                let guard = files.lock()?;
                let job = files.read()?;
                if job.items.iter().any(|v| {
                    matches!(
                        v.status.as_str(),
                        "queued" | "blocked" | "running" | "unknown"
                    )
                }) {
                    return Err(fail(
                        "Resolve or cancel outstanding items before removing recovery data",
                    ));
                }
                let tombstone =
                    files
                        .dir
                        .with_file_name(format!(".removing-{}-{}", id, Uuid::new_v4()));
                fs::rename(&files.dir, &tombstone).map_err(crate::storage::io_error)?;
                drop(guard);
                fs::remove_dir_all(tombstone).map_err(crate::storage::io_error)?;
                print(
                    &json!({"batch_id":id,"local_state_removed":true,"remote_resources_changed":false}),
                    compact,
                )?;
            }
            Batch::Get { id } => {
                let files = Files::at(store, *id)?;
                print(&summary(&files, &files.read()?)?, compact)?;
            }
            Batch::Pause { id } | Batch::Cancel { id } => {
                let files = Files::at(store, *id)?;
                files.set_control(if matches!(command, Batch::Pause { .. }) {
                    Control::Pause
                } else {
                    Control::Cancel
                })?;
                if let Ok(_lock) = files.lock() {
                    let mut job = files.read()?;
                    apply_control(&mut job, files.control()?);
                    files.save(&job)?;
                }
                print(&summary(&files, &files.read()?)?, compact)?;
            }
            _ => return Err(fail("Not a local command")),
        }
        Ok(0)
    };
    if matches!(
        command,
        Batch::List
            | Batch::Forget { .. }
            | Batch::Get { .. }
            | Batch::Pause { .. }
            | Batch::Cancel { .. }
    ) {
        Some(action())
    } else {
        None
    }
}
fn supported(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "png"
                | "jpg"
                | "jpeg"
                | "webp"
                | "gif"
                | "bmp"
                | "avif"
                | "mp4"
                | "mov"
                | "webm"
                | "m4v"
                | "docx"
                | "pdf"
                | "glb"
        )
    })
}
fn scan(
    path: &Path,
    parent: Option<Uuid>,
    depth: u8,
    files: &Files,
    job: &mut Job,
    limit: usize,
    total: &mut u64,
) -> Result<()> {
    if job.items.len() >= limit || depth > 64 {
        return Err(Error::new(
            "INVALID_INPUT",
            "Import exceeds item/depth limit; split the directory",
        ));
    }
    let id = Uuid::new_v4();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned();
    if !valid_name(&name) {
        return Err(Error::new(
            "INVALID_INPUT",
            "Import paths must have UTF-8 names without separators/control characters",
        ));
    }
    let mut item = Item {
        id,
        source: path.display().to_string(),
        name,
        parent,
        kind: "file".into(),
        status: "queued".into(),
        hash: None,
        size: 0,
        key: format!("import-{}", id),
        resource_id: None,
        remote_name: None,
        error: None,
        result: None,
    };
    let meta = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(_) => {
            item.status = "failed".into();
            item.error =
                Some(json!({"code":"SOURCE_UNREADABLE","message":"Cannot read import source"}));
            job.items.push(item);
            return Ok(());
        }
    };
    if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
        item.status = "skipped".into();
        item.error = Some(
            json!({"code":"UNSUPPORTED_SOURCE","message":"Symlinks and special files are not imported"}),
        );
        job.items.push(item);
        return Ok(());
    }
    if meta.is_dir() {
        crate::tool_project::open_directory(path)?;
        item.kind = "folder".into();
        job.items.push(item);
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).map_err(crate::storage::io_error)? {
            entries.push(entry.map_err(crate::storage::io_error)?.path());
            if entries.len() + job.items.len() > limit {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "Directory exceeds import item limit; split the batch",
                ));
            }
        }
        entries.sort();
        for entry in entries {
            scan(&entry, Some(id), depth + 1, files, job, limit, total)?;
        }
        return Ok(());
    }
    if !supported(path) {
        item.status = "skipped".into();
        item.error = Some(
            json!({"code":"UNSUPPORTED_TYPE","message":"File type is outside the upload contract"}),
        );
        job.items.push(item);
        return Ok(());
    }
    let data = match crate::resource_upload::read_file(path, MAX_FILE, false) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => {
            item.status = "failed".into();
            item.error = Some(json!({"code":"EMPTY_FILE"}));
            job.items.push(item);
            return Ok(());
        }
        Err(error) => {
            item.status = "failed".into();
            item.error = Some(json!(error));
            job.items.push(item);
            return Ok(());
        }
    };
    *total += data.len() as u64;
    if *total > MAX_BATCH {
        return Err(Error::new(
            "INVALID_INPUT",
            "Import snapshot exceeds 2 GiB; split the batch",
        ));
    }
    let dir = files.dir.join("files").join(id.to_string());
    private_dir(&dir)?;
    let mut output = crate::resource_upload::open_file(&dir.join(&item.name), true, true)?;
    output.write_all(&data).map_err(crate::storage::io_error)?;
    output.sync_all().map_err(crate::storage::io_error)?;
    item.hash = Some(format!("{:x}", Sha256::digest(&data)));
    item.size = data.len() as u64;
    job.items.push(item);
    Ok(())
}
pub async fn create(
    args: &Import,
    client: &Client,
    store: &Store,
    backend: &str,
    org: Uuid,
    compact: bool,
) -> Result<u8> {
    let user = workflow_state::user(client).await?;
    let parent = Resolver::new(client, org).folder(&args.parent).await?;
    {
        let _lock = store.lock()?;
    }
    let root = store.root.join("batches");
    private_dir(&root)?;
    let temp = tempfile::tempdir_in(&root).map_err(crate::storage::io_error)?;
    let files = Files {
        dir: temp.path().to_owned(),
    };
    private_dir(&files.dir.join("files"))?;
    let id = Uuid::new_v4();
    let mut job = Job {
        version: 1,
        id,
        backend: backend.into(),
        user,
        org,
        parent,
        conflict: args.conflict,
        status: "prepared".into(),
        items: vec![],
    };
    let mut total = 0;
    for path in &args.paths {
        scan(
            path,
            None,
            0,
            &files,
            &mut job,
            args.max_items as usize,
            &mut total,
        )?;
    }
    if job.items.is_empty() {
        return Err(Error::new("INVALID_INPUT", "No import entries"));
    }
    files.save(&job)?;
    files.set_control(Control::Run)?;
    let destination = root.join(id.to_string());
    fs::rename(temp.path(), &destination).map_err(crate::storage::io_error)?;
    eprintln!(
        "Prepared batch {id}: {} entries, {total} staged bytes",
        job.items.len()
    );
    if args.defer {
        print(&summary(&Files { dir: destination }, &job)?, compact)?;
        return Ok(0);
    }
    run(
        &Batch::Run { id },
        client,
        store,
        backend,
        Some(org),
        compact,
    )
    .await
}
fn parent_for(job: &Job, item: &Item) -> Result<Option<Uuid>> {
    match item.parent {
        None => Ok(job.parent),
        Some(id) => {
            let parent = job
                .items
                .iter()
                .find(|v| v.id == id)
                .ok_or_else(|| fail("Missing parent item"))?;
            if parent.status != "succeeded" {
                return Err(fail("Parent folder has not completed"));
            }
            parent
                .resource_id
                .map(Some)
                .ok_or_else(|| fail("Folder result missing ID"))
        }
    }
}
fn apply_control(job: &mut Job, control: Control) {
    if job
        .items
        .iter()
        .all(|item| matches!(item.status.as_str(), "succeeded" | "skipped"))
    {
        job.status = "completed".into();
        return;
    }
    if control == Control::Cancel {
        for item in &mut job.items {
            if matches!(item.status.as_str(), "queued" | "blocked") {
                item.status = "cancelled".into();
            }
        }
        job.status = "cancelled".into();
    }
    if control == Control::Pause {
        job.status = "paused".into();
    }
    if job.items.iter().any(|v| v.status == "unknown") {
        job.status = "needs_attention".into();
    }
}
fn finish(job: &mut Job) -> u8 {
    if job.items.iter().any(|v| v.status == "unknown") {
        job.status = "needs_attention".into();
        7
    } else if job
        .items
        .iter()
        .any(|v| matches!(v.status.as_str(), "failed" | "blocked" | "cancelled"))
    {
        job.status = "partially_completed".into();
        6
    } else {
        job.status = "completed".into();
        0
    }
}
pub async fn run(
    command: &Batch,
    client: &Client,
    store: &Store,
    backend: &str,
    selected_org: Option<Uuid>,
    compact: bool,
) -> Result<u8> {
    let id = match command {
        Batch::Run { id }
        | Batch::Resume { id }
        | Batch::Retry { id, .. }
        | Batch::BindFolder { id, .. }
        | Batch::BindFile { id, .. } => *id,
        _ => return Err(fail("Unsupported online batch command")),
    };
    let files = Files::at(store, id)?;
    let _guard = files.lock()?;
    let mut job = files.read()?;
    let user = workflow_state::user(client).await?;
    if job.backend != backend || job.user != user || selected_org.is_some_and(|org| org != job.org)
    {
        return Err(Error::new(
            "BATCH_IDENTITY_CHANGED",
            "Batch is pinned to another backend, account or organization",
        ));
    }
    eprintln!("Batch {} uses pinned organization {}", job.id, job.org);
    if let Batch::BindFile { item, resource, .. } = command {
        let index = job
            .items
            .iter()
            .position(|v| v.id == *item)
            .ok_or_else(|| fail("Unknown batch item"))?;
        let entry = &job.items[index];
        if entry.kind != "file"
            || entry.hash.is_none()
            || !matches!(entry.status.as_str(), "unknown" | "failed" | "running")
        {
            return Err(fail("Only uncertain/failed snapshotted files can be bound"));
        }
        let parent = parent_for(&job, entry)?;
        let detail = client.resource(job.org, *resource).await?;
        let asset = detail
            .asset
            .as_ref()
            .ok_or_else(|| fail("Selected resource has no byte asset"))?;
        if detail.resource.parent_id != parent
            || asset
                .pointer("/content/checksum/algorithm")
                .and_then(Value::as_str)
                != Some("sha256")
            || asset
                .pointer("/content/checksum/value")
                .and_then(Value::as_str)
                != entry.hash.as_deref()
            || asset.pointer("/content/size_bytes").and_then(Value::as_u64) != Some(entry.size)
        {
            return Err(fail(
                "Resource parent, size or checksum does not match the frozen file",
            ));
        }
        job.items[index].resource_id = Some(*resource);
        job.items[index].result = Some(
            json!({"resource":detail.resource,"asset":asset,"recovered_by":"explicit_binding"}),
        );
        job.items[index].error = None;
        job.items[index].status = "succeeded".into();
        files.save(&job)?;
        print(
            &json!({"batch_id":id,"bound_item":item,"resource_id":resource}),
            compact,
        )?;
        return Ok(0);
    }
    if let Batch::BindFolder { item, folder, .. } = command {
        let index = job
            .items
            .iter()
            .position(|v| v.id == *item)
            .ok_or_else(|| fail("Unknown batch item"))?;
        if job.items[index].kind != "folder"
            || !matches!(
                job.items[index].status.as_str(),
                "unknown" | "failed" | "running"
            )
        {
            return Err(fail("Only uncertain/failed folder items can be bound"));
        }
        let expected = parent_for(&job, &job.items[index])?;
        let actual = client.resource(job.org, *folder).await?.resource;
        if !actual.is_folder() || actual.parent_id != expected {
            return Err(Error::new(
                "INVALID_INPUT",
                "Selected folder is not under the frozen parent",
            ));
        }
        job.items[index].resource_id = Some(actual.id);
        job.items[index].remote_name = Some(actual.name);
        job.items[index].status = "succeeded".into();
        job.items[index].error = None;
        files.save(&job)?;
        print(
            &json!({"batch_id":id,"bound_item":item,"resource_id":folder}),
            compact,
        )?;
        return Ok(0);
    }
    if let Batch::Retry {
        item,
        include_cancelled,
        ..
    } = command
    {
        if item.iter().any(|id| !job.items.iter().any(|v| v.id == *id)) {
            return Err(fail("Retry includes unknown item ID"));
        }
        for entry in &mut job.items {
            if !item.is_empty() && !item.contains(&entry.id) {
                continue;
            }
            let retry = entry.status == "failed"
                || entry.status == "blocked"
                || (*include_cancelled && entry.status == "cancelled")
                || (entry.kind == "file" && entry.status == "unknown");
            if retry && (entry.kind == "folder" || entry.hash.is_some()) {
                entry.status = "queued".into();
                entry.error = None;
            }
        }
    }
    for item in &mut job.items {
        if item.status == "running" {
            item.status = if item.kind == "folder" {
                "unknown"
            } else {
                "queued"
            }
            .into();
            item.error = Some(
                json!({"code":"INTERRUPTED_WORKER","message":"Prior worker ended before checkpointing the response"}),
            );
        }
        if item.kind == "file"
            && item.status == "unknown"
            && matches!(command, Batch::Resume { .. })
        {
            item.status = "queued".into();
        }
    }
    if matches!(command, Batch::Resume { .. } | Batch::Retry { .. }) {
        files.set_control(Control::Run)?;
    }
    job.status = "running".into();
    files.save(&job)?;
    for index in 0..job.items.len() {
        let control = files.control()?;
        if control != Control::Run {
            apply_control(&mut job, control);
            files.save(&job)?;
            print(&summary(&files, &job)?, compact)?;
            return Ok(if job.status == "needs_attention" {
                7
            } else {
                0
            });
        }
        if !matches!(job.items[index].status.as_str(), "queued" | "blocked") {
            continue;
        }
        if let Some(parent) = job.items[index].parent {
            let parent = job
                .items
                .iter()
                .find(|v| v.id == parent)
                .ok_or_else(|| fail("Missing parent"))?;
            if matches!(parent.status.as_str(), "skipped" | "cancelled") {
                job.items[index].status = parent.status.clone();
                job.items[index].error = Some(json!({"code":"PARENT_SKIPPED"}));
                files.save(&job)?;
                continue;
            }
        }
        let parent = match parent_for(&job, &job.items[index]) {
            Ok(v) => v,
            Err(_) => {
                job.items[index].status = "blocked".into();
                job.items[index].error = Some(json!({"code":"PARENT_NOT_READY"}));
                files.save(&job)?;
                continue;
            }
        };
        if job.items[index].kind == "folder" && job.items[index].remote_name.is_none() {
            let mut resolver = Resolver::new(client, job.org);
            let names = resolver.children(parent).await?;
            let exists = names.iter().any(|v| v.name == job.items[index].name);
            if exists {
                match job.conflict {
                    Conflict::Skip => {
                        job.items[index].status = "skipped".into();
                        job.items[index].error = Some(
                            json!({"code":"FOLDER_NAME_EXISTS","message":"Skipped directory and descendants; no merge was assumed"}),
                        );
                        files.save(&job)?;
                        continue;
                    }
                    Conflict::Error => {
                        job.items[index].status = "failed".into();
                        job.items[index].error = Some(json!({"code":"FOLDER_NAME_EXISTS"}));
                        files.save(&job)?;
                        continue;
                    }
                    Conflict::Rename => {
                        job.items[index].remote_name = Some(format!(
                            "{}-{}",
                            job.items[index].name.chars().take(246).collect::<String>(),
                            &job.items[index].id.to_string()[..8]
                        ))
                    }
                }
            } else {
                job.items[index].remote_name = Some(job.items[index].name.clone());
            }
        }
        let item = job.items[index].clone();
        job.items[index].status = "running".into();
        files.save(&job)?;
        eprintln!(
            "Batch item {}/{}: {}",
            index + 1,
            job.items.len(),
            crate::resource_paths::label(&item.name)
        );
        let request = async {
            if item.kind == "folder" {
                let folder = client
                    .create_folder(
                        job.org,
                        parent,
                        item.remote_name.as_deref().unwrap_or(&item.name),
                    )
                    .await?;
                Ok((Some(folder.id), json!({"resource":folder}), false))
            } else {
                let bytes = files.bytes(&item)?;
                let mut options = UploadOptions::new(Destination::folder(job.org, parent));
                options.conflict = match job.conflict {
                    Conflict::Rename => UploadConflict::Rename,
                    Conflict::Skip => UploadConflict::Skip,
                    Conflict::Error => UploadConflict::Error,
                };
                let result = client
                    .upload_asset(
                        AssetUploadFile::new(item.name.clone(), bytes),
                        &options,
                        &item.key,
                        user,
                    )
                    .await?;
                let resource_id = result.resource.as_ref().map(|v| v.id).ok_or_else(|| {
                    Error::new(
                        "INVALID_RESPONSE",
                        "Directory upload response has no resource entry",
                    )
                })?;
                Ok((Some(resource_id), json!(result), result.skipped))
            }
        };
        let result: Result<(Option<Uuid>, Value, bool)> = tokio::select! {value=request=>value,_=tokio::signal::ctrl_c()=>{
            job.items[index].status="unknown".into();job.items[index].error=Some(json!({"code":"INTERRUPTED","message":"In-flight request may have completed"}));job.status="needs_attention".into();if files.control()? == Control::Cancel {apply_control(&mut job,Control::Cancel);} else {files.set_control(Control::Pause)?;} files.save(&job)?;print(&summary(&files,&job)?,compact)?;return Ok(7);
        }};
        match result {
            Ok((id, value, skipped)) => {
                job.items[index].resource_id = id;
                job.items[index].result = Some(value);
                job.items[index].error = None;
                job.items[index].status = if skipped { "skipped" } else { "succeeded" }.into();
            }
            Err(error) => {
                let error = workflow_state::uncertain(
                    error,
                    "Batch write uncertain; resume files with their same keys, or explicitly bind the created folder",
                    if item.kind == "file" {
                        Some(item.key.as_str())
                    } else {
                        None
                    },
                );
                job.items[index].status = if error.code == "WORKFLOW_UNCERTAIN" {
                    "unknown"
                } else {
                    "failed"
                }
                .into();
                job.items[index].error = Some(json!(error));
            }
        }
        files.save(&job)?;
    }
    let control = files.control()?;
    let code = if control == Control::Run {
        finish(&mut job)
    } else {
        apply_control(&mut job, control);
        if job.status == "needs_attention" {
            7
        } else {
            0
        }
    };
    files.save(&job)?;
    print(&summary(&files, &job)?, compact)?;
    Ok(code)
}
