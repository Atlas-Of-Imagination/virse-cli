//! Composer gateway: messages are single-attempt, never idempotent generation submissions.
use crate::{Client, Error, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposerAccepted {
    pub accepted: bool,
    pub run_id: String,
    pub agent_turn_id: Option<String>,
    pub agent_block_id: Uuid,
    pub session_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposerEvent {
    pub event: String,
    pub id: Option<String>,
    pub data: Value,
}
#[derive(Default)]
struct Sse {
    pending: Vec<u8>,
}
impl Sse {
    fn feed(&mut self, bytes: &[u8], eof: bool) -> Result<Vec<ComposerEvent>> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > 8 * 1024 * 1024 {
            return Err(Error::new("INVALID_RESPONSE", "SSE frame exceeds 8 MiB"));
        }
        let mut out = vec![];
        loop {
            let boundary = self
                .pending
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|i| (i, 2))
                .into_iter()
                .chain(
                    self.pending
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|i| (i, 4)),
                )
                .min();
            let (end, sep) = match boundary {
                Some(x) => x,
                None if eof && !self.pending.is_empty() => (self.pending.len(), 0),
                None => break,
            };
            let raw = String::from_utf8(self.pending.drain(..end + sep).collect())
                .map_err(|_| Error::protocol())?;
            let (mut event, mut id, mut data) = ("message".to_string(), None, vec![]);
            for line in raw.lines() {
                if line.starts_with(':') {
                    continue;
                }
                let (k, v) = line.split_once(':').unwrap_or((line, ""));
                let v = v.strip_prefix(' ').unwrap_or(v);
                match k {
                    "event" => event = v.into(),
                    "id" => id = Some(v.into()),
                    "data" => data.push(v),
                    _ => {}
                }
            }
            if !data.is_empty() {
                out.push(ComposerEvent {
                    event,
                    id,
                    data: serde_json::from_str(&data.join("\n")).map_err(|_| Error::protocol())?,
                });
            }
        }
        Ok(out)
    }
}
pub struct ComposerSocket {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}
impl ComposerSocket {
    pub async fn next(&mut self) -> Result<Value> {
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(20), self.socket.next()).await
            {
                Err(_) => {
                    self.socket
                        .send(Message::Text(json!({"type":"ping"}).to_string().into()))
                        .await
                        .map_err(|_| Error::transport())?;
                    return Ok(json!({"type":"observer_heartbeat"}));
                }
                Ok(Some(Ok(Message::Text(t)))) => {
                    crate::compatibility::check_text(&t)?;
                    return serde_json::from_str(&t).map_err(|_| Error::protocol());
                }
                Ok(Some(Ok(Message::Ping(p)))) => self
                    .socket
                    .send(Message::Pong(p))
                    .await
                    .map_err(|_| Error::transport())?,
                Ok(Some(Ok(Message::Close(Some(f))))) if u16::from(f.code) == 4409 => {
                    return Err(crate::compatibility::upgrade_error(None));
                }
                Ok(Some(Ok(Message::Close(Some(f)))))
                    if matches!(
                        u16::from(f.code),
                        1008 | 4000 | 4005 | 4006 | 4401 | 4403 | 4409
                    ) =>
                {
                    return Err(Error::new(
                        "WS_ACCESS_DENIED",
                        "Composer socket authorization ended",
                    ));
                }
                Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_)))) => {
                    return Err(Error::transport());
                }
                _ => {}
            }
        }
    }
}
impl Client {
    pub async fn composer_send(
        &self,
        block: Uuid,
        body: Value,
        request_id: &str,
    ) -> Result<ComposerAccepted> {
        if body
            .get("input")
            .and_then(Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(Error::new("INVALID_INPUT", "Composer input is required"));
        }
        let request = self
            .http
            .post(self.url(&format!("agent/blocks/{block}/messages"))?)
            .query(&[("delivery", "ws")])
            .header("authorization", self.authorization.clone())
            .header("request-id", request_id)
            .json(&body);
        self.ensure_compatibility().await?;
        let response = self
            .contract_request(request)
            .send()
            .await
            .map_err(|_| Error::transport())?;
        let accepted: ComposerAccepted =
            serde_json::from_value(self.read_json_response(response).await?)
                .map_err(|_| Error::protocol())?;
        if !accepted.accepted || accepted.agent_block_id != block || accepted.run_id.is_empty() {
            return Err(Error::protocol());
        }
        Ok(accepted)
    }
    pub async fn composer_history(
        &self,
        block: Uuid,
        limit: u16,
        before: Option<&str>,
    ) -> Result<Vec<ComposerEvent>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(v) = before {
            query.push(("before", v.into()));
        }
        let request = self
            .http
            .get(self.url(&format!("agent/blocks/{block}/history"))?)
            .query(&query)
            .header("authorization", self.authorization.clone());
        self.ensure_compatibility().await?;
        let response = self
            .contract_request(request)
            .send()
            .await
            .map_err(|_| Error::transport())?;
        if !response.status().is_success() {
            return match self.read_json_response(response).await {
                Err(e) => Err(e),
                Ok(_) => Err(Error::protocol()),
            };
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"))
        {
            return Err(Error::protocol());
        }
        let mut stream = response.bytes_stream();
        let mut parser = Sse::default();
        let mut events = vec![];
        let mut total = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| Error::transport())?;
            total += chunk.len();
            if total > 32 * 1024 * 1024 {
                return Err(Error::new(
                    "INVALID_RESPONSE",
                    "History page exceeds 32 MiB",
                ));
            }
            events.extend(parser.feed(&chunk, false)?);
        }
        events.extend(parser.feed(&[], true)?);
        Ok(events)
    }
    pub async fn composer_interrupt(&self, block: Uuid) -> Result<Value> {
        self.request(
            Method::POST,
            &format!("agent/blocks/{block}/interrupt"),
            &[],
            None,
            None,
        )
        .await
    }
    pub async fn composer_socket(&self, canvas: Uuid) -> Result<ComposerSocket> {
        let mut url = self.url(&format!("canvas/ws/{canvas}"))?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).map_err(|_| Error::protocol())?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| Error::protocol())?;
        request
            .headers_mut()
            .insert("authorization", self.authorization.clone());
        self.contract_socket(request.headers_mut());
        self.ensure_compatibility().await?;
        let (socket, _) = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio_tungstenite::connect_async(request),
        )
        .await
        .map_err(|_| Error::transport())?
        .map_err(|error| match error {
            tokio_tungstenite::tungstenite::Error::Http(response)
                if crate::compatibility::is_upgrade_response(&response) =>
            {
                crate::compatibility::upgrade_error(None)
            }
            _ => Error::new("OBSERVATION_UNAVAILABLE", "Cannot connect canvas observer"),
        })?;
        Ok(ComposerSocket { socket })
    }
}

impl Client {
    pub async fn composer_list(
        &self,
        canvas: Uuid,
        org: Uuid,
        offset: u32,
        limit: u16,
    ) -> Result<Value> {
        self.request(
            Method::GET,
            &format!("v2/canvas/projects/{canvas}/elements"),
            &[
                ("org_id", org.to_string()),
                ("type", "agent".into()),
                ("offset", offset.to_string()),
                ("limit", limit.to_string()),
            ],
            None,
            None,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_utf8_crlf_multiline_and_tail() {
        let mut p = Sse::default();
        let raw = "id: 1\r\nevent: progress\r\ndata: {\"text\":\r\ndata: \"你好\"}\r\n\r\n:data\n\nevent: done\ndata: {}";
        let mut all = vec![];
        for b in raw.as_bytes() {
            all.extend(p.feed(&[*b], false).unwrap());
        }
        all.extend(p.feed(&[], true).unwrap());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].data["text"], "你好");
        assert_eq!(all[0].id.as_deref(), Some("1"));
        assert_eq!(all[1].event, "done");
    }
}
