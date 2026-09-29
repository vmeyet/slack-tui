use crate::api::{Channel, ChannelKind, Group, Message, Slack, User, in_parallel};
use crate::cache::Cache;
use crate::pattern::regex;
use crate::{fuzzy, markdown, mrkdwn};
use anyhow::{Result, bail};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

static CHANNEL_ID: LazyLock<Regex> = LazyLock::new(|| regex(r"^[CDG][A-Z0-9]{8,}$"));
static USER_ID: LazyLock<Regex> = LazyLock::new(|| regex(r"^[UW][A-Z0-9]{8,}$"));

/// Channels, users and usergroups of one workspace, cached on disk and refreshed on a miss.
/// Clones share what is known; the lock is a sync one so it can never be held while Slack answers.
#[derive(Clone)]
pub struct Directory {
    slack: Slack,
    cache: Cache,
    known: Arc<Mutex<Known>>,
}

#[derive(Default)]
struct Known {
    channels: Vec<Channel>,
    users: Vec<User>,
    groups: Vec<Group>,
    user_at: HashMap<String, usize>,
    names: NameBook,
    channels_fresh: bool,
    users_fresh: bool,
    groups_fresh: bool,
}

impl Directory {
    pub async fn new(slack: Slack, cache: Cache) -> Self {
        let mut known = Known::default();
        known.set_channels(cache.load("channels").await.unwrap_or_default());
        known.set_users(cache.load("users").await.unwrap_or_default());
        known.set_groups(cache.load("usergroups").await.unwrap_or_default());
        Self { slack, cache, known: Arc::new(Mutex::new(known)) }
    }

