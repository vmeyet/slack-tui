//! Live events over Slack's RTM websocket, the same feed the web client uses.
use super::{Message, Slack};
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as Frame;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Connected,
    Message { channel: String, message: Message },
    Changed { channel: String, message: Message },
    Deleted { channel: String, ts: String },
    Reaction { channel: String, ts: String, name: String, added: bool },
    Disconnected(String),
}

impl Event {
    pub fn parse(raw: &Value) -> Option<Event> {
        let channel = || raw["channel"].as_str().map(str::to_owned);
        match raw["type"].as_str()? {
            "hello" => Some(Event::Connected),
            "message" => match raw["subtype"].as_str() {
                Some("message_changed") => {
                    let message = serde_json::from_value(raw["message"].clone()).ok()?;
                    Some(Event::Changed { channel: channel()?, message })
                }
                Some("message_deleted") => Some(Event::Deleted { channel: channel()?, ts: raw["deleted_ts"].as_str()?.to_owned() }),
                Some("message_replied") => None,
                _ => {
                    let message = serde_json::from_value(raw.clone()).ok()?;
                    Some(Event::Message { channel: channel()?, message })
                }
            },
            kind @ ("reaction_added" | "reaction_removed") => Some(Event::Reaction {
                channel: raw["item"]["channel"].as_str()?.to_owned(),
                ts: raw["item"]["ts"].as_str()?.to_owned(),
                name: raw["reaction"].as_str()?.to_owned(),
                added: kind == "reaction_added",
            }),
            _ => None,
        }
    }
}

const PING_EVERY: Duration = Duration::from_secs(30);
const RECONNECT_AFTER: Duration = Duration::from_secs(5);
const MAX_FAILURES: u32 = 3;

/// Keeps a connection alive and forwards its events until the receiver is dropped.
/// Gives up after a few consecutive failures so a workspace that refuses RTM can fall back to polling.
pub async fn stream(slack: Slack, tx: mpsc::UnboundedSender<Event>) {
    let mut failures = 0;
    loop {
        match session(&slack, &tx).await {
            Ok(()) => failures = 0,
            Err(e) => {
                failures += 1;
                let fatal = failures >= MAX_FAILURES;
                let text = if fatal { format!("{e} (giving up)") } else { e.to_string() };
                if tx.send(Event::Disconnected(text)).is_err() || fatal {
                    return;
                }
            }
        }
        if tx.is_closed() {
            return;
        }
        tokio::time::sleep(RECONNECT_AFTER).await;
    }
}

async fn session(slack: &Slack, tx: &mpsc::UnboundedSender<Event>) -> Result<()> {
    let url = slack.rtm_url().await?;
    let mut request = url.as_str().into_client_request().context("bad RTM url")?;
    if let Some(cookie) = slack.cookie_header() {
        request.headers_mut().insert("Cookie", cookie.parse()?);
    }
    let (socket, _) = tokio_tungstenite::connect_async(request).await.context("opening the RTM websocket")?;
    let (mut sink, mut source) = socket.split();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut ping_id = 0u64;
    loop {
        tokio::select! {
            _ = ping.tick() => {
                ping_id += 1;
                sink.send(Frame::Text(json!({"id": ping_id, "type": "ping"}).to_string().into())).await.context("ping")?;
            }
            frame = source.next() => {
                let Some(frame) = frame else { bail!("RTM connection closed") };
                let Frame::Text(text) = frame.context("RTM read")? else { continue };
                let Ok(raw) = serde_json::from_str::<Value>(&text) else { continue };
                if let Some(event) = Event::parse(&raw)
                    && tx.send(event).is_err()
                {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Credentials;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parses_message_events() {
        let raw = json!({"type": "message", "channel": "C1", "ts": "1.0", "user": "U1", "text": "hi"});
        let Some(Event::Message { channel, message }) = Event::parse(&raw) else { panic!() };
        assert_eq!(channel, "C1");
        assert_eq!(message.text, "hi");
        let changed = json!({"type": "message", "subtype": "message_changed", "channel": "C1", "message": {"ts": "1.0", "text": "edited"}});
        let Some(Event::Changed { message, .. }) = Event::parse(&changed) else { panic!() };
        assert_eq!(message.text, "edited");
        let deleted = json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "deleted_ts": "1.0"});
        assert_eq!(Event::parse(&deleted), Some(Event::Deleted { channel: "C1".into(), ts: "1.0".into() }));
        assert_eq!(Event::parse(&json!({"type": "message", "subtype": "message_replied", "channel": "C1"})), None);
    }

    #[test]
    fn parses_reactions_and_ignores_noise() {
        let raw = json!({"type": "reaction_added", "reaction": "tada", "item": {"channel": "C1", "ts": "1.0"}});
        assert_eq!(Event::parse(&raw), Some(Event::Reaction { channel: "C1".into(), ts: "1.0".into(), name: "tada".into(), added: true }));
        assert_eq!(Event::parse(&json!({"type": "hello"})), Some(Event::Connected));
        assert_eq!(Event::parse(&json!({"type": "user_typing"})), None);
        assert_eq!(Event::parse(&json!({"reply_to": 1, "ok": true})), None);
    }

    #[tokio::test]
    async fn streams_events_from_a_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Frame::Text(json!({"type": "hello"}).to_string().into())).await.unwrap();
            ws.send(Frame::Text(json!({"type": "message", "channel": "C1", "ts": "1.0", "text": "live"}).to_string().into()))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let server = MockServer::start().await;
        Mock::given(path("/rtm.connect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "url": ws_url})))
            .mount(&server)
            .await;
        let slack = Slack::new(&server.uri(), Credentials::new("xoxc", Some("xoxd"))).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(stream(slack, tx));
        assert_eq!(rx.recv().await, Some(Event::Connected));
        let Some(Event::Message { message, .. }) = rx.recv().await else { panic!() };
        assert_eq!(message.text, "live");
    }

    #[tokio::test]
    async fn gives_up_after_repeated_failures() {
        let server = MockServer::start().await;
        Mock::given(path("/rtm.connect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": false, "error": "not_allowed_token_type"})))
            .mount(&server)
            .await;
        let slack = Slack::new(&server.uri(), Credentials::new("xoxc", None)).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::time::pause();
        tokio::spawn(stream(slack, tx));
        let mut seen = 0;
        while let Some(event) = rx.recv().await {
            assert!(matches!(event, Event::Disconnected(_)));
            seen += 1;
        }
        assert_eq!(seen, MAX_FAILURES);
    }
}
