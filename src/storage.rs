use crate::args::ConfigKey;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};
use virse_api::{Error, Result};

pub const PRODUCTION: &str = "https://api.virse.ai";

pub fn backend() -> Result<String> {
    match std::env::var("VIRSE_BACKEND_URL") {
        Ok(value) => virse_api::normalize_backend(&value),
        Err(std::env::VarError::NotPresent) => virse_api::normalize_backend(PRODUCTION),
        Err(_) => Err(Error::new(
            "INVALID_INPUT",
            "VIRSE_BACKEND_URL is not valid Unicode",
        )),
    }
}
#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Defaults {
    pub canvas: Option<uuid::Uuid>,
    pub org: Option<uuid::Uuid>,
}

#[derive(Serialize)]
pub struct PlacementIdentity<'a> {
    pub backend: &'a str,
    pub user: uuid::Uuid,
    pub org: uuid::Uuid,
    pub canvas: uuid::Uuid,
    pub key: &'a str,
}

pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn open() -> Result<Self> {
        let root = match std::env::var_os("VIRSE_CONFIG_DIR") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            Some(_) => return Err(Error::new("INVALID_INPUT", "VIRSE_CONFIG_DIR is empty")),
            None => directories::ProjectDirs::from("", "", "virse")
                .ok_or_else(|| Error::new("CONFIG_ERROR", "Cannot locate config directory"))?
                .config_dir()
                .to_path_buf(),
        };
        Ok(Self { root })
    }
    // Hash the tuple to avoid separators/path traversal and isolate profile/environment state.
    fn scope(profile: &str, backend: &str) -> String {
        digest(&serde_json::json!([profile, backend]).to_string())
    }
    pub fn defaults(&self, profile: &str, backend: &str) -> Result<Defaults> {
        let values: BTreeMap<String, Defaults> = self.read("config.json")?;
        Ok(values
            .get(&Self::scope(profile, backend))
            .cloned()
            .unwrap_or_default())
    }
    pub fn set(
        &self,
        profile: &str,
        backend: &str,
        key: &ConfigKey,
        value: uuid::Uuid,
    ) -> Result<()> {
        let _lock = self.lock()?;
        let mut values: BTreeMap<String, Defaults> = self.read("config.json")?;
        let entry = values.entry(Self::scope(profile, backend)).or_default();
        match key {
            ConfigKey::Canvas => entry.canvas = Some(value),
            ConfigKey::Org => entry.org = Some(value),
        }
        self.save("config.json", &values)
    }
    pub fn token(&self, profile: &str, backend: &str) -> Result<String> {
        match std::env::var("VIRSE_API_TOKEN") {
            Ok(value) if !value.is_empty() => return Ok(value),
            Ok(_) => return Err(Error::new("AUTH_REQUIRED", "VIRSE_API_TOKEN is empty")),
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(Error::new(
                    "AUTH_REQUIRED",
                    "VIRSE_API_TOKEN is not valid Unicode",
                ));
            }
            _ => {}
        }
        let values: BTreeMap<String, String> = self.read("credentials.json")?;
        values
            .get(&Self::scope(profile, backend))
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    "AUTH_REQUIRED",
                    "Set VIRSE_API_TOKEN or run auth login for this backend/profile",
                )
            })
    }
    pub fn save_token(&self, profile: &str, backend: &str, token: Option<&str>) -> Result<()> {
        let _lock = self.lock()?;
        let mut values: BTreeMap<String, String> = self.read("credentials.json")?;
        let scope = Self::scope(profile, backend);
        if let Some(token) = token {
            values.insert(scope, token.into());
        } else {
            values.remove(&scope);
        }
        self.save("credentials.json", &values)
    }
    pub fn prepare_submission(
        &self,
        backend: &str,
        account: &str,
        canvas: uuid::Uuid,
        key: &str,
        body: &virse_api::SubmitTask,
    ) -> Result<String> {
        let _lock = self.lock()?;
        let identity = serde_json::json!([backend, account, canvas, key]);
        let name = format!("submission-{}.json", digest(&identity.to_string()));
        let request_digest = digest(
            &serde_json::to_string(body)
                .map_err(|_| Error::new("INVALID_INPUT", "Cannot serialize request"))?,
        );
        let old: serde_json::Value = self.read(&name)?;
        if !old.is_null() && old["request_digest"] != request_digest {
            return Err(Error::new(
                "INVALID_INPUT",
                "This idempotency key was already used with another request",
            ));
        }
        if old.is_null() {
            self.save(&name, &serde_json::json!({"identity":identity,"request_digest":request_digest,"task_id":null}))?;
        }
        Ok(name)
    }
    pub fn resource_command(
        &self,
        backend: &str,
        user: uuid::Uuid,
        org: uuid::Uuid,
        key: &str,
        intent: &serde_json::Value,
        request: Option<&virse_api::ResourceMutation>,
    ) -> Result<Option<virse_api::ResourceMutation>> {
        let _lock = self.lock()?;
        let identity = serde_json::json!([backend, user, org, key]);
        let name = format!("resource-command-{}.json", digest(&identity.to_string()));
        let fingerprint = digest(&intent.to_string());
        let old: serde_json::Value = self.read(&name)?;
        if !old.is_null() {
            if old["intent_digest"] != fingerprint {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "This key belongs to another resource command; use the original command or a new key",
                ));
            }
            let frozen: virse_api::ResourceMutation =
                serde_json::from_value(old["request"].clone())
                    .map_err(|_| Error::new("CONFIG_ERROR", "Invalid resource recovery record"))?;
            if frozen.context.org_id != org {
                return Err(Error::new(
                    "CONFIG_ERROR",
                    "Recovery record belongs to another organization",
                ));
            }
            frozen.validate()?;
            return Ok(Some(frozen));
        }
        if let Some(request) = request {
            self.save(&name, &serde_json::json!({"identity":identity,"intent_digest":fingerprint,"request":request}))?;
            return Ok(Some(request.clone()));
        }
        Ok(None)
    }
    pub fn placement_command(
        &self,
        identity: &PlacementIdentity<'_>,
        intent: &serde_json::Value,
        request: Option<&virse_api::CreateAssetElement>,
    ) -> Result<Option<virse_api::CreateAssetElement>> {
        let _lock = self.lock()?;
        let identity_json = serde_json::to_string(identity)
            .map_err(|_| Error::new("CONFIG_ERROR", "Cannot encode placement identity"))?;
        let name = format!("canvas-placement-{}.json", digest(&identity_json));
        let fingerprint = digest(&intent.to_string());
        let old: serde_json::Value = self.read(&name)?;
        if !old.is_null() {
            if old["intent_digest"] != fingerprint {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "This key belongs to another placement; repeat the original command or use a new key",
                ));
            }
            let frozen: virse_api::CreateAssetElement =
                serde_json::from_value(old["request"].clone())
                    .map_err(|_| Error::new("CONFIG_ERROR", "Invalid canvas placement record"))?;
            if frozen.org_id != identity.org {
                return Err(Error::new(
                    "CONFIG_ERROR",
                    "Placement record belongs to another organization",
                ));
            }
            frozen.validate()?;
            return Ok(Some(frozen));
        }
        if let Some(request) = request {
            self.save(&name, &serde_json::json!({"identity": identity, "intent_digest": fingerprint, "request": request}))?;
            return Ok(Some(request.clone()));
        }
        Ok(None)
    }
    pub fn tool_run_command(
        &self,
        identity: &PlacementIdentity<'_>,
        intent: &serde_json::Value,
        request: Option<&virse_api::ToolRunRequest>,
    ) -> Result<Option<virse_api::ToolRunRequest>> {
        let _lock = self.lock()?;
        let identity_json = serde_json::to_string(identity)
            .map_err(|_| Error::new("CONFIG_ERROR", "Cannot encode tool run identity"))?;
        let name = format!("tool-run-{}.json", digest(&identity_json));
        let fingerprint = digest(&intent.to_string());
        let old: serde_json::Value = self.read(&name)?;
        if !old.is_null() {
            if old["intent_digest"] != fingerprint {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "This key belongs to another tool run; repeat the original command or use a new key",
                ));
            }
            let frozen: virse_api::ToolRunRequest = serde_json::from_value(old["request"].clone())
                .map_err(|_| Error::new("CONFIG_ERROR", "Invalid canvas tool run record"))?;
            if frozen.org_id != identity.org {
                return Err(Error::new(
                    "CONFIG_ERROR",
                    "Tool run record belongs to another organization",
                ));
            }
            frozen.validate()?;
            return Ok(Some(frozen));
        }
        if let Some(request) = request {
            self.save(&name, &serde_json::json!({"identity": identity, "intent_digest": fingerprint, "request": request}))?;
            return Ok(Some(request.clone()));
        }
        Ok(None)
    }
    pub fn accepted(&self, record: &str, task: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut value: serde_json::Value = self.read(record)?;
        value["task_id"] = task.into();
        self.save(record, &value)
    }
    pub(crate) fn read<T: DeserializeOwned + Default>(&self, name: &str) -> Result<T> {
        let path = self.root.join(name);
        match std::fs::read(&path) {
            Ok(bytes) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&path)
                        .map_err(io_error)?
                        .permissions()
                        .mode();
                    if mode & 0o077 != 0 {
                        return Err(Error::new(
                            "CONFIG_ERROR",
                            "Config files must have permissions 0600",
                        ));
                    }
                }
                serde_json::from_slice(&bytes)
                    .map_err(|_| Error::new("CONFIG_ERROR", format!("Invalid local file: {name}")))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
            Err(e) => Err(io_error(e)),
        }
    }
    pub(crate) fn lock(&self) -> Result<std::fs::File> {
        std::fs::create_dir_all(&self.root).map_err(io_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
                .map_err(io_error)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(self.root.join("state.lock"))
            .map_err(io_error)?;
        file.try_lock().map_err(|_| {
            Error::new(
                "CONFIG_BUSY",
                "Another CLI is updating local state; retry this command",
            )
        })?;
        Ok(file)
    }
    pub(crate) fn save(&self, name: &str, value: &impl Serialize) -> Result<()> {
        atomic_write(
            &self.root.join(name),
            &serde_json::to_vec_pretty(value)
                .map_err(|_| Error::new("CONFIG_ERROR", "Cannot serialize local state"))?,
        )
    }
}
pub fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.as_file().sync_all().map_err(io_error)?;
    file.persist(path).map_err(|e| io_error(e.error))?;
    Ok(())
}
pub fn io_error(_: std::io::Error) -> Error {
    Error::new("LOCAL_IO_ERROR", "Local file operation failed")
}
