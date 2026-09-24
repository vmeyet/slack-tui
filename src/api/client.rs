use super::types::{Channel, Counts, Group, Identity, Message, Posted, SearchResult, Section, ThreadView, User};
use crate::auth::Credentials;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::StatusCode;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_API_URL: &str = "https://slack.com/api";
const MAX_RETRIES: usize = 3;

pub type Params = Vec<(String, String)>;

pub fn params(pairs: &[(&str, &str)]) -> Params {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

/// With `oldest` alone Slack pages from the oldest message forward; a `latest` far in the future
/// makes it page from the newest one back, without trusting the local clock.
fn window(oldest: &str) -> Params {
    params(&[("oldest", oldest), ("latest", "9999999999")])
}

#[derive(Debug, thiserror::Error)]
#[error("{method} failed: {code}{}", hint(code))]
pub struct ApiError {
    pub method: String,
    pub code: String,
}

fn hint(code: &str) -> &'static str {
    match code {
        "invalid_auth" | "not_authed" | "token_revoked" | "token_expired" => " (session expired? run `slack login`)",
        "channel_not_found" => " (check the name, or are you a member?)",
        "ratelimited" => " (slow down a little)",
        _ => "",
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    ok: bool,
    error: Option<String>,
}

/// Slack omits `response_metadata`, or leaves `next_cursor` empty, on the last page.
#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    response_metadata: ResponseMetadata,
}

#[derive(Default, Deserialize)]
struct ResponseMetadata {
    #[serde(default)]
    next_cursor: String,
}

#[derive(Deserialize)]
struct RtmConnect {
    url: String,
}

/// The whole body when `key` is empty, else the value under `key`, which must be there.
fn read<T: DeserializeOwned>(method: &str, mut body: Value, key: &str) -> Result<T> {
    let node = if key.is_empty() {
        body
    } else {
        body.get_mut(key).map(Value::take).with_context(|| format!("{method}: no `{key}` in the response"))?
    };
    serde_json::from_value(node).with_context(|| format!("{method}: unexpected `{key}` shape"))
}

#[derive(Clone)]
pub struct Slack {
    http: reqwest::Client,
    base: String,
    credentials: Credentials,
}

