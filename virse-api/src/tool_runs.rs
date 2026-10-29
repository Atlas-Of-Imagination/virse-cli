use crate::{Client, Error, Result, ToolRun, ToolRunAction, ToolRunRequest};
use futures_util::{SinkExt, Stream, StreamExt};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use uuid::Uuid;

fn decode_run(value: Value, canvas: Uuid, id: Option<Uuid>) -> Result<ToolRun> {
    decode_canvas_run(value, canvas, id, "tool")
}
fn decode_canvas_run(value: Value, canvas: Uuid, id: Option<Uuid>, kind: &str) -> Result<ToolRun> {
    let run: ToolRun = serde_json::from_value(value).map_err(|_| Error::protocol())?;
    if run.kind != kind || run.canvas_id != canvas || id.is_some_and(|id| run.run_id != id) {
        return Err(Error::protocol());
    }
    Ok(run)
}
fn socket_denied(code: u16) -> Option<Error> {
    if matches!(code, 1008 | 4000 | 4002..=4006) {
        Some(Error {
            status: Some(if code == 4003 || code == 1008 {
                403
            } else {
                401
            }),
            ..Error::new(
                "WS_ACCESS_DENIED",
                "Canvas socket authentication or permission denied",
            )
        })
    } else {
        None
    }
}
impl Client {
    /// One attempt, with a caller-owned frozen request and idempotency key.
    pub async fn submit_tool_run(
        &self,
        canvas: Uuid,
        body: &ToolRunRequest,
        key: &str,
    ) -> Result<ToolRun> {
        body.validate()?;
        let value = self
            .request(
                Method::POST,
                &format!("v2/canvas/projects/{canvas}/tool-runs"),
                &[],
                Some(serde_json::to_value(body).map_err(|_| Error::protocol())?),
                Some(key),
            )
            .await?;
        let run = decode_run(value, canvas, None)?;
        if run.tool.as_ref().is_none_or(|tool| {
            tool.skill_id != body.tool.skill_id || tool.version_id != body.tool.version_id
        }) {
            return Err(Error::protocol());
        }
        Ok(run)
    }
    pub async fn submit_tool_run_as(
        &self,
        canvas: Uuid,
        body: &ToolRunRequest,
        key: &str,
        user: Uuid,
    ) -> Result<ToolRun> {
        self.expecting_user(user)
            .submit_tool_run(canvas, body, key)
            .await
    }
    pub async fn tool_run(&self, canvas: Uuid, org: Uuid, id: Uuid) -> Result<ToolRun> {
        let value = self
            .request(
                Method::GET,
                &format!("v2/canvas/projects/{canvas}/tool-runs/{id}"),
                &[("org_id", org.to_string())],
                None,
                None,
            )
            .await?;
        decode_run(value, canvas, Some(id))
    }
    /// Returns the actual server snapshot: an interrupt acknowledgement is not cancellation.
    pub async fn tool_run_action(
        &self,
        canvas: Uuid,
        org: Uuid,
        id: Uuid,
        action: ToolRunAction,
    ) -> Result<ToolRun> {
        let value = self
            .request(
                Method::POST,
                &format!(
                    "v2/canvas/projects/{canvas}/tool-runs/{id}/{}",
                    action.as_str()
                ),
                &[("org_id", org.to_string())],
                None,
                None,
            )
            .await?;
        decode_run(value, canvas, Some(id))
    }
    /// Canvas broadcasts need no subscription request. `active_users` is sent after registration
    /// and Redis subscription; only then GET to close the connect/read race. No HTTP polling.
    /// Dropping the stream only stops observation. The run continues independently.
    pub fn watch_tool_run(
        &self,
        canvas: Uuid,
        org: Uuid,
        id: Uuid,
    ) -> impl Stream<Item = Result<ToolRun>> + '_ {
        self.watch_canvas_run(canvas, org, id, "tool")
    }
    pub fn watch_aesthetic_training(
        &self,
        canvas: Uuid,
        org: Uuid,
        id: Uuid,
    ) -> impl Stream<Item = Result<ToolRun>> + '_ {
        self.watch_canvas_run(canvas, org, id, "aesthetic_training")
    }
    pub async fn aesthetic_training(&self, canvas: Uuid, org: Uuid, id: Uuid) -> Result<ToolRun> {
        self.read_canvas_run(canvas, org, id, "aesthetic_training")
            .await
    }
    pub async fn aesthetic_training_action(
        &self,
        canvas: Uuid,
        org: Uuid,
        id: Uuid,
        action: ToolRunAction,
    ) -> Result<ToolRun> {
        let action = match action {
            ToolRunAction::Cancel => "cancel",
            ToolRunAction::Resume => "resume",
            _ => {
                return Err(Error::new(
                    "INVALID_INPUT",
                    "Running training cannot be interrupted",
                ));
            }
        };
        let value = self
            .request(
                Method::POST,
                &format!("v2/aesthetics/canvases/{canvas}/training-runs/{id}/{action}"),
                &[("org_id", org.to_string())],
                None,
                None,
            )
            .await?;
        decode_canvas_run(value, canvas, Some(id), "aesthetic_training")
    }
    async fn read_canvas_run(
        &self,
        canvas: Uuid,
        org: Uuid,
        id: Uuid,
        kind: &str,
    ) -> Result<ToolRun> {
        if kind == "tool" {
            return self.tool_run(canvas, org, id).await;
        }
        let value = self
            .request(
                Method::GET,
                &format!("v2/aesthetics/canvases/{canvas}/training-runs/{id}"),
                &[("org_id", org.to_string())],
                None,
                None,
            )
            .await?;
        decode_canvas_run(value, canvas, Some(id), kind)
    }
    fn watch_canvas_run(
        &self,
        canvas: Uuid,
        org: Uuid,
        id: Uuid,
        kind: &'static str,
    ) -> impl Stream<Item = Result<ToolRun>> + '_ {
        async_stream::try_stream! {
            let mut revision: Option<u64> = None;
            let mut failures = 0u32;
            'reconnect: loop {
                if failures >= 5 { Err(Error::new("OBSERVATION_UNAVAILABLE", "Canvas observation recovery exhausted; use tool runs watch to reconnect"))?; }
                if failures > 0 { tokio::time::sleep(Duration::from_secs(1 << (failures - 1).min(3))).await; }
                failures += 1;
                let mut url = self.url(&format!("canvas/ws/{canvas}"))?;
                let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
                url.set_scheme(scheme).map_err(|_| Error::protocol())?;
                let mut request = url.as_str().into_client_request().map_err(|_| Error::protocol())?;
                request.headers_mut().insert("authorization", self.authorization.clone());
                let connected = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async(request)).await;
                let (mut socket, _) = match connected {
                    Ok(Ok(value)) => value,
                    Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) if matches!(response.status().as_u16(), 401 | 403) => {
                        Err(Error { status: Some(response.status().as_u16()), ..Error::new("WS_ACCESS_DENIED", "Canvas socket authentication or permission denied") })?;
                        unreachable!()
                    }
                    _ => continue,
                };
                let ready = async {
                    loop {
                        match socket.next().await {
                            Some(Ok(Message::Text(text))) => {
                                let value: Value = serde_json::from_str(&text).map_err(|_| Error::protocol())?;
                                if value["type"] == "active_users" { return Ok(true); }
                                if value["type"] == "error" { return Err(Error::new("OBSERVATION_UNAVAILABLE", "Canvas rejected observation")); }
                            }
                            Some(Ok(Message::Ping(bytes))) => { if socket.send(Message::Pong(bytes)).await.is_err() { return Ok(false); } }
                            Some(Ok(Message::Close(frame))) => {
                                if let Some(error) = frame.and_then(|f| socket_denied(u16::from(f.code))) { return Err(error); }
                                return Ok(false);
                            }
                            Some(Ok(_)) => {},
                            _ => return Ok(false),
                        }
                    }
                };
                match tokio::time::timeout(Duration::from_secs(15), ready).await {
                    Ok(Ok(true)) => {}, Ok(Err(error)) => Err(error)?, _ => continue,
                }
                // Socket buffers events while GET is in flight. Revision guards discard old replay.
                let run = match self.read_canvas_run(canvas, org, id, kind).await {
                    Ok(run) => run,
                    Err(error) if error.retryable => continue,
                    Err(error) => { Err(error)?; unreachable!() },
                };
                if revision.is_none_or(|r| run.revision > r) {
                    revision = Some(run.revision);
                    let done = run.stop_waiting();
                    yield run;
                    if done { return; }
                }
                let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
                heartbeat.tick().await;
                let mut last_received = tokio::time::Instant::now();
                let connected_at = tokio::time::Instant::now();
                loop {
                    let message = tokio::select! {
                        _ = heartbeat.tick() => {
                            if last_received.elapsed() > Duration::from_secs(60) { continue 'reconnect; }
                            if socket.send(Message::Text(json!({"type":"ping"}).to_string().into())).await.is_err() { continue 'reconnect; }
                            continue;
                        }
                        message = socket.next() => message,
                    };
                    let message = match message { Some(Ok(value)) => value, _ => continue 'reconnect };
                    last_received = tokio::time::Instant::now();
                    if connected_at.elapsed() > Duration::from_secs(60) { failures = 0; }
                    match message {
                        Message::Text(text) => {
                            let value: Value = serde_json::from_str(&text).map_err(|_| Error::protocol())?;
                            match value["type"].as_str() {
                                Some("canvas_run_updated") => {
                                    if value["project_id"].as_str() != Some(canvas.to_string().as_str())
                                        || value["run"]["run_id"].as_str() != Some(id.to_string().as_str()) { continue; }
                                    let run = decode_canvas_run(value["run"].clone(), canvas, Some(id), kind)?;
                                    if revision.is_some_and(|r| run.revision <= r) { continue; }
                                    revision = Some(run.revision);
                                    failures = 0;
                                    let done = run.stop_waiting();
                                    yield run;
                                    if done { return; }
                                }
                                Some("error") => Err(Error::new("OBSERVATION_UNAVAILABLE", "Canvas rejected observation"))?,
                                _ => {},
                            }
                        }
                        Message::Ping(bytes) => { if socket.send(Message::Pong(bytes)).await.is_err() { continue 'reconnect; } }
                        Message::Close(frame) => {
                            if let Some(error) = frame.and_then(|f| socket_denied(u16::from(f.code))) { Err(error)?; }
                            continue 'reconnect;
                        }
                        _ => {},
                    }
                }
            }
        }
    }
}
