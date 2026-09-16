use crate::api::{Channel, ChannelKind, Message, Slack, User};
use crate::cache::Cache;
use crate::{markdown, mrkdwn};
use anyhow::{Result, bail};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static CHANNEL_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[CDG][A-Z0-9]{8,}$").unwrap());
static USER_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[UW][A-Z0-9]{8,}$").unwrap());

/// Channels and users of one workspace, cached on disk and refreshed on a miss.
pub struct Directory {
    slack: Slack,
    cache: Cache,
    channels: Vec<Channel>,
    users: Vec<User>,
    channels_fresh: bool,
    users_fresh: bool,
}

impl Directory {
    pub fn new(slack: Slack, cache: Cache) -> Self {
        Self {
            channels: cache.load("channels").unwrap_or_default(),
            users: cache.load("users").unwrap_or_default(),
            slack,
            cache,
            channels_fresh: false,
            users_fresh: false,
        }
    }

    pub async fn refresh_channels(&mut self) -> Result<()> {
        self.channels = self.slack.channels().await?;
        self.channels_fresh = true;
        self.cache.save("channels", &self.channels)
    }

    pub async fn refresh_users(&mut self) -> Result<()> {
        self.users = self.slack.users().await?.into_iter().filter(|u| !u.deleted).collect();
        self.users_fresh = true;
        self.cache.save("users", &self.users)
    }

    pub async fn channels(&mut self) -> Result<&[Channel]> {
        if self.channels.is_empty() {
            self.refresh_channels().await?;
        }
        Ok(&self.channels)
    }

    pub async fn users(&mut self) -> Result<&[User]> {
        if self.users.is_empty() {
            self.refresh_users().await?;
        }
        Ok(&self.users)
    }

    /// `#name`, `name`, `@handle`, or a raw id, to a conversation id.
    pub async fn channel_id(&mut self, target: &str) -> Result<String> {
        let target = target.trim();
        if CHANNEL_ID.is_match(target) {
            return Ok(target.to_owned());
        }
        if let Some(handle) = target.strip_prefix('@') {
            let user = self.user_id(handle).await?;
            return self.slack.open_dm(&user).await;
        }
        let name = target.trim_start_matches('#');
        if let Some(id) = self.find_channel(name) {
            return Ok(id);
        }
        if !self.channels_fresh {
            self.refresh_channels().await?;
            if let Some(id) = self.find_channel(name) {
                return Ok(id);
            }
        }
        let close: Vec<String> = self.channels.iter().filter(|c| c.name.contains(name)).map(|c| format!("#{}", c.name)).take(5).collect();
        match close.is_empty() {
            true => bail!("channel `{target}` not found (are you a member?)"),
            false => bail!("channel `{target}` not found. Did you mean: {}", close.join(", ")),
        }
    }

    fn find_channel(&self, name: &str) -> Option<String> {
        self.channels.iter().find(|c| c.name == name).map(|c| c.id.clone())
    }

    pub async fn user_id(&mut self, handle: &str) -> Result<String> {
        let handle = handle.trim().trim_start_matches('@');
        if USER_ID.is_match(handle) {
            return Ok(handle.to_owned());
        }
        self.users().await?;
        if let Some(id) = self.find_user(handle) {
            return Ok(id);
        }
        if !self.users_fresh {
            self.refresh_users().await?;
            if let Some(id) = self.find_user(handle) {
                return Ok(id);
            }
        }
        bail!("user `@{handle}` not found")
    }

    fn find_user(&self, handle: &str) -> Option<String> {
        let lower = handle.to_lowercase();
        let exact = |u: &&User| u.handle().eq_ignore_ascii_case(handle) || u.name.eq_ignore_ascii_case(handle);
        let loose = |u: &&User| u.real_name.to_lowercase() == lower || u.profile.real_name.to_lowercase() == lower;
        self.users.iter().find(exact).or_else(|| self.users.iter().find(loose)).map(|u| u.id.clone())
    }

    /// Fetches the users mentioned or authoring these messages that are not known yet.
    pub async fn learn_users(&mut self, messages: &[Message]) -> Result<()> {
        let mentioned = messages.iter().flat_map(|m| mentioned_users(&m.text).into_iter().chain(m.user.clone()));
        let mut unknown: Vec<String> = mentioned.filter(|id| !self.users.iter().any(|u| &u.id == id)).collect();
        unknown.sort();
        unknown.dedup();
        if unknown.is_empty() {
            return Ok(());
        }
        if self.users.is_empty() && !self.users_fresh {
            self.refresh_users().await?;
            unknown.retain(|id| !self.users.iter().any(|u| &u.id == id));
        }
        for id in unknown {
            if let Ok(user) = self.slack.user_info(&id).await {
                self.users.push(user);
            }
        }
        self.cache.save("users", &self.users)
    }

    pub fn names(&self) -> NameBook {
        let channels = self.channels.iter().map(|c| (c.id.clone(), self.display_channel(c))).collect();
        NameBook {
            users: self.users.iter().map(|u| (u.id.clone(), u.handle().to_owned())).collect(),
            handles: self.users.iter().map(|u| (u.handle().to_lowercase(), u.id.clone())).collect(),
            channels,
            channel_names: self.channels.iter().filter(|c| !c.name.is_empty()).map(|c| (c.name.clone(), c.id.clone())).collect(),
        }
    }

    pub fn display_channel(&self, channel: &Channel) -> String {
        match channel.kind() {
            ChannelKind::Dm => {
                let user = channel.user.as_deref().unwrap_or("");
                let handle = self.users.iter().find(|u| u.id == user).map(|u| u.handle().to_owned()).unwrap_or_else(|| user.to_owned());
                format!("@{handle}")
            }
            ChannelKind::GroupDm => channel.name.replace("mpdm-", "").replace("--", ", ").trim_end_matches("-1").to_owned(),
            ChannelKind::Private => format!("🔒{}", channel.name),
            ChannelKind::Public => format!("#{}", channel.name),
        }
    }