impl Slack {
    pub fn new(base: &str, credentials: Credentials) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("{}/{}", crate::app::NAME, env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { http, base: base.trim_end_matches('/').to_owned(), credentials })
    }

    pub fn api_url_from_env() -> String {
        crate::app::env("API_URL").and_then(|url| url.into_string().ok()).unwrap_or_else(|| DEFAULT_API_URL.to_owned())
    }

    pub async fn call(&self, method: &str, params: Params) -> Result<Value> {
        let mut attempt = 0;
        loop {
            let response = self.send(method, &params).await?;
            if response.status() == StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES {
                attempt += 1;
                tokio::time::sleep(retry_after(&response)).await;
                continue;
            }
            let status = response.status();
            let body: Value = response.json().await.with_context(|| format!("{method}: response is not JSON (HTTP {status})"))?;
            let envelope = Envelope::deserialize(&body).with_context(|| format!("{method}: unexpected response shape"))?;
            if envelope.ok {
                return Ok(body);
            }
            let code = envelope.error.unwrap_or_else(|| "unknown_error".to_owned());
            return Err(ApiError { method: method.to_owned(), code }.into());
        }
    }

    pub async fn call_as<T: DeserializeOwned>(&self, method: &str, params: Params, key: &str) -> Result<T> {
        let body = self.call(method, params).await?;
        read(method, body, key)
    }

    pub async fn call_pages<T: DeserializeOwned>(&self, method: &str, params: Params, key: &str) -> Result<Vec<T>> {
        let mut items = Vec::new();
        let mut cursor = String::new();
        loop {
            let mut page = params.clone();
            if !cursor.is_empty() {
                page.push(("cursor".into(), cursor));
            }
            let body = self.call(method, page).await?;
            cursor = Page::deserialize(&body)
                .with_context(|| format!("{method}: unexpected `response_metadata` shape"))?
                .response_metadata
                .next_cursor;
            items.extend(read::<Vec<T>>(method, body, key)?);
            if cursor.is_empty() {
                return Ok(items);
            }
        }
    }

    async fn send(&self, method: &str, params: &Params) -> Result<reqwest::Response> {
        let mut request = self.http.post(format!("{}/{method}", self.base)).bearer_auth(&self.credentials.token).form(params);
        if let Some(cookie) = self.credentials.cookie_header() {
            request = request.header("Cookie", cookie);
        }
        request.send().await.map_err(|e| anyhow!("{method}: {}", e.without_url()))
    }

    pub fn cookie_header(&self) -> Option<String> {
        self.credentials.cookie_header()
    }

    pub async fn rtm_url(&self) -> Result<String> {
        Ok(self.call_as::<RtmConnect>("rtm.connect", vec![], "").await?.url)
    }

    pub async fn auth_test(&self) -> Result<Identity> {
        self.call_as("auth.test", vec![], "").await
    }

    pub async fn channels(&self) -> Result<Vec<Channel>> {
        let p = params(&[("types", "public_channel,private_channel,mpim,im"), ("exclude_archived", "true"), ("limit", "1000")]);
        self.call_pages("conversations.list", p, "channels").await
    }

    pub async fn users(&self) -> Result<Vec<User>> {
        self.call_pages("users.list", params(&[("limit", "1000")]), "members").await
    }

    pub async fn groups(&self) -> Result<Vec<Group>> {
        self.call_as("usergroups.list", params(&[("include_users", "true")]), "usergroups").await
    }

    pub async fn user_info(&self, id: &str) -> Result<User> {
        self.call_as("users.info", params(&[("user", id)]), "user").await
    }

    pub async fn open_dm(&self, user_id: &str) -> Result<String> {
        let body = self.call("conversations.open", params(&[("users", user_id)])).await?;
        body["channel"]["id"].as_str().map(str::to_owned).context("conversations.open: no channel id")
    }

    /// The newest `limit` messages after `oldest`, oldest first.
    pub async fn history(&self, channel: &str, limit: usize, oldest: Option<&str>) -> Result<Vec<Message>> {
        let mut p = params(&[("channel", channel), ("limit", &limit.to_string())]);
        if let Some(o) = oldest {
            p.extend(window(o));
        }
        let mut messages: Vec<Message> = self.call_as("conversations.history", p, "messages").await?;
        messages.reverse();
        Ok(messages)
    }

    /// Every message after `oldest`, oldest first.
    pub async fn history_since(&self, channel: &str, oldest: &str) -> Result<Vec<Message>> {
        let mut p = params(&[("channel", channel), ("limit", "200")]);
        p.extend(window(oldest));
        let mut messages: Vec<Message> = self.call_pages("conversations.history", p, "messages").await?;
        messages.reverse();
        Ok(messages)
    }

    pub async fn replies(&self, channel: &str, thread_ts: &str) -> Result<Vec<Message>> {
        let p = params(&[("channel", channel), ("ts", thread_ts), ("limit", "1000")]);
        self.call_pages("conversations.replies", p, "messages").await
    }

    pub async fn post_message(
        &self,
        channel: &str,
        text: &str,
        blocks: Option<&Value>,
        thread_ts: Option<&str>,
        broadcast: bool,
    ) -> Result<Posted> {
        let mut p = params(&[("channel", channel), ("text", text)]);
        if let Some(b) = blocks {
            p.push(("blocks".into(), b.to_string()));
        }
        if let Some(t) = thread_ts {
            p.push(("thread_ts".into(), t.to_owned()));
            if broadcast {
                p.push(("reply_broadcast".into(), "true".into()));
            }
        }
        let body = self.call("chat.postMessage", p).await?;
        let channel = body["channel"].as_str().unwrap_or(channel).to_owned();
        let ts = body["ts"].as_str().context("chat.postMessage: no ts")?.to_owned();
        Ok(Posted { channel, ts, permalink: String::new() })
    }

    /// Fetches a file Slack hosts, with the session's credentials. Only Slack's own hosts are
    /// allowed so a crafted message can never point the client elsewhere, and the body is capped.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        const MAX_BYTES: u64 = 10 * 1024 * 1024;
        let parsed = reqwest::Url::parse(url).context("file url")?;
        let host = parsed.host_str().unwrap_or_default();
        let own = reqwest::Url::parse(&self.base).ok().and_then(|b| b.host_str().map(str::to_owned)).unwrap_or_default();
        let trusted = host == own || host.ends_with(".slack.com") || host.ends_with(".slack-edge.com");
        if parsed.scheme() != "https" && host != own || !trusted {
            bail!("refusing to download from {host}");
        }
        let mut request = self.http.get(parsed).bearer_auth(&self.credentials.token);
        if let Some(cookie) = self.credentials.cookie_header() {
            request = request.header("Cookie", cookie);
        }
        let response = request.send().await?.error_for_status()?;
        if response.content_length().is_some_and(|n| n > MAX_BYTES) {
            bail!("file too large");
        }
        let bytes = response.bytes().await?;
        if bytes.len() as u64 > MAX_BYTES {
            bail!("file too large");
        }
        Ok(bytes.to_vec())
    }

    pub async fn permalink(&self, channel: &str, ts: &str) -> Result<String> {
        let body = self.call("chat.getPermalink", params(&[("channel", channel), ("message_ts", ts)])).await?;
        body["permalink"].as_str().map(str::to_owned).context("chat.getPermalink: no permalink")
    }

    pub async fn react(&self, channel: &str, ts: &str, emoji: &str) -> Result<()> {
        let name = emoji.trim_matches(':');
        self.call("reactions.add", params(&[("channel", channel), ("timestamp", ts), ("name", name)])).await?;
        Ok(())
    }

    /// Replaces the text of one's own message; the blocks Slack kept give way to this text.
    pub async fn update_message(&self, channel: &str, ts: &str, text: &str) -> Result<()> {
        self.call("chat.update", params(&[("channel", channel), ("ts", ts), ("text", text)])).await?;
        Ok(())
    }

    pub async fn delete_message(&self, channel: &str, ts: &str) -> Result<()> {
        self.call("chat.delete", params(&[("channel", channel), ("ts", ts)])).await?;
        Ok(())
    }

    /// Read state of every conversation, the web client's own endpoint.
    pub async fn counts(&self) -> Result<Counts> {
        self.call_as("client.counts", vec![], "").await
    }

    /// Threads the user follows, newest first, with their read state.
    pub async fn thread_view(&self, limit: usize) -> Result<Vec<ThreadView>> {
        self.call_as("subscriptions.thread.getView", params(&[("limit", &limit.to_string())]), "threads").await
    }

    /// Sidebar sections as the web client shows them: stars first, then custom ones. Best effort.
    pub async fn sections(&self) -> Result<Vec<Section>> {
        self.call_as("users.channelSections.list", vec![], "channel_sections").await
    }

    /// Channel ids the user muted, from the web client's preference blob. Best effort.
    pub async fn muted(&self) -> Result<Vec<String>> {
        let body = self.call("users.prefs.get", vec![]).await?;
        let raw = body["prefs"]["muted_channels"].as_str().unwrap_or_default();
        Ok(raw.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect())
    }

    pub async fn join(&self, channel: &str) -> Result<()> {
        self.call("conversations.join", params(&[("channel", channel)])).await?;
        Ok(())
    }

    pub async fn leave(&self, channel: &str) -> Result<()> {
        self.call("conversations.leave", params(&[("channel", channel)])).await?;
        Ok(())
    }

    pub async fn mark_read(&self, channel: &str, ts: &str) -> Result<()> {
        self.call("conversations.mark", params(&[("channel", channel), ("ts", ts)])).await?;
        Ok(())
    }

    pub async fn mark_thread_read(&self, channel: &str, thread_ts: &str, ts: &str) -> Result<()> {
        self.call("subscriptions.thread.mark", params(&[("channel", channel), ("thread_ts", thread_ts), ("ts", ts)])).await?;
        Ok(())
    }

    pub async fn search(&self, query: &str, count: usize) -> Result<SearchResult> {
        self.search_page(query, count, 1).await
    }

    /// Newest first; `page` starts at 1.
    pub async fn search_page(&self, query: &str, count: usize, page: usize) -> Result<SearchResult> {
        let (count, page) = (count.to_string(), page.to_string());
        let p = params(&[("query", query), ("count", &count), ("page", &page), ("sort", "timestamp"), ("sort_dir", "desc")]);
        self.call_as("search.messages", p, "messages").await
    }
}

