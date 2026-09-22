//! The few Chrome devtools protocol (CDP) calls the login needs, over one websocket.
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

#[derive(Debug, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
}

#[derive(Deserialize)]
struct Cookies {
    cookies: Vec<Cookie>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Targets {
    target_infos: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    #[serde(rename = "targetId")]
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attached {
    session_id: String,
}

pub struct Cdp {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    seen_urls: Vec<String>,
}

impl Cdp {
    pub async fn connect(url: &str) -> Result<Self> {
        let (socket, _) = tokio_tungstenite::connect_async(url).await.with_context(|| format!("connecting to the browser at {url}"))?;
        let mut cdp = Self { socket, next_id: 1, seen_urls: Vec::new() };
        cdp.call("Target.setDiscoverTargets", json!({"discover": true})).await?;
        Ok(cdp)
    }

    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.call_in(None, method, params).await
    }

    async fn call_as<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> Result<T> {
        let result = self.call(method, params).await?;
        serde_json::from_value(result).with_context(|| format!("{method}: unexpected result shape"))
    }

    pub async fn call_in(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.socket.send(Message::Text(msg.to_string().into())).await?;
        loop {
            let Some(frame) = self.socket.next().await else { bail!("the browser closed the connection") };
            let Message::Text(text) = frame? else { continue };
            let msg: Value = serde_json::from_str(&text)?;
            if msg["id"] != id {
                self.remember_url(&msg);
                continue;
            }
            if let Some(err) = msg.get("error") {
                bail!("{method}: {}", err["message"].as_str().unwrap_or("unknown CDP error"));
            }
            return Ok(msg["result"].clone());
        }
    }

    /// Pages redirect faster than we poll, so every URL a tab passes through is kept.
    fn remember_url(&mut self, event: &Value) {
        if let Some(url) = event["params"]["targetInfo"]["url"].as_str()
            && !url.is_empty()
            && !self.seen_urls.iter().any(|u| u == url)
        {
            self.seen_urls.push(url.to_owned());
        }
    }

    pub async fn cookies(&mut self) -> Result<Vec<Cookie>> {
        Ok(self.call_as::<Cookies>("Storage.getCookies", json!({})).await?.cookies)
    }

    /// Open pages as `(target id, url)`, followed by every URL seen before, newest first.
    pub async fn pages(&mut self) -> Result<Vec<(String, String)>> {
        let targets = self.call_as::<Targets>("Target.getTargets", json!({})).await?.target_infos;
        let current = targets.into_iter().filter(|t| t.kind == "page").map(|t| (t.id, t.url));
        let seen = self.seen_urls.iter().rev().map(|u| (String::new(), u.clone()));
        Ok(current.chain(seen).collect())
    }

    /// Runs a JS expression in a page and returns its value.
    pub async fn evaluate(&mut self, target_id: &str, expression: &str) -> Result<Value> {
        let session = self.call_as::<Attached>("Target.attachToTarget", json!({"targetId": target_id, "flatten": true})).await?.session_id;
        let result = self.call_in(Some(&session), "Runtime.evaluate", json!({"expression": expression, "returnByValue": true})).await;
        let _ = self.call("Target.detachFromTarget", json!({"sessionId": session})).await;
        Ok(result?["result"]["value"].clone())
    }

    pub async fn close_browser(&mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(3), self.call("Browser.close", json!({}))).await;
    }
}

/// Chrome writes `<port>\n<browser ws path>` here once its debug port is ready.
pub fn parse_active_port(contents: &str) -> Option<String> {
    let mut lines = contents.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    Some(format!("ws://127.0.0.1:{port}{path}"))
}

pub async fn wait_for_active_port(profile: &Path, timeout: Duration) -> Result<String> {
    let file = profile.join("DevToolsActivePort");
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(url) = std::fs::read_to_string(&file).ok().and_then(|c| parse_active_port(&c)) {
            return Ok(url);
        }
        if tokio::time::Instant::now() > deadline {
            bail!("the browser did not expose its DevTools port in time");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    async fn fake_browser(answering: Value) -> Cdp {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = ws.next().await {
                let call: Value = serde_json::from_str(&text).unwrap();
                let result = answering.get(call["method"].as_str().unwrap()).cloned().unwrap_or_else(|| json!({}));
                ws.send(Message::Text(json!({"id": call["id"], "result": result}).to_string().into())).await.unwrap();
            }
        });
        Cdp::connect(&url).await.unwrap()
    }

    #[tokio::test]
    async fn reads_cookies_and_pages_from_the_browser() {
        let mut cdp = fake_browser(json!({
            "Storage.getCookies": {"cookies": [{"name": "d", "value": "xoxd-1", "domain": ".slack.com", "path": "/"}]},
            "Target.getTargets": {"targetInfos": [
                {"targetId": "T1", "type": "page", "url": "https://app.slack.com/client", "title": "Slack"},
                {"targetId": "T2", "type": "service_worker", "url": "https://app.slack.com/sw.js", "title": ""}
            ]}
        }))
        .await;
        let cookies = cdp.cookies().await.unwrap();
        assert_eq!((cookies[0].name.as_str(), cookies[0].value.as_str()), ("d", "xoxd-1"));
        assert_eq!(cdp.pages().await.unwrap(), [("T1".to_owned(), "https://app.slack.com/client".to_owned())]);
    }

    #[tokio::test]
    async fn a_renamed_result_field_is_an_error_not_an_empty_list() {
        let mut cdp = fake_browser(json!({"Storage.getCookies": {"items": []}, "Target.getTargets": {"targets": []}})).await;
        assert!(cdp.cookies().await.unwrap_err().to_string().contains("Storage.getCookies: unexpected result shape"));
        assert!(cdp.pages().await.unwrap_err().to_string().contains("Target.getTargets: unexpected result shape"));
    }

    #[test]
    fn parses_active_port_file() {
        assert_eq!(parse_active_port("54321\n/devtools/browser/abc\n").as_deref(), Some("ws://127.0.0.1:54321/devtools/browser/abc"));
        assert_eq!(parse_active_port("nope\n/x"), None);
        assert_eq!(parse_active_port("1"), None);
    }
}
