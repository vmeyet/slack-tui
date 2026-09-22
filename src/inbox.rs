//! Everything waiting for you: unread DMs, mentions, and threads with new replies.
use crate::api::{Message, ReadState, Slack};
use crate::cache::Cache;
use crate::mrkdwn;
use crate::resolve::{Directory, NameBook};
use crate::typesafe::{Judge, Question, Unavailable};
use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Dm,
    Mention,
    Thread,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    Low,
    Medium,
    High,
}

impl Urgency {
    const LEVELS: [&'static str; 3] = ["low", "medium", "high"];

    fn from_score(score: f64) -> Self {
        match score.round() as i64 {
            ..=0 => Urgency::Low,
            1 => Urgency::Medium,
            _ => Urgency::High,
        }
    }
}

/// What Jev made of an item: whether it waits on you, and how soon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Priority {
    pub needs_reply: bool,
    pub urgency: Urgency,
}

/// Priorities by the message they were judged on, so a refresh only asks about what is new.
pub type Verdicts = BTreeMap<String, Priority>;

const VERDICTS_CACHE: &str = "inbox-priorities";
const STATE_MESSAGES: usize = 5;
const STATE_TEXT_CHARS: usize = 2000;
const PRIORITY_QUESTIONS: [(&str, Question); 2] = [
    ("urgency", Question::Score("How urgently should `me` look at this Slack conversation?", &Urgency::LEVELS)),
    ("needs_reply", Question::Noul("Does the latest message expect an answer or an action from `me`?")),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Stable identity across refreshes: `channel/thread_ts` or `channel`.
    pub key: String,
    pub kind: Kind,
    pub channel: String,
    pub label: String,
    /// The thread to reply into, when the item lives in one.
    pub thread_ts: Option<String>,
    /// Newest unread ts; marking read means up to here.
    pub ts: String,
    pub unread: Vec<Message>,
    /// Only once `[typesafe] enabled` and Jev answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
}

impl Item {
    /// The newest unread message: a newer one needs a new verdict.
    fn verdict_key(&self) -> String {
        format!("{}/{}", self.channel, self.ts)
    }

