use crate::storage::Store;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use virse_api::Error;

fn update_command() -> &'static str {
    if cfg!(windows) {
        "& ([scriptblock]::Create((Invoke-RestMethod 'https://virse.ai/install.ps1'))) -Version latest"
    } else {
        "curl -fsSL https://virse.ai/install.sh | sh -s -- --version latest"
    }
}
pub fn decorate(mut error: Error) -> Error {
    if error.code == "CLIENT_UPGRADE_REQUIRED" {
        let mut details = error
            .details
            .take()
            .map(|d| *d)
            .filter(|d| d.is_object())
            .unwrap_or_else(|| json!({}));
        details["current_contract_version"] = json!(virse_api::compatibility::CONTRACT_VERSION);
        details["cli_version"] = json!(env!("CARGO_PKG_VERSION"));
        details["update_command"] = json!(update_command());
        error.details = Some(Box::new(details));
        error.retryable = false;
    }
    error
}
#[derive(Default, Serialize, Deserialize)]
struct UpdateCache {
    checked_at: u64,
    latest: Option<String>,
}
fn newer(latest: &str, current: &str) -> bool {
    match (
        semver::Version::parse(latest.trim_start_matches('v')),
        semver::Version::parse(current),
    ) {
        (Ok(latest), Ok(current)) => {
            latest.pre.is_empty() && latest.cmp_precedence(&current).is_gt()
        }
        _ => false,
    }
}
pub async fn notify_update(store: &Store) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut cache: UpdateCache = store.read("cli-update.json").unwrap_or_default();
    if cache.checked_at == 0 || cache.checked_at > now || now - cache.checked_at >= 86400 {
        cache = UpdateCache {
            checked_at: now,
            latest: virse_api::compatibility::latest_cli_version().await,
        };
        if let Ok(_lock) = store.lock() {
            let _ = store.save("cli-update.json", &cache);
        }
    }
    if let Some(latest) = cache.latest.filter(|v| newer(v, env!("CARGO_PKG_VERSION"))) {
        eprintln!(
            "Virse CLI {latest} is available (current {}). Update: {}",
            env!("CARGO_PKG_VERSION"),
            update_command()
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_release_comparison() {
        assert!(newer("v0.10.0", "0.9.0"));
        assert!(!newer("v0.9.0", "0.10.0"));
        assert!(!newer("v1.0.0-rc.1", "0.1.0"));
        assert!(!newer("invalid", "0.1.0"));
        assert!(!newer("v1.0.0+build", "1.0.0"));
    }
    #[test]
    fn upgrade_has_command_and_policy() {
        let error = decorate(virse_api::compatibility::upgrade_error(Some(2)));
        let details = error.details.unwrap();
        assert_eq!(details["minimum_contract_version"], 2);
        assert_eq!(details["current_contract_version"], 1);
        assert!(
            details["update_command"]
                .as_str()
                .unwrap()
                .contains("https://virse.ai/install.")
        );
    }
}
