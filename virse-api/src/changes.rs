//! Permission-aware change pages and WebSocket-triggered catch-up. No cursor persistence.
use crate::{Client, Error, Result};
use futures_util::{SinkExt, Stream, StreamExt};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};
use tokio::time::{Instant, sleep, timeout, timeout_at};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Created,
    ContentUpdated,
    Renamed,
    Moved,
    Copied,
    Trashed,
    Restored,
    Purged,
    AccessChanged,
    ScopeInvalidated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeNotice {
    pub event_id: Uuid,
    pub kind: ChangeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_revision: Option<u64>,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesPage {
    pub items: Vec<ChangeNotice>,
    pub next_cursor: String,
    pub has_more: bool,
    pub bootstrap: bool,
}

/// A page is the delivery/checkpoint unit. Persist its cursor only after processing all items.
/// `scope_invalidated` items require callers to invalidate/requery their own workspace view.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangesWatchEvent {
    Page(ChangesPage),
    Notice {
        notice: String,
        cursor: Option<String>,
        attempt: u32,
    },
}

const MAX_RECONNECTS: u32 = 4;
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

fn validate_cursor(cursor: &str) -> Result<()> {
    if cursor.is_empty() || cursor.len() > 256 {
        return Err(Error::new(
            "INVALID_INPUT",
            "Changes cursor must contain 1..256 bytes",
        ));
    }
    Ok(())
}

fn recoverable(error: &Error) -> bool {
    // These statuses always require an explicit caller decision, even if retryable=true.
    !matches!(error.status, Some(401 | 403 | 409 | 503))
        && (error.code == "TRANSPORT_ERROR"
            || (error.retryable && matches!(error.status, Some(429 | 500 | 502 | 504))))
}

fn delay(attempt: u32) -> Duration {
    Duration::from_millis(250 * (1 << attempt.min(3)))
}

fn rejected(status: u16) -> Error {
    let (code, message) = match status {
        401 => (
            "AUTH_REQUIRED",
            "Changes authentication expired or was rejected",
        ),
        403 => (
            "workspace_access_revoked",
            "Workspace access revoked; observation stopped",
        ),
        409 => (
            "sync_reset_required",
            "Changes cursor requires an explicit new baseline and full read",
        ),
        503 => (
            "CHANGES_UNAVAILABLE",
            "Realtime changes are unavailable; observation stopped",
        ),
        _ => (
            "CHANGES_WS_REJECTED",
            "Changes WebSocket handshake rejected",
        ),
    };
    Error {
        status: Some(status),
        ..Error::new(code, message)
    }
}

impl Client {
    /// Fetch one authorized page. `None` obtains a baseline, never a historical full listing.
    /// 409 reset/expiry and 403 revocation remain errors; this never resets a cursor.
    pub async fn changes(&self, org: Uuid, cursor: Option<&str>) -> Result<ChangesPage> {
        let mut query = vec![("org_id", org.to_string())];
        if let Some(cursor) = cursor {
            validate_cursor(cursor)?;
            query.push(("cursor", cursor.to_owned()));
        }
        let value = self
            .request(Method::GET, "api/v1/changes", &query, None, None)
            .await
            .map_err(|mut error| {
                if error.status == Some(409)
                    && matches!(
                        error.code.as_str(),
                        "cursor_expired" | "sync_reset_required"
                    )
                {
                    error.message.push_str(
                        "; an explicit new baseline and full resource resync are required",
                    );
                }
                error
            })?;
        let page: ChangesPage = serde_json::from_value(value).map_err(|_| Error::protocol())?;
        if page.next_cursor.is_empty() || page.next_cursor.len() > 256 {
            return Err(Error::protocol());
        }
        if cursor.is_some() && page.bootstrap {
            return Err(rejected(409));
        }
        if (cursor.is_none() && (!page.bootstrap || !page.items.is_empty() || page.has_more))
            || ((page.has_more || !page.items.is_empty())
                && cursor == Some(page.next_cursor.as_str()))
            || page.items.iter().any(|item| {
                item.kind == ChangeKind::ScopeInvalidated
                    && (item.resource_id.is_some()
                        || item.asset_id.is_some()
                        || item.content_revision.is_some())
            })
        {
            return Err(Error::protocol());
        }
        Ok(page)
    }

    async fn changes_with_recovery(&self, org: Uuid, cursor: Option<&str>) -> Result<ChangesPage> {
        for attempt in 0..=3 {
            match self.changes(org, cursor).await {
                Err(error) if recoverable(&error) && attempt < 3 => sleep(delay(attempt)).await,
                result => return result,
            }
        }
        unreachable!()
    }