    /// Where a reply from the inbox goes: the DM itself, or the thread of the message.
    pub fn reply_thread(&self) -> Option<String> {
        match self.kind {
            Kind::Dm => None,
            Kind::Mention | Kind::Thread => self.thread_ts.clone().or_else(|| self.unread.last().map(|m| m.ts.clone())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Snooze {
    OneHour,
    ThreeHours,
    Tomorrow,
    NextWeek,
}

impl Snooze {
    pub const ALL: [Snooze; 4] = [Snooze::OneHour, Snooze::ThreeHours, Snooze::Tomorrow, Snooze::NextWeek];

    pub fn label(self) -> &'static str {
        match self {
            Snooze::OneHour => "1 hour",
            Snooze::ThreeHours => "3 hours",
            Snooze::Tomorrow => "tomorrow 9:00",
            Snooze::NextWeek => "monday 9:00",
        }
    }

    pub fn until(self, now: DateTime<Local>) -> DateTime<Local> {
        let at_nine = |d: DateTime<Local>| d.with_hour(9).and_then(|d| d.with_minute(0)).and_then(|d| d.with_second(0)).unwrap_or(d);
        match self {
            Snooze::OneHour => now + chrono::Duration::hours(1),
            Snooze::ThreeHours => now + chrono::Duration::hours(3),
            Snooze::Tomorrow => at_nine(now + chrono::Duration::days(1)),
            Snooze::NextWeek => {
                let days = (7 - now.weekday().num_days_from_monday()) as i64;
                at_nine(now + chrono::Duration::days(if days == 0 { 7 } else { days }))
            }
        }
    }
}

/// Local memory of what was snoozed or dismissed, keyed by item, so nothing depends on
/// Slack's thread read state which has no public API.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub snoozed: BTreeMap<String, i64>,
    #[serde(default)]
    pub read: BTreeMap<String, String>,
}

impl State {
    pub fn path(workspace: &str) -> PathBuf {
        std::env::var_os("SLACK_CLI_STATE_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::state_dir().map(|d| d.join("slack-cli")))
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state/slack-cli")))
            .unwrap_or_else(|| PathBuf::from(".slack-cli-state"))
            .join(workspace)
            .join("inbox.json")
    }

    pub fn load(workspace: &str) -> Self {
        std::fs::read(Self::path(workspace)).ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default()
    }

    pub fn save(&self, workspace: &str) -> Result<()> {
        let path = Self::path(workspace);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, serde_json::to_vec(self)?).with_context(|| format!("writing {}", path.display()))
    }

    pub fn snooze(&mut self, key: &str, until: DateTime<Local>) {
        self.snoozed.insert(key.to_owned(), until.timestamp());
    }

    pub fn mark_read(&mut self, key: &str, ts: &str) {
        self.snoozed.remove(key);
        self.read.insert(key.to_owned(), ts.to_owned());
    }

    /// Items still worth showing now: not snoozed, and with something newer than what was dismissed.
    pub fn visible(&self, items: Vec<Item>, now: DateTime<Local>) -> Vec<Item> {
        items
            .into_iter()
            .filter(|i| self.snoozed.get(&i.key).is_none_or(|until| *until <= now.timestamp()))
            .filter(|i| self.read.get(&i.key).is_none_or(|ts| newer(&i.ts, ts)))
            .collect()
    }

    pub fn forget_expired(&mut self, now: DateTime<Local>) {
        self.snoozed.retain(|_, until| *until > now.timestamp());
    }
}

/// Slack timestamps as numbers, so short or oddly padded values still order correctly.
pub fn newer(a: &str, b: &str) -> bool {
    a.parse::<f64>().unwrap_or(0.0) > b.parse::<f64>().unwrap_or(0.0)
}

static MENTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(?:@([UW][A-Z0-9]+)|!(?:here|channel|everyone|subteam\^[A-Z0-9]+))(?:\|[^>]*)?>").unwrap());

pub fn mentions_me(text: &str, me: &str) -> bool {
    MENTION.captures_iter(text).any(|c| c.get(1).is_none_or(|u| u.as_str() == me))
}

/// Collects every unread item from Slack, newest first.
pub async fn fetch(slack: &Slack, dir: &mut Directory, me: &str) -> Result<Vec<Item>> {
    let counts = slack.counts().await?;
    let mut items = Vec::new();
    for state in counts.ims.iter().chain(&counts.mpims).filter(|s| s.has_unreads) {
        if let Some(item) = dm_item(slack, dir, state).await? {
            items.push(item);
        }
    }
    for state in counts.channels.iter().filter(|s| s.mention_count > 0) {
        items.extend(mention_items(slack, dir, state, me).await?);
    }
    items.extend(thread_items(slack, dir).await?);
    let all: Vec<Message> = items.iter().flat_map(|i| i.unread.clone()).collect();
    dir.learn_users(&all).await?;
    let names = dir.names();
    for item in &mut items {
        item.label = names.channel_label(&item.channel);
    }
    items.sort_by(|a, b| b.ts.parse::<f64>().unwrap_or(0.0).total_cmp(&a.ts.parse::<f64>().unwrap_or(0.0)));
    Ok(items)
}

async fn dm_item(slack: &Slack, dir: &mut Directory, state: &ReadState) -> Result<Option<Item>> {
    let unread: Vec<Message> =
        slack.history(&state.id, 20, Some(&state.last_read)).await?.into_iter().filter(|m| newer(&m.ts, &state.last_read)).collect();
    let Some(last) = unread.last() else { return Ok(None) };
    dir.channels().await?;
    Ok(Some(Item {
        key: state.id.clone(),
        kind: Kind::Dm,
        channel: state.id.clone(),
        label: String::new(),
        thread_ts: None,
        ts: last.ts.clone(),
        unread,
        priority: None,
    }))
}

async fn mention_items(slack: &Slack, dir: &mut Directory, state: &ReadState, me: &str) -> Result<Vec<Item>> {
    dir.channels().await?;
    let recent = slack.history(&state.id, 100, Some(&state.last_read)).await?;
    let mentions: Vec<Message> = recent.into_iter().filter(|m| newer(&m.ts, &state.last_read) && mentions_me(&m.text, me)).collect();
    Ok(mentions
        .into_iter()
        .rev()
        .take(state.mention_count as usize)
        .map(|m| Item {
            key: format!("{}/{}", state.id, m.ts),
            kind: Kind::Mention,
            channel: state.id.clone(),
            label: String::new(),
            thread_ts: m.thread_ts.clone(),
            ts: m.ts.clone(),
            unread: vec![m],
            priority: None,
        })
        .collect())
}

async fn thread_items(slack: &Slack, dir: &mut Directory) -> Result<Vec<Item>> {
    let Ok(threads) = slack.thread_view(50).await else { return Ok(vec![]) };
    let mut items = Vec::new();
    for t in threads.into_iter().filter(|t| newer(&t.root_msg.latest_reply, &t.root_msg.last_read)) {
        let root = t.root_msg;
        let mut unread: Vec<Message> = t.latest_replies.into_iter().filter(|m| newer(&m.ts, &root.last_read)).collect();
        if unread.is_empty() {
            unread = slack
                .replies(&root.channel, &root.ts)
                .await?
                .into_iter()
                .filter(|m| newer(&m.ts, &root.last_read) && m.ts != root.ts)
                .collect();
        }
        let Some(last) = unread.last() else { continue };
        dir.channels().await?;
        items.push(Item {
            key: format!("{}/{}", root.channel, root.ts),
            kind: Kind::Thread,
            channel: root.channel.clone(),
            label: String::new(),
            thread_ts: Some(root.ts.clone()),
            ts: last.ts.clone(),
            unread,
            priority: None,
        });
    }
    Ok(items)
}

/// Tells Slack the item was read, so the badge clears everywhere.
pub async fn mark_read(slack: &Slack, item: &Item) -> Result<()> {
    match (&item.kind, &item.thread_ts) {
        (Kind::Thread, Some(root)) => slack.mark_thread_read(&item.channel, root, &item.ts).await,
        _ => slack.mark_read(&item.channel, &item.ts).await,
    }
}

/// Priorities for `items`: cached ones reused, the rest asked in parallel and remembered.
pub async fn prioritize(judge: &impl Judge, cache: &Cache, items: &[Item], names: &NameBook, me: &str) -> Result<Verdicts, Unavailable> {
    let known: Verdicts = cache.load(VERDICTS_CACHE).unwrap_or_default();
    let verdicts = judge_all(judge, items, &known, names, me).await?;
    let _ = cache.save(VERDICTS_CACHE, &verdicts);
    Ok(verdicts)
}

async fn judge_all(judge: &impl Judge, items: &[Item], known: &Verdicts, names: &NameBook, me: &str) -> Result<Verdicts, Unavailable> {
    let (cached, fresh): (Vec<&Item>, Vec<&Item>) = items.iter().partition(|i| known.contains_key(&i.verdict_key()));
    let asked = futures_util::future::try_join_all(fresh.into_iter().map(|item| judge_one(judge, item, names, me))).await?;
    let reused = cached.into_iter().map(|i| (i.verdict_key(), known[&i.verdict_key()]));
    Ok(reused.chain(asked).collect())
}

async fn judge_one(judge: &impl Judge, item: &Item, names: &NameBook, me: &str) -> Result<(String, Priority), Unavailable> {
    let answers = judge.ask(&priority_state(item, names, me), &PRIORITY_QUESTIONS).await?;
    let priority = Priority { needs_reply: answers.noul("needs_reply")? >= 0.5, urgency: Urgency::from_score(answers.score("urgency")?) };
    Ok((item.verdict_key(), priority))
}

fn priority_state(item: &Item, names: &NameBook, me: &str) -> Value {
    let latest = &item.unread[item.unread.len().saturating_sub(STATE_MESSAGES)..];
    let unread: Vec<Value> = latest
        .iter()
        .map(|m| {
            let from = m.user.as_deref().map(|u| names.user_label(u)).or_else(|| m.username.clone()).unwrap_or_else(|| "bot".into());
            let text: String = mrkdwn::plain(&m.text, names).chars().take(STATE_TEXT_CHARS).collect();
            json!({"from": from, "text": text})
        })
        .collect();
    json!({"me": me, "conversation": item.label, "kind": item.kind, "unread": unread})
}

/// Waiting on you first, then the most urgent; ties keep their order, newest first.
pub fn rank(items: Vec<Item>, verdicts: &Verdicts) -> Vec<Item> {
    let mut ranked: Vec<Item> = items.into_iter().map(|i| Item { priority: verdicts.get(&i.verdict_key()).copied(), ..i }).collect();
    ranked.sort_by_key(|i| Reverse(i.priority.map(|p| (p.needs_reply, p.urgency))));
    ranked
}

pub fn local_now() -> DateTime<Local> {
    Local::now()
}

pub fn at(secs: i64) -> DateTime<Local> {
    Local.timestamp_opt(secs, 0).single().unwrap_or_else(Local::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Credentials;
    use crate::cache::Cache;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn item(key: &str, ts: &str) -> Item {
        Item {
            key: key.into(),
            kind: Kind::Dm,
            channel: "D1".into(),
            label: String::new(),
            thread_ts: None,
            ts: ts.into(),
            unread: vec![],
            priority: None,
        }
    }

    fn said(key: &str, text: &str) -> Item {
        Item {
            unread: vec![Message { ts: "5".into(), user: Some("U2".into()), text: text.into(), ..Default::default() }],
            ..item(key, key)
        }
    }

    fn priority(needs_reply: bool, urgency: Urgency) -> Priority {
        Priority { needs_reply, urgency }
    }

    /// Reads the verdict off the text itself: `reply` and `urgent`/`soon` are the only words that count.
    fn keyword_judge(state: &Value) -> Result<Value, Unavailable> {
        let text = state["unread"][0]["text"].as_str().unwrap_or_default();
        let score = if text.contains("urgent") {
            2.0
        } else if text.contains("soon") {
            1.2
        } else {
            0.1
        };
        let noul = if text.contains("reply") { 0.9 } else { 0.1 };
        Ok(json!({"urgency": {"score": score}, "needs_reply": {"noul": noul}}))
    }

    #[test]
    fn rank_puts_replies_first_then_urgency_and_keeps_ties_in_order() {
        let items = vec![item("a", "a"), item("b", "b"), item("c", "c"), item("d", "d"), item("e", "e")];
        let verdicts: Verdicts = [
            ("D1/a".into(), priority(false, Urgency::High)),
            ("D1/b".into(), priority(true, Urgency::Low)),
            ("D1/c".into(), priority(false, Urgency::Low)),
            ("D1/e".into(), priority(false, Urgency::High)),
        ]
        .into();
        let ranked = rank(items, &verdicts);
        assert_eq!(ranked.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["b", "a", "e", "c", "d"]);
        assert_eq!(ranked[0].priority, Some(priority(true, Urgency::Low)));
        assert_eq!(ranked[4].priority, None);
    }

    #[tokio::test]
    async fn prioritize_asks_only_about_new_messages_and_remembers_them() {
        use crate::typesafe::stub::Stub;
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());
        let items = vec![said("a", "urgent, please reply"), said("b", "lunch soon?"), said("c", "fyi")];
        let verdicts = prioritize(&Stub(keyword_judge), &cache, &items, &NameBook::default(), "me").await.unwrap();
        assert_eq!(verdicts["D1/a"], priority(true, Urgency::High));
        assert_eq!(verdicts["D1/b"], priority(false, Urgency::Medium));
        assert_eq!(verdicts["D1/c"], priority(false, Urgency::Low));
        let unreachable = Stub(|_| Err(Unavailable("asked again".into())));
        let again = prioritize(&unreachable, &cache, &items, &NameBook::default(), "me").await.unwrap();
        assert_eq!(again, verdicts, "cached verdicts are reused, nothing is asked");
        let newer = vec![said("d", "new")];
        let error = prioritize(&unreachable, &cache, &newer, &NameBook::default(), "me").await.unwrap_err();
        assert_eq!(error, Unavailable("asked again".into()));
    }

    #[test]
    fn priority_state_carries_the_latest_unread_as_plain_text() {
        let item = Item { label: "#ops".into(), ..said("a", "*deploy* <https://x.io|failed>") };
        let state = priority_state(&item, &NameBook::default(), "vivien");
        assert_eq!(
            state,
            json!({"me": "vivien", "conversation": "#ops", "kind": "Dm", "unread": [{"from": "U2", "text": "deploy failed (https://x.io)"}]})
        );
    }

    #[test]
    fn snoozed_items_hide_until_due() {
        let mut state = State::default();
        let now = at(1_000_000);
        state.snooze("a", now + chrono::Duration::hours(1));
        let items = vec![item("a", "5"), item("b", "5")];
        assert_eq!(state.visible(items.clone(), now).len(), 1);
        assert_eq!(state.visible(items, now + chrono::Duration::hours(2)).len(), 2);
    }

    #[test]
    fn read_items_hide_until_something_newer() {
        let mut state = State::default();
        state.mark_read("a", "5");
        assert!(state.visible(vec![item("a", "5")], at(0)).is_empty());
        assert_eq!(state.visible(vec![item("a", "6")], at(0)).len(), 1);
    }

    #[test]
    fn marking_read_clears_a_snooze_and_expiry_prunes() {
        let mut state = State::default();
        state.snooze("a", at(10));
        state.mark_read("a", "1");
        assert!(state.snoozed.is_empty());
        state.snooze("b", at(10));
        state.forget_expired(at(11));
        assert!(state.snoozed.is_empty());
    }

    #[test]
    fn snooze_presets() {
        let now = Local.with_ymd_and_hms(2026, 9, 16, 15, 30, 0).unwrap();
        assert_eq!(Snooze::OneHour.until(now).hour(), 16);
        assert_eq!(Snooze::Tomorrow.until(now).format("%d %H:%M").to_string(), "17 09:00");
        assert_eq!(Snooze::NextWeek.until(now).weekday(), chrono::Weekday::Mon);
        assert_eq!(Snooze::NextWeek.until(now).day(), 21);
        let monday = Local.with_ymd_and_hms(2026, 9, 21, 15, 30, 0).unwrap();
        assert_eq!(Snooze::NextWeek.until(monday).day(), 28);
    }

    #[test]
    fn state_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("SLACK_CLI_STATE_DIR", dir.path()) };
        let mut state = State::default();
        state.snooze("a", at(99));
        state.save("acme").unwrap();
        assert_eq!(State::load("acme"), state);
        assert_eq!(State::load("other"), State::default());
    }

