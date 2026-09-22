use super::types::{Channel, Counts, Group, Identity, Message, Posted, SearchResult, Section, ThreadView, User};
use crate::auth::Credentials;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_API_URL: &str = "https://slack.com/api";
const MAX_RETRIES: usize = 3;

pub type Params = Vec<(String, String)>;

pub fn params(pairs: &[(&str, &str)]) -> Params {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
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

#[derive(Clone)]
pub struct Slack {
    http: reqwest::Client,
    base: String,
    credentials: Credentials,
}

impl Slack {
    pub fn new(base: &str, credentials: Credentials) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("slack-cli/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { http, base: base.trim_end_matches('/').to_owned(), credentials })
    }

    pub fn api_url_from_env() -> String {
        std::env::var("SLACK_CLI_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned())
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
            if body["ok"].as_bool() == Some(true) {
                return Ok(body);
            }
            let code = body["error"].as_str().unwrap_or("unknown_error").to_owned();
            return Err(ApiError { method: method.to_owned(), code }.into());
        }
    }

    pub async fn call_as<T: DeserializeOwned>(&self, method: &str, params: Params, key: &str) -> Result<T> {
        let body = self.call(method, params).await?;
        let node = if key.is_empty() { body } else { body[key].clone() };
        serde_json::from_value(node).with_context(|| format!("{method}: unexpected `{key}` shape"))
    }

    pub async fn call_pages<T: DeserializeOwned>(&self, method: &str, params: Params, key: &str) -> Result<Vec<T>> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut page = params.clone();
            if let Some(c) = &cursor {
                page.push(("cursor".into(), c.clone()));
            }
            let body = self.call(method, page).await?;
            let chunk: Vec<T> = serde_json::from_value(body[key].clone()).with_context(|| format!("{method}: unexpected `{key}` shape"))?;
            items.extend(chunk);
            cursor = body["response_metadata"]["next_cursor"].as_str().filter(|c| !c.is_empty()).map(str::to_owned);
            if cursor.is_none() {
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
        let body = self.call("rtm.connect", vec![]).await?;
        body["url"].as_str().map(str::to_owned).context("rtm.connect: no url")
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

    pub async fn history(&self, channel: &str, limit: usize, oldest: Option<&str>) -> Result<Vec<Message>> {
        let mut p = params(&[("channel", channel), ("limit", &limit.to_string())]);
        if let Some(o) = oldest {
            p.push(("oldest".into(), o.to_owned()));
        }
        let mut messages: Vec<Message> = self.call_as("conversations.history", p, "messages").await?;
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
