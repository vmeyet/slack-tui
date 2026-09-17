use crate::api::{Channel, ChannelKind, Message, Slack, User};
use crate::cache::Cache;
use crate::{fuzzy, markdown, mrkdwn};
use anyhow::{Result, bail};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

static CHANNEL_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[CDG][A-Z0-9]{8,}$").unwrap());
static USER_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[UW][A-Z0-9]{8,}$").unwrap());

/// Channels and users of one workspace, cached on disk and refreshed on a miss.
pub struct Directory {
    slack: Slack,
    cache: Cache,
    channels: Vec<Channel>,
    users: Vec<User>,
    user_at: HashMap<String, usize>,
    names: NameBook,
    channels_fresh: bool,
    users_fresh: bool,
}

impl Directory {
    pub fn new(slack: Slack, cache: Cache) -> Self {
        let channels = cache.load("channels").unwrap_or_default();
        let users = cache.load("users").unwrap_or_default();
        let mut dir = Self {
            slack,
            cache,
            channels: vec![],
            users: vec![],
            user_at: HashMap::new(),
            names: NameBook::default(),
            channels_fresh: false,
            users_fresh: false,
        };
        dir.set_channels(channels);
        dir.set_users(users);
        dir
    }

    fn set_channels(&mut self, channels: Vec<Channel>) {
        self.channels = channels;
        self.names = self.build_names();
    }

    fn set_users(&mut self, users: Vec<User>) {
        self.user_at = users.iter().enumerate().map(|(i, u)| (u.id.clone(), i)).collect();
        self.users = users;
        self.names = self.build_names();
    }

    fn add_users(&mut self, users: Vec<User>) {
        if users.is_empty() {
            return;
        }
        let all = std::mem::take(&mut self.users).into_iter().chain(users).collect();
        self.set_users(all);
    }

    fn user(&self, id: &str) -> Option<&User> {
        self.user_at.get(id).map(|&i| &self.users[i])
    }

    pub async fn refresh_channels(&mut self) -> Result<()> {
        let channels = self.slack.channels().await?;
        self.set_channels(channels);
        self.channels_fresh = true;
        self.cache.save("channels", &self.channels)
    }

    pub async fn refresh_users(&mut self) -> Result<()> {
        let users = self.slack.users().await?;
        self.set_users(users);
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
        let named = || self.channels.iter().filter(|c| !c.name.is_empty()).map(|c| (c.name.clone(), c));
        if let Some(c) = fuzzy::best(name, named()) {
            eprintln!("→ #{}", c.name);
            return Ok(c.id.clone());
        }
        let close: Vec<String> = fuzzy::rank(name, named()).into_iter().map(|(_, c)| format!("#{}", c.name)).take(5).collect();
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
        let people = || self.users.iter().filter(|u| !u.deleted && !u.is_bot).map(|u| (format!("{} {}", u.handle(), u.real_name), u));
        if let Some(u) = fuzzy::best(handle, people()) {
            eprintln!("→ @{}", u.handle());
            return Ok(u.id.clone());
        }
        let close: Vec<String> = fuzzy::rank(handle, people()).into_iter().map(|(_, u)| format!("@{}", u.handle())).take(5).collect();
        match close.is_empty() {
            true => bail!("user `@{handle}` not found"),
            false => bail!("user `@{handle}` not found. Did you mean: {}", close.join(", ")),
        }
    }

    pub fn people(&self) -> Vec<(String, String)> {
        let mut people: Vec<(String, String)> =
            self.users.iter().filter(|u| !u.deleted && !u.is_bot).map(|u| (u.id.clone(), u.handle().to_owned())).collect();
        people.sort_by_key(|(_, h)| h.to_lowercase());
        people
    }

    fn find_user(&self, handle: &str) -> Option<String> {
        let lower = handle.to_lowercase();
        let exact = |u: &&User| u.handle().eq_ignore_ascii_case(handle) || u.name.eq_ignore_ascii_case(handle);
        let loose = |u: &&User| u.real_name.to_lowercase() == lower || u.profile.real_name.to_lowercase() == lower;
        let active = || self.users.iter().filter(|u| !u.deleted);
        active().find(exact).or_else(|| active().find(loose)).map(|u| u.id.clone())
    }

