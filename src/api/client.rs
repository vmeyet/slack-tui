use super::types::*;
use crate::auth::Credentials;
use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt;
use std::time::Duration;

pub const DEFAULT_API_URL: &str = "https://slack.com/api";
const MAX_RETRIES: usize = 3;

pub type Params = Vec<(String, String)>;

pub fn params(pairs: &[(&str, &str)]) -> Params {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

#[derive(Debug)]
pub struct ApiError {
    pub method: String,
    pub code: String,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed: {}", self.method, self.code)?;
        match self.code.as_str() {
            "invalid_auth" | "not_authed" | "token_revoked" | "token_expired" => {
                write!(f, " (session expired? run `slack login`)")
            }
            "channel_not_found" => write!(f, " (check the name, or are you a member?)"),
            "ratelimited" => write!(f, " (slow down a little)"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for ApiError {}

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

    pub async fn permalink(&self, channel: &str, ts: &str) -> Result<String> {
        let body = self.call("chat.getPermalink", params(&[("channel", channel), ("message_ts", ts)])).await?;
        body["permalink"].as_str().map(str::to_owned).context("chat.getPermalink: no permalink")
    }

    pub async fn react(&self, channel: &str, ts: &str, emoji: &str) -> Result<()> {
        let name = emoji.trim_matches(':');
        self.call("reactions.add", params(&[("channel", channel), ("timestamp", ts), ("name", name)])).await?;
        Ok(())
    }

    pub async fn search(&self, query: &str, count: usize) -> Result<SearchResult> {
        let p = params(&[("query", query), ("count", &count.to_string()), ("sort", "timestamp"), ("sort_dir", "desc")]);
        self.call_as("search.messages", p, "messages").await
    }
}

fn retry_after(response: &reqwest::Response) -> Duration {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client(server: &MockServer) -> Slack {
        Slack::new(&server.uri(), Credentials::new("xoxc-1", Some("xoxd-1"))).unwrap()
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
        let posted = client(&server).await.post_message("C1", "hi", None, None, false).await.unwrap();
        assert_eq!(posted.ts, "1.2");
    }

    #[tokio::test]
    async fn api_error_is_readable() {
        let server = MockServer::start().await;
        Mock::given(path("/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": false, "error": "channel_not_found"})))
            .mount(&server)
            .await;
        let err = client(&server).await.call("chat.postMessage", vec![]).await.unwrap_err();
        let api = err.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api.code, "channel_not_found");
        assert!(err.to_string().starts_with("chat.postMessage failed: channel_not_found"));
    }

    #[tokio::test]
    async fn invalid_auth_hints_login() {
        let server = MockServer::start().await;
        Mock::given(path("/auth.test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": false, "error": "invalid_auth"})))
            .mount(&server)
            .await;
        let err = client(&server).await.auth_test().await.unwrap_err();
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
        let me = client(&server).await.auth_test().await.unwrap();
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
        let channels = client(&server).await.channels().await.unwrap();
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
        let messages = client(&server).await.history("C1", 10, None).await.unwrap();
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
        client(&server).await.react("C1", "1.0", ":tada:").await.unwrap();
    }
}
