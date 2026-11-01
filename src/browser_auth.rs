use crate::storage::Store;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use virse_api::{
    AuthorizationProof, AuthorizationStart, AuthorizedKey, CliAuthClient, Client, Error, Result,
};

#[derive(Serialize, Deserialize)]
struct Pending {
    start: AuthorizationStart,
    proof: AuthorizationProof,
    state: String,
    redirect: Option<String>,
    expires: u64,
    key: Option<AuthorizedKey>,
}
#[derive(Serialize, Deserialize)]
pub struct Origin {
    pub key_id: String,
    pub user_id: String,
}
fn name(profile: &str, backend: &str, kind: &str) -> String {
    format!(
        "cli-auth-{kind}-{}.json",
        crate::storage::digest(&json!([profile, backend]).to_string())
    )
}
fn random() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::new("AUTH_RANDOM_FAILED", "Cannot generate authorization proof"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn persist(store: &Store, file: &str, pending: &Pending) -> Result<()> {
    let _guard = store.lock()?;
    store.save(file, &Some(pending))
}
pub fn origin(store: &Store, profile: &str, backend: &str) -> Result<Option<Origin>> {
    store.read(&name(profile, backend, "origin"))
}
pub fn clear_origin(store: &Store, profile: &str, backend: &str) -> Result<()> {
    let _guard = store.lock()?;
    store.save(&name(profile, backend, "pending"), &Option::<Pending>::None)?;
    store.save(&name(profile, backend, "origin"), &Option::<Origin>::None)
}
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    if result.is_err() {
        eprintln!("Could not open browser. Open the displayed URL manually.");
    }
}
async fn callback(listener: &TcpListener, pending: &Pending) -> Result<()> {
    loop {
        let (mut socket, _) = listener.accept().await.map_err(crate::storage::io_error)?;
        let read_line = async {
            let mut bytes = Vec::new();
            let mut chunk = [0; 1024];
            while bytes.len() < 8192 {
                let n = socket.read(&mut chunk).await?;
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.windows(2).any(|v| v == b"\r\n") {
                    break;
                }
            }
            Ok::<_, std::io::Error>(bytes)
        };
        let bytes = match tokio::time::timeout(Duration::from_secs(3), read_line).await {
            Ok(Ok(bytes)) if bytes.len() < 8192 => bytes,
            _ => continue,
        };
        let line = String::from_utf8_lossy(&bytes);
        let path = line
            .lines()
            .next()
            .unwrap_or("")
            .strip_prefix("GET ")
            .and_then(|s| s.split_once(" HTTP/1.").map(|v| v.0));
        let parsed = path.and_then(|p| url::Url::parse(&format!("http://127.0.0.1{p}")).ok());
        let valid = parsed.as_ref().is_some_and(|url| {
            let pairs: Vec<_> = url.query_pairs().collect();
            let q: BTreeMap<_, _> = pairs.iter().cloned().collect();
            url.path() == "/callback"
                && pairs.len() == q.len()
                && q.get("state").map(|v| v.as_ref()) == Some(pending.state.as_str())
                && q.get("code").map(|v| v.as_ref()) == Some(pending.start.request_id.as_str())
        });
        let response = if valid {
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nReturn to Virse CLI to finish signing in."
        } else {
            "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid authorization callback."
        };
        let _ = socket.write_all(response.as_bytes()).await;
        if valid {
            return Ok(());
        }
    }
}
async fn receive(
    auth: &CliAuthClient,
    pending: &Pending,
    listener: Option<&TcpListener>,
) -> Result<AuthorizedKey> {
    // 恢复时先领取一次：浏览器回调可能已成功，但此前 CLI 在保存前退出。
    let mut wait_callback = false;
    loop {
        if wait_callback {
            callback(listener.unwrap(), pending).await?;
        }
        let value = auth.exchange(&pending.proof).await?;
        match value["status"].as_str() {
            Some("authorization_pending" | "slow_down") => {
                wait_callback = listener.is_some();
                if !wait_callback {
                    tokio::time::sleep(Duration::from_secs(
                        value["interval"].as_u64().unwrap_or(5).clamp(5, 60),
                    ))
                    .await;
                }
            }
            None => {
                return serde_json::from_value(value)
                    .map_err(|_| Error::new("INVALID_RESPONSE", "Invalid authorization result"));
            }
            _ => {
                return Err(Error::new(
                    "INVALID_RESPONSE",
                    "Unexpected authorization status",
                ));
            }
        }
    }
}
pub fn guard(store: &Store, profile: &str, backend: &str) -> Result<std::fs::File> {
    let file = name(profile, backend, "pending");
    {
        let _guard = store.lock()?;
    }
    let mut lock_options = std::fs::OpenOptions::new();
    lock_options
        .create(true)
        .truncate(false)
        .write(true)
        .read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        lock_options.mode(0o600);
    }
    let _flow_lock = lock_options
        .open(store.root.join(format!("{file}.lock")))
        .map_err(crate::storage::io_error)?;
    _flow_lock.try_lock().map_err(|_| {
        Error::new(
            "AUTH_BUSY",
            "Another login is active for this profile and backend",
        )
    })?;
    Ok(_flow_lock)
}
pub async fn login(
    store: &Store,
    profile: &str,
    backend: &str,
    device: bool,
    no_browser: bool,
    compact: bool,
) -> Result<u8> {
    let file = name(profile, backend, "pending");
    let _flow_lock = guard(store, profile, backend)?;
    let existing: Option<Pending> = store.read(&file)?;
    let listener;
    let mut pending = if let Some(p) = existing {
        if p.expires.saturating_add(300) <= now() && p.key.is_none() {
            return Err(Error::new(
                "AUTHORIZATION_EXPIRED",
                "Saved authorization expired; run auth logout before starting again",
            ));
        }
        listener = match &p.redirect {
            Some(uri) => {
                let url = url::Url::parse(uri)
                    .map_err(|_| Error::new("CONFIG_ERROR", "Invalid callback record"))?;
                Some(
                    TcpListener::bind(("127.0.0.1", url.port().unwrap_or(0)))
                        .await
                        .map_err(crate::storage::io_error)?,
                )
            }
            None => None,
        };
        p
    } else {
        listener = if device {
            None
        } else {
            Some(
                TcpListener::bind("127.0.0.1:0")
                    .await
                    .map_err(crate::storage::io_error)?,
            )
        };
        let redirect = listener
            .as_ref()
            .map(|l| {
                l.local_addr()
                    .map(|a| format!("http://127.0.0.1:{}/callback", a.port()))
            })
            .transpose()
            .map_err(crate::storage::io_error)?;
        let verifier = random()?;
        let state = random()?;
        let device_name: String = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| std::env::consts::OS.into())
            .chars()
            .filter(|c| !c.is_control())
            .take(100)
            .collect();
        let device_name = if device_name.trim().is_empty() {
            "Virse CLI"
        } else {
            &device_name
        };
        let start = CliAuthClient::new(backend)?
            .start(
                if device { "device" } else { "loopback" },
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
                &state,
                redirect.as_deref(),
                device_name,
            )
            .await?;
        let proof = AuthorizationProof {
            request_id: start.request_id.clone(),
            device_secret: start.device_secret.clone(),
            code_verifier: verifier,
        };
        let p = Pending {
            expires: now() + start.expires_in,
            start,
            proof,
            state,
            redirect,
            key: None,
        };
        persist(store, &file, &p)?;
        p
    };
    eprintln!(
        "Authorize Virse CLI: {}\nVerify code: {}",
        pending.start.authorization_url, pending.start.user_code
    );
    if !no_browser {
        open_browser(&pending.start.authorization_url);
    }
    let auth = CliAuthClient::new(backend)?;
    if pending.key.is_none() {
        let key = tokio::select! {
            result=tokio::time::timeout(Duration::from_secs(pending.expires.saturating_sub(now()).max(30)),receive(&auth,&pending,listener.as_ref()))=>result.map_err(|_|Error::new("WAIT_TIMEOUT","Authorization expired; run auth logout to discard the pending request"))??,
            _=tokio::signal::ctrl_c()=>return Err(Error::new("INTERRUPTED","Authorization paused; rerun auth login to resume")),
        };
        pending.key = Some(key);
        persist(store, &file, &pending)?;
    }
    let key = pending.key.as_ref().unwrap();
    let me = Client::new(backend, &key.access_token)?.me().await?;
    if me["id"].as_str() != Some(key.user_id.as_str()) {
        return Err(Error::new(
            "IDENTITY_CHANGED",
            "Authorization account does not match",
        ));
    }
    store.save_token(profile, backend, Some(&key.access_token))?;
    {
        let _lock = store.lock()?;
        store.save(
            &name(profile, backend, "origin"),
            &Some(Origin {
                key_id: key.key_id.clone(),
                user_id: key.user_id.clone(),
            }),
        )?;
    }
    auth.acknowledge(&pending.proof).await?;
    {
        let _lock = store.lock()?;
        store.save(&file, &Option::<Pending>::None)?;
    }
    if std::env::var_os("VIRSE_API_TOKEN").is_some() {
        eprintln!("VIRSE_API_TOKEN still overrides this saved authorization.");
    }
    crate::print(
        &json!({"authenticated":true,"method":"browser","user":me,"key_id":key.key_id}),
        compact,
    )?;
    Ok(0)
}
pub async fn logout(store: &Store, profile: &str, backend: &str) -> Result<()> {
    let _flow_lock = guard(store, profile, backend)?;
    if let Some(origin) = origin(store, profile, backend)? {
        if std::env::var_os("VIRSE_API_TOKEN").is_some() {
            return Err(Error::new(
                "AUTH_ENV_OVERRIDE",
                "Unset VIRSE_API_TOKEN before revoking the saved browser authorization",
            ));
        }
        Client::new(backend, &store.token(profile, backend)?)?
            .revoke_api_token(&origin.key_id)
            .await?;
    }
    store.save_token(profile, backend, None)?;
    clear_origin(store, profile, backend)?;
    let _lock = store.lock()?;
    store.save(&name(profile, backend, "pending"), &Option::<Pending>::None)
}