    /// Conversations worth showing: public, private, group DMs, then DMs, each alphabetical.
    /// Without `all`, DMs with bots and deactivated people are hidden.
    pub fn conversations(&self, all: bool) -> Vec<(&Channel, String)> {
        let mut rows: Vec<(&Channel, String)> = self
            .channels
            .iter()
            .filter(|c| all || c.is_member || c.is_mpim || (c.is_im && self.is_person(c.user.as_deref().unwrap_or(""))))
            .map(|c| (c, self.display_channel(c)))
            .collect();
        rows.sort_by_cached_key(|(c, label)| (kind_rank(c.kind()), label.trim_start_matches(['#', '🔒', '@']).to_lowercase()));
        rows
    }

    fn is_person(&self, user_id: &str) -> bool {
        !SLACKBOT.contains(&user_id) && self.user(user_id).is_none_or(|u| !u.deleted && !u.is_bot)
    }

    /// DMs can point at people `users.list` no longer returns (deactivated, app users); fetch those one by one.
    pub async fn learn_dm_users(&mut self) -> Result<()> {
        let unknown: Vec<String> = self
            .channels
            .iter()
            .filter(|c| c.is_im)
            .filter_map(|c| c.user.clone())
            .filter(|id| !SLACKBOT.contains(&id.as_str()) && self.user(id).is_none())
            .collect();
        self.fetch_users(unknown).await
    }

    /// Fetches the users mentioned or authoring these messages that are not known yet.
    pub async fn learn_users(&mut self, messages: &[Message]) -> Result<()> {
        let ids: Vec<String> = messages.iter().flat_map(|m| mentioned_users(&m.text).into_iter().chain(m.user.clone())).collect();
        self.learn_ids(&ids).await
    }

    pub async fn learn_ids(&mut self, ids: &[String]) -> Result<()> {
        let mut unknown: Vec<String> = ids.iter().filter(|id| self.user(id).is_none()).cloned().collect();
        unknown.sort();
        unknown.dedup();
        if unknown.is_empty() {
            return Ok(());
        }
        if self.users.is_empty() && !self.users_fresh {
            self.refresh_users().await?;
            unknown.retain(|id| self.user(id).is_none());
        }
        self.fetch_users(unknown).await
    }

    async fn fetch_users(&mut self, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut found = vec![];
        for id in ids {
            if let Ok(user) = self.slack.user_info(&id).await {
                found.push(user);
            }
        }
        self.add_users(found);
        self.cache.save("users", &self.users)
    }

    pub fn names(&self) -> NameBook {
        self.names.clone()
    }

    fn build_names(&self) -> NameBook {
        NameBook(Arc::new(Tables {
            users: self.users.iter().map(|u| (u.id.clone(), u.handle().to_owned())).collect(),
            handles: self.users.iter().map(|u| (u.handle().to_lowercase(), u.id.clone())).collect(),
            channels: self.channels.iter().map(|c| (c.id.clone(), self.display_channel(c))).collect(),
            channel_names: self.channels.iter().filter(|c| !c.name.is_empty()).map(|c| (c.name.clone(), c.id.clone())).collect(),
        }))
    }