    #[test]
    fn mention_detection() {
        assert!(mentions_me("hey <@U1> look", "U1"));
        assert!(mentions_me("<!here> deploy", "U1"));
        assert!(mentions_me("<!subteam^S1|@team>", "U1"));
        assert!(!mentions_me("hey <@U2>", "U1"));
        assert!(!mentions_me("plain", "U1"));
    }

    #[test]
    fn reply_targets() {
        let dm = item("D1", "1");
        assert_eq!(dm.reply_thread(), None);
        let mut mention = Item { kind: Kind::Mention, ..item("C1/2", "2") };
        mention.unread = vec![Message { ts: "2".into(), ..Default::default() }];
        assert_eq!(mention.reply_thread().as_deref(), Some("2"));
        mention.thread_ts = Some("1".into());
        assert_eq!(mention.reply_thread().as_deref(), Some("1"));
    }

    fn ok(body: serde_json::Value) -> ResponseTemplate {
        let mut body = body;
        body["ok"] = json!(true);
        ResponseTemplate::new(200).set_body_json(body)
    }

    #[tokio::test]
    async fn fetch_collects_dms_mentions_and_threads() {
        let server = MockServer::start().await;
        Mock::given(path("/client.counts"))
            .respond_with(ok(json!({
                "channels": [{"id": "C1", "last_read": "10.0", "mention_count": 1, "has_unreads": true}, {"id": "C2", "last_read": "1.0", "mention_count": 0, "has_unreads": true}],
                "ims": [{"id": "D1", "last_read": "5.0", "has_unreads": true}, {"id": "D2", "last_read": "5.0", "has_unreads": false}],
                "mpims": []
            })))
            .mount(&server)
            .await;
        Mock::given(path("/conversations.history"))
            .and(body_string_contains("channel=D1"))
            .respond_with(ok(json!({"messages": [{"ts": "7.0", "user": "U2", "text": "yo"}, {"ts": "6.0", "user": "U2", "text": "hi"}]})))
            .mount(&server)
            .await;
        Mock::given(path("/conversations.history"))
            .and(body_string_contains("channel=C1"))
            .respond_with(ok(
                json!({"messages": [{"ts": "12.0", "user": "U2", "text": "<@U1> ping"}, {"ts": "11.0", "user": "U2", "text": "noise"}]}),
            ))
            .mount(&server)
            .await;
        Mock::given(path("/subscriptions.thread.getView"))
            .respond_with(ok(json!({"threads": [
                {"root_msg": {"channel": "C2", "ts": "1.0", "text": "root", "last_read": "2.0", "latest_reply": "3.0"}, "latest_replies": [{"ts": "3.0", "user": "U2", "text": "reply"}]},
                {"root_msg": {"channel": "C2", "ts": "4.0", "text": "seen", "last_read": "9.0", "latest_reply": "9.0"}, "latest_replies": []}
            ]})))
            .mount(&server)
            .await;
        Mock::given(path("/conversations.list"))
            .respond_with(ok(json!({"channels": [{"id": "C1", "name": "ops"}, {"id": "D1", "is_im": true, "user": "U2"}]})))
            .mount(&server)
            .await;
        Mock::given(path("/users.list")).respond_with(ok(json!({"members": [{"id": "U2", "name": "bob"}]}))).mount(&server).await;
        let slack = Slack::new(&server.uri(), Credentials::new("t", None)).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mut dir = Directory::new(slack.clone(), Cache::new(tmp.keep()));
        let items = fetch(&slack, &mut dir, "U1").await.unwrap();
        let summary: Vec<(Kind, &str, &str, usize)> =
            items.iter().map(|i| (i.kind, i.key.as_str(), i.label.as_str(), i.unread.len())).collect();
        assert_eq!(summary, vec![(Kind::Mention, "C1/12.0", "#ops", 1), (Kind::Dm, "D1", "@bob", 2), (Kind::Thread, "C2/1.0", "C2", 1)]);
    }

    #[tokio::test]
    async fn fetch_survives_a_missing_thread_endpoint() {
        let server = MockServer::start().await;
        Mock::given(path("/client.counts")).respond_with(ok(json!({"channels": [], "ims": [], "mpims": []}))).mount(&server).await;
        Mock::given(path("/subscriptions.thread.getView"))
            .respond_with(ok(json!({"ok": false, "error": "unknown_method"})))
            .mount(&server)
            .await;
        let slack = Slack::new(&server.uri(), Credentials::new("t", None)).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mut dir = Directory::new(slack.clone(), Cache::new(tmp.keep()));
        assert!(fetch(&slack, &mut dir, "U1").await.unwrap().is_empty());
    }
}