    fn lock(&self) -> MutexGuard<'_, Known> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub async fn refresh_channels(&self) -> Result<()> {
        let channels = self.slack.channels().await?;
        let saved = self.cache.save("channels", &channels).await;
        let mut known = self.lock();
        known.set_channels(channels);
        known.channels_fresh = true;
        saved
    }

    pub async fn refresh_users(&self) -> Result<()> {
        let users = self.slack.users().await?;
        let saved = self.cache.save("users", &users).await;
        let mut known = self.lock();
        known.set_users(users);
        known.users_fresh = true;
        saved
    }

    async fn refresh_groups(&self) -> Result<()> {
        let groups = self.slack.groups().await?;
        let saved = self.cache.save("usergroups", &groups).await;
        self.lock().set_groups(groups);
        saved
    }

    pub async fn channels(&self) -> Result<()> {
        if self.lock().channels.is_empty() {
            self.refresh_channels().await?;
        }
        Ok(())
    }

    pub async fn users(&self) -> Result<()> {
        if self.lock().users.is_empty() {
            self.refresh_users().await?;
        }
        Ok(())
    }

    /// `#name`, `name`, `@handle`, or a raw id, to a conversation id.
    pub async fn channel_id(&self, target: &str) -> Result<String> {
        let target = target.trim();
        if CHANNEL_ID.is_match(target) {
            return Ok(target.to_owned());
        }
        if let Some(handle) = target.strip_prefix('@') {
            let user = self.user_id(handle).await?;
            return self.slack.open_dm(&user).await;
        }
        let name = target.trim_start_matches('#');
        let found = self.lock().find_channel(name);
        if let Some(id) = found {
            return Ok(id);
        }
        let fresh = self.lock().channels_fresh;
        if !fresh {
            self.refresh_channels().await?;
            let found = self.lock().find_channel(name);
            if let Some(id) = found {
                return Ok(id);
            }
        }
        self.lock().closest_channel(target, name)
    }

    pub async fn user_id(&self, handle: &str) -> Result<String> {
        let handle = handle.trim().trim_start_matches('@');
        if USER_ID.is_match(handle) {
            return Ok(handle.to_owned());
        }
        self.users().await?;
        let found = self.lock().find_user(handle);
        if let Some(id) = found {
            return Ok(id);
        }
        let fresh = self.lock().users_fresh;
        if !fresh {
            self.refresh_users().await?;
            let found = self.lock().find_user(handle);
            if let Some(id) = found {
                return Ok(id);
            }
        }
        self.lock().closest_user(handle)
    }

    pub fn people(&self) -> Vec<(String, String)> {
        self.lock().people()
    }

    /// Conversations worth showing: public, private, group DMs, then DMs, each alphabetical.
    /// Without `all`, DMs with bots and deactivated people are hidden.
    pub fn conversations(&self, all: bool) -> Vec<(Channel, String)> {
        self.lock().conversations(all)
    }

    /// DMs can point at people `users.list` no longer returns (deactivated, app users); fetch those one by one.
    pub async fn learn_dm_users(&self) -> Result<()> {
        let unknown = self.lock().unknown_dm_users();
        self.fetch_users(unknown).await
    }

    /// Fetches the users and usergroups mentioned or authoring these messages that are not known yet.
    pub async fn learn_users(&self, messages: &[Message]) -> Result<()> {
        let ids: Vec<String> = messages.iter().flat_map(|m| mentioned_users(&m.text).into_iter().chain(m.user.clone())).collect();
        self.learn_groups(messages).await;
        self.learn_ids(&ids).await
    }

    /// Usergroups only come as one whole list, so fetch it once, the first time a message names one we cannot read.
    /// A workspace without usergroups, or a token without the scope, keeps rendering: the id stands in for the handle.
    pub async fn learn_groups(&self, messages: &[Message]) {
        let first = self.lock().claim_groups_fetch(messages);
        if first {
            let _ = self.refresh_groups().await;
        }
    }

    /// The usergroups `me` belongs to, or nothing while the workspace has not said who is in
    /// them — a token without the scope must not turn every group mention into a miss.
    pub fn my_groups(&self, me: &str) -> Option<HashSet<String>> {
        self.lock().my_groups(me)
    }

    pub async fn learn_ids(&self, ids: &[String]) -> Result<()> {
        let mut unknown = self.lock().unknown_users(ids);
        if unknown.is_empty() {
            return Ok(());
        }
        let never_listed = self.lock().never_listed_users();
        if never_listed {
            self.refresh_users().await?;
            unknown = self.lock().unknown_users(&unknown);
        }
        self.fetch_users(unknown).await
    }

    async fn fetch_users(&self, ids: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let found = in_parallel(ids, |id| async move { Ok(self.slack.user_info(&id).await.ok()) }).await?;
        let users = {
            let mut known = self.lock();
            known.add_users(found.into_iter().flatten().collect());
            known.users.clone()
        };
        self.cache.save("users", &users).await
    }

    pub fn names(&self) -> NameBook {
        self.lock().names.clone()
    }

    pub fn users_snapshot(&self) -> Vec<User> {
        self.lock().users.clone()
    }

    pub fn has_dms(&self) -> bool {
        self.lock().channels.iter().any(|c| c.is_im)
    }
}

impl Known {
    fn set_channels(&mut self, channels: Vec<Channel>) {
        self.channels = channels;
        self.names = self.build_names();
    }

    fn set_users(&mut self, users: Vec<User>) {
        self.user_at = users.iter().enumerate().map(|(i, u)| (u.id.clone(), i)).collect();
        self.users = users;
        self.names = self.build_names();
    }

    fn set_groups(&mut self, groups: Vec<Group>) {
        self.groups = groups;
        self.names = self.build_names();
    }

    /// Two loads can fetch the same person at once; only the first copy is kept.
    fn add_users(&mut self, users: Vec<User>) {
        let new: Vec<User> = users.into_iter().filter(|u| self.user(&u.id).is_none()).collect();
        if new.is_empty() {
            return;
        }
        let all = std::mem::take(&mut self.users).into_iter().chain(new).collect();
        self.set_users(all);
    }

    fn user(&self, id: &str) -> Option<&User> {
        self.user_at.get(id).map(|&i| &self.users[i])
    }

    fn find_channel(&self, name: &str) -> Option<String> {
        self.channels.iter().find(|c| c.name == name).map(|c| c.id.clone())
    }