fn retry_after(response: &reqwest::Response) -> Duration {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .map_or(Duration::from_secs(1), Duration::from_secs)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> Slack {
        Slack::new(&server.uri(), Credentials::new("xoxc-1", Some("xoxd-1"))).unwrap()
    }

    #[tokio::test]
    async fn downloads_only_from_slack_hosts_with_credentials_and_a_size_cap() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/files/a.png"))
            .and(header("Authorization", "Bearer xoxc-1"))
            .and(header("Cookie", "d=xoxd-1"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PNG".to_vec()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/files/huge.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 11 * 1024 * 1024]))
            .mount(&server)
            .await;
        let slack = client(&server);
        assert_eq!(slack.download(&format!("{}/files/a.png", server.uri())).await.unwrap(), b"PNG");
        let err = slack.download("https://evil.example.com/a.png").await.unwrap_err().to_string();
        assert!(err.contains("refusing"), "{err}");
        let err = slack.download("http://files.slack.com/a.png").await.unwrap_err().to_string();
        assert!(err.contains("refusing"), "{err}");
        let err = slack.download(&format!("{}/files/huge.png", server.uri())).await.unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
    }

    #[tokio::test]
    async fn sends_bearer_cookie_and_form_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .and(header("Authorization", "Bearer xoxc-1"))
            .and(header("Cookie", "d=xoxd-1"))
            .and(body_string_contains("channel=C1"))
            .and(body_string_contains("text=hi"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true, "channel": "C1", "ts": "1.2"})))
            .expect(1)
            .mount(&server)
            .await;
        let posted = client(&server).post_message("C1", "hi", None, None, false).await.unwrap();
        assert_eq!(posted.ts, "1.2");
    }

    #[tokio::test]
    async fn edit_and_delete_name_the_one_message_they_touch() {
        let server = MockServer::start().await;
        let ok = || ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}));
        Mock::given(path("/chat.update"))
            .and(body_string_contains("channel=C1"))
            .and(body_string_contains("ts=1700000000.000100"))
            .and(body_string_contains("text=fixed"))
            .respond_with(ok())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/chat.delete"))
            .and(body_string_contains("channel=C1"))
            .and(body_string_contains("ts=1700000000.000100"))
            .respond_with(ok())
            .expect(1)
            .mount(&server)
            .await;
        let slack = client(&server);
        slack.update_message("C1", "1700000000.000100", "fixed").await.unwrap();
        slack.delete_message("C1", "1700000000.000100").await.unwrap();
    }

    #[tokio::test]
    async fn api_error_is_readable() {
        let server = MockServer::start().await;
        Mock::given(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": false, "error": "channel_not_found"})))
            .mount(&server)
            .await;
        let err = client(&server).call("chat.postMessage", vec![]).await.unwrap_err();
        let api = err.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api.code, "channel_not_found");
        assert!(err.to_string().starts_with("chat.postMessage failed: channel_not_found"));
    }

    #[test]
    fn api_error_message_carries_the_hint_for_its_code() {
        let message = |code: &str| ApiError { method: "m".into(), code: code.into() }.to_string();
        assert_eq!(message("token_expired"), "m failed: token_expired (session expired? run `slack login`)");
        assert_eq!(message("channel_not_found"), "m failed: channel_not_found (check the name, or are you a member?)");
        assert_eq!(message("ratelimited"), "m failed: ratelimited (slow down a little)");
        assert_eq!(message("other"), "m failed: other");
    }

    #[tokio::test]
    async fn invalid_auth_hints_login() {
        let server = MockServer::start().await;
        Mock::given(path("/auth.test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": false, "error": "invalid_auth"})))
            .mount(&server)
            .await;
        let err = client(&server).auth_test().await.unwrap_err();
        assert!(err.to_string().contains("slack login"));
    }

    #[tokio::test]
    async fn retries_on_429() {
        let server = MockServer::start().await;
        Mock::given(path("/auth.test"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(path("/auth.test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": true, "team_id": "T1", "team": "Acme", "user_id": "U1", "user": "vivien"})),
            )
            .mount(&server)
            .await;
        let me = client(&server).auth_test().await.unwrap();
        assert_eq!(me.user, "vivien");
    }

    #[tokio::test]
    async fn follows_cursor_pages() {
        let server = MockServer::start().await;
        Mock::given(path("/conversations.list"))
            .and(body_string_contains("cursor=next"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true, "channels": [{"id": "C2", "name": "two"}]})),
            )
            .mount(&server)
            .await;
        Mock::given(path("/conversations.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"ok": true, "channels": [{"id": "C1", "name": "one"}], "response_metadata": {"next_cursor": "next"}}),
            ))
            .mount(&server)
            .await;
        let channels = client(&server).channels().await.unwrap();
        assert_eq!(channels.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["C1", "C2"]);
    }

    async fn answer(server: &MockServer, api_method: &str, body: Value) {
        Mock::given(path(format!("/{api_method}"))).respond_with(ResponseTemplate::new(200).set_body_json(body)).mount(server).await;
    }

    #[tokio::test]
    async fn replies_follow_the_cursor_until_it_is_empty() {
        let server = MockServer::start().await;
        Mock::given(path("/conversations.replies"))
            .and(body_string_contains("cursor=next"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"ok": true, "messages": [{"ts": "2.0"}], "has_more": false, "response_metadata": {"next_cursor": ""}}),
            ))
            .mount(&server)
            .await;
        answer(
            &server,
            "conversations.replies",
            serde_json::json!({"ok": true, "messages": [{"ts": "1.0"}], "has_more": true, "response_metadata": {"next_cursor": "next"}}),
        )
        .await;
        let messages = client(&server).replies("C1", "1.0").await.unwrap();
        assert_eq!(messages.iter().map(|m| m.ts.as_str()).collect::<Vec<_>>(), ["1.0", "2.0"]);
    }

    #[tokio::test]
    async fn a_page_without_its_items_key_is_an_error() {
        let server = MockServer::start().await;
        answer(&server, "conversations.replies", serde_json::json!({"ok": true, "replies": []})).await;
        let err = client(&server).replies("C1", "1.0").await.unwrap_err();
        assert_eq!(err.to_string(), "conversations.replies: no `messages` in the response");
    }

    #[tokio::test]
    async fn a_cursor_of_the_wrong_type_is_an_error_not_a_last_page() {
        let server = MockServer::start().await;
        answer(&server, "users.list", serde_json::json!({"ok": true, "members": [], "response_metadata": {"next_cursor": 7}})).await;
        let err = client(&server).users().await.unwrap_err();
        assert!(err.to_string().contains("response_metadata"), "{err}");
    }

    #[tokio::test]
    async fn history_without_messages_is_an_error() {
        let server = MockServer::start().await;
        answer(&server, "conversations.history", serde_json::json!({"ok": true, "items": []})).await;
        let err = client(&server).history("C1", 10, None).await.unwrap_err();
        assert_eq!(err.to_string(), "conversations.history: no `messages` in the response");
    }

    #[tokio::test]
    async fn rtm_connect_without_url_is_an_error() {
        let server = MockServer::start().await;
        answer(&server, "rtm.connect", serde_json::json!({"ok": true, "uri": "wss://x"})).await;
        let err = client(&server).rtm_url().await.unwrap_err();
        assert!(format!("{err:#}").contains("missing field `url`"), "{err:#}");
    }

    #[tokio::test]
    async fn history_is_chronological() {
        let server = MockServer::start().await;
        Mock::given(path("/conversations.history"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": true, "messages": [{"ts": "2.0", "text": "b"}, {"ts": "1.0", "text": "a"}]})),
            )
            .mount(&server)
            .await;
        let messages = client(&server).history("C1", 10, None).await.unwrap();
        assert_eq!(messages.iter().map(|m| m.ts.as_str()).collect::<Vec<_>>(), ["1.0", "2.0"]);
    }

    /// Three pages of history after `oldest`, newest first like Slack sends them.
    async fn three_pages(server: &MockServer) {
        let page = |ts: [&str; 2], next: &str| {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [{"ts": ts[0]}, {"ts": ts[1]}],
                "response_metadata": {"next_cursor": next},
            }))
        };
        Mock::given(path("/conversations.history"))
            .and(body_string_contains("cursor=b"))
            .respond_with(page(["2.0", "1.0"], ""))
            .mount(server)
            .await;
        Mock::given(path("/conversations.history"))
            .and(body_string_contains("cursor=a"))
            .respond_with(page(["4.0", "3.0"], "b"))
            .mount(server)
            .await;
        Mock::given(path("/conversations.history"))
            .and(body_string_contains("oldest=0.5"))
            .and(body_string_contains("latest=9999999999"))
            .respond_with(page(["6.0", "5.0"], "a"))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn history_with_oldest_keeps_the_newest() {
        let server = MockServer::start().await;
        three_pages(&server).await;
        let messages = client(&server).history("C1", 2, Some("0.5")).await.unwrap();
        assert_eq!(messages.iter().map(|m| m.ts.as_str()).collect::<Vec<_>>(), ["5.0", "6.0"]);
    }

    #[tokio::test]
    async fn history_since_reads_the_whole_window() {
        let server = MockServer::start().await;
        three_pages(&server).await;
        let messages = client(&server).history_since("C1", "0.5").await.unwrap();
        assert_eq!(messages.iter().map(|m| m.ts.as_str()).collect::<Vec<_>>(), ["1.0", "2.0", "3.0", "4.0", "5.0", "6.0"]);
    }

    #[tokio::test]
    async fn react_strips_colons() {
        let server = MockServer::start().await;
        Mock::given(path("/reactions.add"))
            .and(body_string_contains("name=tada"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;
        client(&server).react("C1", "1.0", ":tada:").await.unwrap();
    }
}