    pub fn users_snapshot(&self) -> &[User] {
        &self.users
    }

    pub fn channels_snapshot(&self) -> &[Channel] {
        &self.channels
    }
}

static MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<@([UW][A-Z0-9]+)(?:\|[^>]*)?>").unwrap());

fn mentioned_users(text: &str) -> Vec<String> {
    MENTION.captures_iter(text).map(|c| c[1].to_owned()).collect()
}

#[derive(Clone, Debug, Default)]
pub struct NameBook {
    users: HashMap<String, String>,
    handles: HashMap<String, String>,
    channels: HashMap<String, String>,
    channel_names: HashMap<String, String>,
}

impl NameBook {
    pub fn channel_label(&self, id: &str) -> String {
        self.channels.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }

    pub fn user_label(&self, id: &str) -> String {
        self.users.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }
}

impl mrkdwn::Names for NameBook {
    fn user(&self, id: &str) -> Option<String> {
        self.users.get(id).cloned()
    }
    fn channel(&self, id: &str) -> Option<String> {
        self.channels.get(id).map(|n| n.trim_start_matches(['#', '🔒']).to_owned())
    }
}

impl markdown::Mentions for NameBook {
    fn user(&self, handle: &str) -> Option<String> {
        self.handles.get(&handle.to_lowercase()).cloned()
    }
    fn channel(&self, name: &str) -> Option<String> {
        self.channel_names.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Credentials;
    use wiremock::matchers::{body_string_contains, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn directory(server: &MockServer) -> Directory {
        let slack = Slack::new(&server.uri(), Credentials::new("t", None)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.keep());
        Directory::new(slack, cache)
    }

    fn ok(body: serde_json::Value) -> ResponseTemplate {
        let mut body = body;
        body["ok"] = serde_json::json!(true);
        ResponseTemplate::new(200).set_body_json(body)
    }

    #[tokio::test]
    async fn resolves_channel_by_name_after_fetch() {
        let server = MockServer::start().await;
        Mock::given(path("/conversations.list"))
            .respond_with(ok(
                serde_json::json!({"channels": [{"id": "C1", "name": "general"}, {"id": "C2", "name": "general-fr", "is_private": true}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let mut d = directory(&server).await;
        assert_eq!(d.channel_id("#general").await.unwrap(), "C1");
        assert_eq!(d.channel_id("general-fr").await.unwrap(), "C2");
        assert_eq!(d.channel_id("C0AAAAAAAA").await.unwrap(), "C0AAAAAAAA");
        let err = d.channel_id("gener").await.unwrap_err().to_string();
        assert!(err.contains("#general, 🔒general-fr") || err.contains("#general"), "{err}");
    }

    #[tokio::test]
    async fn dm_target_opens_conversation() {
        let server = MockServer::start().await;
        Mock::given(path("/users.list"))
            .respond_with(ok(serde_json::json!({"members": [{"id": "U1", "name": "vmeyet", "profile": {"display_name": "vivien"}}]})))
            .mount(&server)
            .await;
        Mock::given(path("/conversations.open"))
            .and(body_string_contains("users=U1"))
            .respond_with(ok(serde_json::json!({"channel": {"id": "D1"}})))
            .mount(&server)
            .await;
        let mut d = directory(&server).await;
        assert_eq!(d.channel_id("@vivien").await.unwrap(), "D1");
        assert_eq!(d.channel_id("@VMEYET").await.unwrap(), "D1");
        assert!(d.channel_id("@ghost").await.unwrap_err().to_string().contains("@ghost"));
    }

    #[tokio::test]
    async fn learns_unknown_authors_one_by_one() {
        let server = MockServer::start().await;
        Mock::given(path("/users.list")).respond_with(ok(serde_json::json!({"members": []}))).mount(&server).await;
        Mock::given(path("/users.info"))
            .and(body_string_contains("user=U9"))
            .respond_with(ok(serde_json::json!({"user": {"id": "U9", "name": "bob"}})))
            .expect(1)
            .mount(&server)
            .await;
        let mut d = directory(&server).await;
        let messages = vec![Message { ts: "1".into(), user: Some("U9".into()), text: "hi <@U9>".into(), ..Default::default() }];
        d.learn_users(&messages).await.unwrap();
        assert_eq!(d.names().user_label("U9"), "bob");
        assert_eq!(d.names().user_label("U0"), "U0");
    }

    #[test]
    fn namebook_maps_both_directions() {
        let slack = Slack::new("http://x", Credentials::new("t", None)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut d = Directory::new(slack, Cache::new(dir.keep()));
        d.users.push(User {
            id: "U1".into(),
            name: "vmeyet".into(),
            profile: crate::api::Profile { display_name: "Vivien".into(), ..Default::default() },
            ..Default::default()
        });
        d.channels.push(Channel { id: "C1".into(), name: "general".into(), ..Default::default() });
        d.channels.push(Channel { id: "D1".into(), is_im: true, user: Some("U1".into()), ..Default::default() });
        let names = d.names();
        assert_eq!(markdown::Mentions::user(&names, "vivien").as_deref(), Some("U1"));
        assert_eq!(markdown::Mentions::channel(&names, "general").as_deref(), Some("C1"));
        assert_eq!(mrkdwn::Names::user(&names, "U1").as_deref(), Some("Vivien"));
        assert_eq!(mrkdwn::Names::channel(&names, "C1").as_deref(), Some("general"));
        assert_eq!(names.channel_label("D1"), "@Vivien");
    }
}
