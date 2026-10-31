use crate::{Client, Error, Result, Task};
use futures_util::{SinkExt, Stream, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use uuid::Uuid;

impl Client {
    /// Observes current snapshots, not an event history. Dropping the stream never cancels a task.
    /// Stops at terminal/needs_attention. Callers impose their own overall timeout.
    pub fn watch(&self, canvas: Uuid, task_id: Uuid) -> impl Stream<Item = Result<Task>> + '_ {
        async_stream::try_stream! {
            let mut revision = None;
            let mut failures = 0u32;
            loop {
                // Initial read and reconnect reconciliation only; no periodic HTTP polling.
                match self.task(canvas, task_id).await {
                    Ok(task) => {
                        if revision.is_none_or(|r| task.revision > r || (task.revision == r && task.stop_waiting())) {
                            revision = Some(task.revision);
                            let done = task.stop_waiting();
                            yield task;
                            if done { break; }
                        }
                    }
                    Err(error) if error.retryable => {},
                    Err(error) => Err(error)?,
                }
                if failures >= 5 { Err(Error::new("OBSERVATION_UNAVAILABLE", "Connection recovery exhausted; resume with generation watch"))?; }
                if failures > 0 { tokio::time::sleep(Duration::from_secs(1 << (failures - 1).min(4))).await; }
                failures += 1;
                let mut url = self.url(&format!("canvas/ws/{canvas}"))?;
                let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
                url.set_scheme(scheme).map_err(|_| Error::protocol())?;
                let mut request = url.as_str().into_client_request().map_err(|_| Error::protocol())?;
                request.headers_mut().insert("authorization", self.authorization.clone());
                self.contract_socket(request.headers_mut());
                self.ensure_compatibility().await?;
                let connected = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async(request)).await;
                let (mut socket, _) = match connected {
                    Ok(Ok(value)) => value,
                    Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) if crate::compatibility::is_upgrade_response(&response) => { Err(crate::compatibility::upgrade_error(None))?; unreachable!() }
                    Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) if matches!(response.status().as_u16(), 401 | 403) => {
                        Err(Error { status: Some(response.status().as_u16()), ..Error::new("WS_ACCESS_DENIED", "WebSocket authentication or permission denied") })?;
                        unreachable!()
                    }
                    _ => continue,
                };
                if socket.send(Message::Text(json!({"type":"generation_subscribe","data":{"task_ids":[task_id]}}).to_string().into())).await.is_err() { continue; }
                let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
                // The first interval tick is immediate; subscribe already wrote to the socket.
                // Delay ping so a closing peer cannot discard buffered final snapshots.
                heartbeat.tick().await;
                let mut last_received = tokio::time::Instant::now();
                let mut subscribed = false;
                let connected_at = tokio::time::Instant::now();
                loop {
                    let message = tokio::select! {
                        _ = heartbeat.tick() => {
                            if last_received.elapsed() > Duration::from_secs(60) || (!subscribed && connected_at.elapsed() > Duration::from_secs(15)) { break; }
                            if socket.send(Message::Text(json!({"type":"ping"}).to_string().into())).await.is_err() { break; }
                            continue;
                        }
                        message = socket.next() => message,
                    };
                    let message = match message { Some(Ok(message)) => message, _ => break };
                    last_received = tokio::time::Instant::now();
                            match message {
                                Message::Text(text) => {
                                    crate::compatibility::check_text(&text)?;
                                    let value: Value = serde_json::from_str(&text).map_err(|_| Error::protocol())?;
                                    match value["type"].as_str() {
                                        Some("generation_snapshot" | "generation_updated") => {
                                            let task: Task = serde_json::from_value(value["task"].clone()).map_err(|_| Error::protocol())?;
                                            if task.task_id != task_id.to_string() || task.project_id != canvas.to_string() { continue; }
                                            subscribed = true;
                                            failures = 0;
                                            if revision.is_some_and(|r| task.revision <= r) { continue; }
                                            revision = Some(task.revision);
                                            let done = task.stop_waiting();
                                            yield task;
                                            if done { return; }
                                        }
                                        Some("generation_subscription_error") => {
                                            if value["retryable"].as_bool() == Some(true) { break; }
                                            Err(self.scrub(Error::new(value["error"].as_str().unwrap_or("SUBSCRIPTION_REJECTED"), "Task subscription rejected")))?;
                                        }
                                        _ => {},
                                    }
                                }
                                Message::Ping(data) => { if socket.send(Message::Pong(data)).await.is_err() { break; } }
                                Message::Close(frame) => {
                                    crate::compatibility::check_close(frame.as_ref())?;
                                    if let Some(frame) = frame {
                                        let code = u16::from(frame.code);
                                        if matches!(code, 4000 | 4005 | 4006 | 4401 | 4403 | 4409 | 1008) {
                                            Err(Error { status: Some(if code == 4403 {403} else {401}), ..Error::new("WS_REJECTED", format!("WebSocket closed with code {code}")) })?;
                                        }
                                    }
                                    break;
                                }
                                _ => {},
                            }
                }
            }
        }
    }
}