    fn closest_channel(&self, target: &str, name: &str) -> Result<String> {
        let named = || self.channels.iter().filter(|c| !c.name.is_empty()).map(|c| (c.name.as_str(), c));
        if let Some(c) = fuzzy::best(name, named()) {
            eprintln!("→ #{}", c.name);
            return Ok(c.id.clone());
        }
        let close: Vec<String> = fuzzy::rank(name, named()).into_iter().map(|(_, c)| format!("#{}", c.name)).take(5).collect();
        if close.is_empty() {
            bail!("channel `{target}` not found (are you a member?)");
        }
        bail!("channel `{target}` not found. Did you mean: {}", close.join(", "))
    }

    fn find_user(&self, handle: &str) -> Option<String> {
        let lower = handle.to_lowercase();
        let exact = |u: &&User| u.handle().eq_ignore_ascii_case(handle) || u.name.eq_ignore_ascii_case(handle);
        let loose = |u: &&User| u.real_name.to_lowercase() == lower || u.profile.real_name.to_lowercase() == lower;
        let active = || self.users.iter().filter(|u| !u.deleted);
        active().find(exact).or_else(|| active().find(loose)).map(|u| u.id.clone())
    }

    fn closest_user(&self, handle: &str) -> Result<String> {
        let labelled: Vec<(String, &User)> =
            self.users.iter().filter(|u| !u.deleted && !u.is_bot).map(|u| (format!("{} {}", u.handle(), u.real_name), u)).collect();
        let people = || labelled.iter().map(|(label, u)| (label.as_str(), *u));
        if let Some(u) = fuzzy::best(handle, people()) {
            eprintln!("→ @{}", u.handle());
            return Ok(u.id.clone());
        }
        let close: Vec<String> = fuzzy::rank(handle, people()).into_iter().map(|(_, u)| format!("@{}", u.handle())).take(5).collect();
        if close.is_empty() {
            bail!("user `@{handle}` not found");
        }
        bail!("user `@{handle}` not found. Did you mean: {}", close.join(", "))
    }

    fn people(&self) -> Vec<(String, String)> {
        let mut people: Vec<(String, String)> =
            self.users.iter().filter(|u| !u.deleted && !u.is_bot).map(|u| (u.id.clone(), u.handle().to_owned())).collect();
        people.sort_by_key(|(_, h)| h.to_lowercase());
        people
    }

    fn conversations(&self, all: bool) -> Vec<(Channel, String)> {
        let mut rows: Vec<(Channel, String)> = self
            .channels
            .iter()
            .filter(|c| all || c.is_member || c.is_mpim || (c.is_im && self.is_person(c.user.as_deref().unwrap_or(""))))
            .map(|c| (c.clone(), self.display_channel(c)))
            .collect();
        rows.sort_by_cached_key(|(c, label)| (kind_rank(c.kind()), label.trim_start_matches(['#', '🔒', '@']).to_lowercase()));
        rows
    }

    fn is_person(&self, user_id: &str) -> bool {
        !SLACKBOT.contains(&user_id) && self.user(user_id).is_none_or(|u| !u.deleted && !u.is_bot)
    }

    fn unknown_dm_users(&self) -> Vec<String> {
        self.channels
            .iter()
            .filter(|c| c.is_im)
            .filter_map(|c| c.user.clone())
            .filter(|id| !SLACKBOT.contains(&id.as_str()) && self.user(id).is_none())
            .collect()
    }

    fn unknown_users(&self, ids: &[String]) -> Vec<String> {
        let mut unknown: Vec<String> = ids.iter().filter(|id| self.user(id).is_none()).cloned().collect();
        unknown.sort();
        unknown.dedup();
        unknown
    }

    fn never_listed_users(&self) -> bool {
        self.users.is_empty() && !self.users_fresh
    }

    /// True for the one caller that should fetch the usergroups; later callers read what it stored.
    fn claim_groups_fetch(&mut self, messages: &[Message]) -> bool {
        let unknown = |m: &Message| mentioned_groups(&m.text).iter().any(|id| !self.knows_group(id));
        if self.groups_fresh || !messages.iter().any(unknown) {
            return false;
        }
        self.groups_fresh = true;
        true
    }