    pub fn display_channel(&self, channel: &Channel) -> String {
        match channel.kind() {
            ChannelKind::Dm => {
                let user = channel.user.as_deref().unwrap_or("");
                let handle = self.user(user).map_or(user, User::handle);
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

const SLACKBOT: [&str; 2] = ["USLACKBOT", "USLACK"];

fn kind_rank(kind: ChannelKind) -> u8 {
    match kind {
        ChannelKind::Public => 0,
        ChannelKind::Private => 1,
        ChannelKind::GroupDm => 2,
        ChannelKind::Dm => 3,
    }
}

static MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<@([UW][A-Z0-9]+)(?:\|[^>]*)?>").unwrap());

fn mentioned_users(text: &str) -> Vec<String> {
    MENTION.captures_iter(text).map(|c| c[1].to_owned()).collect()
}

/// Cheap to clone: every copy shares the same tables.
#[derive(Clone, Debug, Default)]
pub struct NameBook(Arc<Tables>);

#[derive(Debug, Default)]
struct Tables {
    users: HashMap<String, String>,
    handles: HashMap<String, String>,
    channels: HashMap<String, String>,
    channel_names: HashMap<String, String>,
}

impl NameBook {
    pub fn channel_label(&self, id: &str) -> String {
        self.0.channels.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }

    pub fn user_label(&self, id: &str) -> String {
        self.0.users.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }
}

impl mrkdwn::Names for NameBook {
    fn user(&self, id: &str) -> Option<String> {
        self.0.users.get(id).cloned()
    }
    fn channel(&self, id: &str) -> Option<String> {
        self.0.channels.get(id).map(|n| n.trim_start_matches(['#', '🔒']).to_owned())
    }
}

impl markdown::Mentions for NameBook {
    fn user(&self, handle: &str) -> Option<String> {
        self.0.handles.get(&handle.to_lowercase()).cloned()
    }
    fn channel(&self, name: &str) -> Option<String> {
        self.0.channel_names.get(name).cloned()
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
        assert!(err.contains("Did you mean: #general, #general-fr"), "{err}");
        assert_eq!(d.channel_id("gfr").await.unwrap(), "C2");
        assert!(d.channel_id("zzz").await.unwrap_err().to_string().contains("not found"));
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
    fn conversations_are_grouped_sorted_and_filtered() {
        let slack = Slack::new("http://x", Credentials::new("t", None)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut d = Directory::new(slack, Cache::new(dir.keep()));
        d.add_users(vec![
            User { id: "U1".into(), name: "zoe".into(), ..Default::default() },
            User { id: "U2".into(), name: "gone".into(), deleted: true, ..Default::default() },
            User { id: "U3".into(), name: "robot".into(), is_bot: true, ..Default::default() },
            User { id: "U4".into(), name: "adam".into(), ..Default::default() },
        ]);
        d.set_channels(vec![
            Channel { id: "D1".into(), is_im: true, user: Some("U1".into()), ..Default::default() },
            Channel { id: "D2".into(), is_im: true, user: Some("U2".into()), ..Default::default() },
            Channel { id: "D3".into(), is_im: true, user: Some("U3".into()), ..Default::default() },
            Channel { id: "D4".into(), is_im: true, user: Some("U4".into()), ..Default::default() },
            Channel { id: "D5".into(), is_im: true, user: Some("USLACK".into()), ..Default::default() },
            Channel { id: "C1".into(), name: "zebra".into(), is_member: true, ..Default::default() },
            Channel { id: "C2".into(), name: "apple".into(), is_member: true, is_private: true, ..Default::default() },
            Channel { id: "C3".into(), name: "Beta".into(), is_member: true, ..Default::default() },
            Channel { id: "C4".into(), name: "not-mine".into(), is_member: false, ..Default::default() },
            Channel { id: "G1".into(), name: "mpdm-zoe--adam-1".into(), is_mpim: true, ..Default::default() },
        ]);
        let labels: Vec<String> = d.conversations(false).into_iter().map(|(_, l)| l).collect();
        assert_eq!(labels, ["#Beta", "#zebra", "🔒apple", "zoe, adam", "@adam", "@zoe"]);
        assert_eq!(d.conversations(true).len(), 10);
        assert_eq!(d.find_user("gone"), None);
    }

    #[tokio::test]
    async fn dm_users_missing_from_the_list_are_fetched() {
        let server = MockServer::start().await;
        Mock::given(path("/users.info"))
            .and(body_string_contains("user=U7"))
            .respond_with(ok(serde_json::json!({"user": {"id": "U7", "name": "old-bot", "is_bot": true, "deleted": true}})))
            .expect(1)
            .mount(&server)
            .await;
        let mut d = directory(&server).await;
        d.set_channels(vec![
            Channel { id: "D7".into(), is_im: true, user: Some("U7".into()), ..Default::default() },
            Channel { id: "D8".into(), is_im: true, user: Some("USLACKBOT".into()), ..Default::default() },
        ]);
        assert_eq!(d.conversations(false).len(), 1);
        d.learn_dm_users().await.unwrap();
        d.learn_dm_users().await.unwrap();
        assert!(d.conversations(false).is_empty());
    }

    #[test]
    fn namebook_maps_both_directions() {
        let slack = Slack::new("http://x", Credentials::new("t", None)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut d = Directory::new(slack, Cache::new(dir.keep()));
        d.add_users(vec![User {
            id: "U1".into(),
            name: "vmeyet".into(),
            profile: crate::api::Profile { display_name: "Vivien".into(), ..Default::default() },
            ..Default::default()
        }]);
        d.set_channels(vec![
            Channel { id: "C1".into(), name: "general".into(), ..Default::default() },
            Channel { id: "D1".into(), is_im: true, user: Some("U1".into()), ..Default::default() },
        ]);
        let names = d.names();
        assert_eq!(markdown::Mentions::user(&names, "vivien").as_deref(), Some("U1"));
        assert_eq!(markdown::Mentions::channel(&names, "general").as_deref(), Some("C1"));
        assert_eq!(mrkdwn::Names::user(&names, "U1").as_deref(), Some("Vivien"));
        assert_eq!(mrkdwn::Names::channel(&names, "C1").as_deref(), Some("general"));
        assert_eq!(names.channel_label("D1"), "@Vivien");
    }
}