    /// Observe pages using WS hints, catching up on ready, available and every reconnect.
    /// Bearer authentication is sent only in the handshake header, never in the URL.
    /// No periodic HTTP polling or automatic baseline reset. Dropping the stream cancels it.
    /// This is an event stream, not materialized state: consumers apply content revisions and
    /// invalidate their own views. Hints carry no revisions and are never used as event data.
    /// The caller owns cursors and any overall timeout. Recovery is bounded (four reconnects
    /// per unstable connection sequence); healthy messages after a minute reset that budget.
    pub fn watch_changes(
        &self,
        org: Uuid,
        cursor: Option<&str>,
    ) -> impl Stream<Item = Result<ChangesWatchEvent>> + Send + '_ {
        let mut cursor = cursor.map(str::to_owned);
        async_stream::try_stream! {
            if let Some(value) = cursor.as_deref() {
                validate_cursor(value)?;
            } else {
                let page = self.changes_with_recovery(org, None).await?;
                cursor = Some(page.next_cursor.clone());
                yield ChangesWatchEvent::Page(page);
            }
            let mut reconnects = 0;
            loop {
                let mut url = self.url("api/v1/changes/ws")?;
                let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
                url.set_scheme(scheme).map_err(|_| Error::protocol())?;
                url.query_pairs_mut().append_pair("org_id", &org.to_string());
                let mut request = url.as_str().into_client_request().map_err(|_| Error::protocol())?;
                request.headers_mut().insert("authorization", self.authorization.clone());
                let connected = timeout(READY_TIMEOUT, tokio_tungstenite::connect_async(request)).await;
                match connected {
                    Ok(Ok((mut socket, _))) => {
                        let connected_at = Instant::now();
                        let mut ready = false;
                        loop {
                            let deadline = if ready { Instant::now() + IDLE_TIMEOUT } else { connected_at + READY_TIMEOUT };
                            let message = match timeout_at(deadline, socket.next()).await {
                                Ok(Some(Ok(message))) => message,
                                _ => break,
                            };
                            match message {
                                Message::Text(text) => {
                                    let value: Value = serde_json::from_str(&text).map_err(|_| Error::protocol())?;
                                    match value["type"].as_str() {
                                        Some("asset_changes_ready" | "asset_changes_available") => {
                                            let message_org = value["org_id"].as_str().and_then(|s| Uuid::parse_str(s).ok());
                                            if message_org != Some(org) { Err(Error::protocol())?; }
                                            if value["type"] == "asset_changes_ready" { ready = true; }
                                            // Notifications before ready cannot close the subscribe/read race.
                                            if !ready { continue; }
                                            let mut seen = HashSet::new();
                                            seen.insert(cursor.clone().ok_or_else(Error::protocol)?);
                                            loop {
                                                let page = self.changes_with_recovery(org, cursor.as_deref()).await?;
                                                let more = page.has_more;
                                                if more && !seen.insert(page.next_cursor.clone()) { Err(Error::protocol())?; }
                                                cursor = Some(page.next_cursor.clone());
                                                yield ChangesWatchEvent::Page(page);
                                                if !more { break; }
                                            }
                                        }
                                        Some("workspace_access_revoked") => Err(rejected(403))?,
                                        Some("heartbeat" | "pong") => {},
                                        _ => Err(Error::protocol())?,
                                    }
                                    // An idle timeout or close is not proof of a healthy connection.
                                    if ready && connected_at.elapsed() >= IDLE_TIMEOUT { reconnects = 0; }
                                }
                                Message::Ping(data) => {
                                    if !matches!(timeout(READY_TIMEOUT, socket.send(Message::Pong(data))).await, Ok(Ok(()))) { break; }
                                }
                                Message::Close(frame) => {
                                    if let Some(frame) = frame {
                                        match u16::from(frame.code) {
                                            4403 => Err(rejected(403))?,
                                            4000 | 4005 | 4006 | 4401 | 1008 => Err(rejected(401))?,
                                            4409 => Err(rejected(409))?,
                                            1013 => Err(rejected(503))?,
                                            _ => {},
                                        }
                                    }
                                    break;
                                }
                                Message::Binary(_) => Err(Error::protocol())?,
                                _ => {},
                            }
                        }
                    }
                    Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => {
                        let status = response.status().as_u16();
                        if !matches!(status, 429 | 500 | 502 | 504) {
                            let mut error = rejected(status);
                            // Preserve structured rejection codes, but never echo raw headers/URLs.
                            if let Some(body) = response.body().as_ref().and_then(|body| serde_json::from_slice::<Value>(body).ok()) {
                                if let Some(code) = body["error"]["code"].as_str() { error.code = code.into(); }
                                if let Some(message) = body["error"]["message"].as_str() { error.message = message.into(); }
                                error.request_id = body["request_id"].as_str().map(str::to_owned);
                            }
                            Err(self.scrub(error))?;
                        }
                    }
                    _ => {},
                }
                if reconnects >= MAX_RECONNECTS {
                    Err(Error::new("CHANGES_UNAVAILABLE", "Changes connection recovery exhausted; resume explicitly with the last delivered cursor"))?;
                }
                reconnects += 1;
                yield ChangesWatchEvent::Notice {
                    notice: "reconnecting".into(), cursor: cursor.clone(), attempt: reconnects,
                };
                sleep(delay(reconnects - 1)).await;
            }
        }
    }
}