    fn knows_group(&self, id: &str) -> bool {
        self.groups.iter().any(|g| g.id == id)
    }

    fn my_groups(&self, me: &str) -> Option<HashSet<String>> {
        let listed = self.groups.iter().any(|g| !g.users.is_empty());
        listed.then(|| self.groups.iter().filter(|g| g.users.iter().any(|u| u == me)).map(|g| g.id.clone()).collect())
    }

    fn build_names(&self) -> NameBook {
        NameBook(Arc::new(Tables {
            users: self.users.iter().map(|u| (u.id.clone(), u.handle().to_owned())).collect(),
            handles: self.users.iter().map(|u| (u.handle().to_lowercase(), u.id.clone())).collect(),
            channels: self.channels.iter().map(|c| (c.id.clone(), self.display_channel(c))).collect(),
            channel_names: self.channels.iter().filter(|c| !c.name.is_empty()).map(|c| (c.name.clone(), c.id.clone())).collect(),
            groups: self.groups.iter().filter(|g| g.is_live() && !g.handle.is_empty()).map(|g| (g.id.clone(), g.handle.clone())).collect(),
        }))
    }

    fn display_channel(&self, channel: &Channel) -> String {
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

static MENTION: LazyLock<Regex> = LazyLock::new(|| regex(r"<@([UW][A-Z0-9]+)(?:\|[^>]*)?>"));

fn mentioned_users(text: &str) -> Vec<String> {
    MENTION.captures_iter(text).map(|c| c[1].to_owned()).collect()
}

static GROUP_MENTION: LazyLock<Regex> = LazyLock::new(|| regex(r"<!subteam\^([A-Z0-9]+)"));

fn mentioned_groups(text: &str) -> Vec<String> {
    GROUP_MENTION.captures_iter(text).map(|c| c[1].to_owned()).collect()
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
    groups: HashMap<String, String>,
}

impl NameBook {
    pub fn channel_label(&self, id: &str) -> String {
        self.0.channels.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }

    pub fn user_label(&self, id: &str) -> String {
        self.0.users.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }

    /// Both copies share the same tables, so they know the same names.
    pub fn same(&self, other: &NameBook) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl mrkdwn::Names for NameBook {
    fn user(&self, id: &str) -> Option<String> {
        self.0.users.get(id).cloned()
    }
    fn channel(&self, id: &str) -> Option<String> {
        self.0.channels.get(id).map(|n| n.trim_start_matches(['#', '🔒']).to_owned())
    }
    fn group(&self, id: &str) -> Option<String> {
        self.0.groups.get(id).cloned()
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
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::auth::Credentials;
    use wiremock::matchers::{body_string_contains, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn directory(server: &MockServer) -> Directory {
        let slack = Slack::new(&server.uri(), Credentials::new("t", None)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.keep());
        Directory::new(slack, cache).await
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
        let d = directory(&server).await;
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
        let d = directory(&server).await;
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
        let d = directory(&server).await;
        let messages = vec![Message { ts: "1".into(), user: Some("U9".into()), text: "hi <@U9>".into(), ..Default::default() }];
        d.learn_users(&messages).await.unwrap();
        assert_eq!(d.names().user_label("U9"), "bob");
        assert_eq!(d.names().user_label("U0"), "U0");
    }

    /// A directory that already listed its users, and a `users.info` for U9 that takes a while.
    async fn slow_user_lookup(server: &MockServer) -> Directory {
        Mock::given(path("/users.info"))
            .respond_with(ok(serde_json::json!({"user": {"id": "U9", "name": "bob"}})).set_delay(std::time::Duration::from_millis(200)))
            .mount(server)
            .await;
        let d = directory(server).await;
        d.lock().add_users(vec![User { id: "U1".into(), name: "ann".into(), ..Default::default() }]);
        d
    }

    #[tokio::test]
    async fn names_stay_readable_while_a_user_is_fetched() {
        let server = MockServer::start().await;
        let d = slow_user_lookup(&server).await;
        let ids = ["U9".to_owned()];
        let peek = async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            d.known.try_lock().is_ok()
        };
        let (learned, readable) = tokio::join!(d.learn_ids(&ids), peek);
        learned.unwrap();
        assert!(readable, "the lock is free while Slack answers");
        assert_eq!(d.names().user_label("U9"), "bob");
    }

    #[tokio::test]
    async fn a_user_learned_twice_at_once_is_kept_once() {
        let server = MockServer::start().await;
        let d = slow_user_lookup(&server).await;
        let ids = ["U9".to_owned()];
        let (first, second) = tokio::join!(d.learn_ids(&ids), d.learn_ids(&ids));
        first.unwrap();
        second.unwrap();
        assert_eq!(d.users_snapshot().iter().filter(|u| u.id == "U9").count(), 1);
    }

    fn mentioning_groups(text: &str) -> Vec<Message> {
        vec![Message { ts: "1".into(), text: text.into(), ..Default::default() }]
    }

    #[tokio::test]
    async fn usergroups_are_fetched_once_and_skip_the_disbanded() {
        let server = MockServer::start().await;
        Mock::given(path("/usergroups.list"))
            .respond_with(ok(serde_json::json!({"usergroups": [
                {"id": "S1", "handle": "team-x", "date_delete": 0},
                {"id": "S2", "handle": "team-gone", "date_delete": 1_700_000_000},
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let d = directory(&server).await;
        d.learn_users(&mentioning_groups("<!subteam^S1> <!subteam^S2> <!subteam^S3>")).await.unwrap();
        d.learn_users(&mentioning_groups("<!subteam^S3>")).await.unwrap();
        let names = d.names();
        assert_eq!(mrkdwn::Names::group(&names, "S1").as_deref(), Some("team-x"));
        assert_eq!(mrkdwn::Names::group(&names, "S2"), None);
        assert_eq!(mrkdwn::Names::group(&names, "S3"), None);
    }

    #[tokio::test]
    async fn a_usergroup_we_cannot_fetch_still_renders() {
        let server = MockServer::start().await;
        Mock::given(path("/usergroups.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": false, "error": "missing_scope"})))
            .mount(&server)
            .await;
        let d = directory(&server).await;
        d.learn_users(&mentioning_groups("<!subteam^S1>")).await.unwrap();
        assert_eq!(mrkdwn::plain("<!subteam^S1>", &d.names()), "@S1");
    }

    #[tokio::test]
    async fn conversations_are_grouped_sorted_and_filtered() {
        let slack = Slack::new("http://x", Credentials::new("t", None)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let d = Directory::new(slack, Cache::new(dir.keep())).await;
        d.lock().add_users(vec![
            User { id: "U1".into(), name: "zoe".into(), ..Default::default() },
            User { id: "U2".into(), name: "gone".into(), deleted: true, ..Default::default() },
            User { id: "U3".into(), name: "robot".into(), is_bot: true, ..Default::default() },
            User { id: "U4".into(), name: "adam".into(), ..Default::default() },
        ]);
        d.lock().set_channels(vec![
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
        assert_eq!(d.lock().find_user("gone"), None);
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
        let d = directory(&server).await;
        d.lock().set_channels(vec![
            Channel { id: "D7".into(), is_im: true, user: Some("U7".into()), ..Default::default() },
            Channel { id: "D8".into(), is_im: true, user: Some("USLACKBOT".into()), ..Default::default() },
        ]);
        assert_eq!(d.conversations(false).len(), 1);
        d.learn_dm_users().await.unwrap();
        d.learn_dm_users().await.unwrap();
        assert!(d.conversations(false).is_empty());
    }

    #[tokio::test]
    async fn namebook_maps_both_directions() {
        let slack = Slack::new("http://x", Credentials::new("t", None)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let d = Directory::new(slack, Cache::new(dir.keep())).await;
        d.lock().add_users(vec![User {
            id: "U1".into(),
            name: "vmeyet".into(),
            profile: crate::api::Profile { display_name: "Vivien".into(), ..Default::default() },
            ..Default::default()
        }]);
        d.lock().set_channels(vec![
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
