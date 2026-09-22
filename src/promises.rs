//! Follow-ups you promised in your own messages, and whether a later reply of yours closed them.
use crate::api::{SearchMatch, Slack};
use crate::cache::Cache;
use crate::inbox::newer;
use crate::mrkdwn;
use crate::permalink;
use crate::render::time;
use crate::resolve::NameBook;
use crate::typesafe::{Judge, Question, Unavailable};
use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const DEFAULT_SINCE: &str = "14d";

/// Jev's yes or no, by the message it was asked about, so a re-run only asks about new ones.
type Verdicts = BTreeMap<String, bool>;

const VERDICTS_CACHE: &str = "promises";
const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 5;
const STATE_TEXT_CHARS: usize = 2000;
const PROMISE_THRESHOLD: f64 = 0.7;
const FULFILS_THRESHOLD: f64 = 0.5;
const PROMISE_QUESTION: Question = Question::Noul(
    "Does `me` promise in this Slack message to do something later, like \"I'll look into it\", \"will send it tomorrow\" or \
     \"let me check and come back to you\"? A question, an explanation, a plan for someone else, or work already done in the \
     message is no promise.",
);
const FULFILS_QUESTION: Question = Question::Noul(
    "`me` wrote the `promise` earlier in this thread. Does the `later` message by `me` close it: sharing the result, saying it \
     is done, or saying it is dropped?",
);

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Promise {
    #[serde(flatten)]
    pub message: SearchMatch,
    /// A later message of yours in the same thread fulfils it.
    pub closed: bool,
}

/// Your own messages since `oldest` (a Slack ts), newest first, up to `MAX_PAGES` of search results.
pub async fn sent_since(slack: &Slack, oldest: &str) -> Result<Vec<SearchMatch>> {
    let query = format!("from:me after:{}", day_before(oldest));
    let mut sent = Vec::new();
    for page in 1..=MAX_PAGES {
        let matches = slack.search_page(&query, PAGE_SIZE, page).await?.matches;
        let last = matches.len() < PAGE_SIZE;
        sent.extend(matches.into_iter().filter(|m| !newer(oldest, &m.ts)));
        if last {
            break;
        }
    }
    Ok(sent)
}

/// Slack's `after:` skips the day it names.
fn day_before(ts: &str) -> String {
    time::parse_ts(ts).map(|d| (d - chrono::Duration::days(1)).format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// Every promise in `sent`, closed or not: cached verdicts reused, the rest asked in parallel and remembered.
pub async fn track(
    judge: &impl Judge,
    cache: &Cache,
    sent: &[SearchMatch],
    names: &NameBook,
    me: &str,
) -> Result<Vec<Promise>, Unavailable> {
    let known: Verdicts = cache.load(VERDICTS_CACHE).unwrap_or_default();
    let promise_asks = sent.iter().map(|m| (message_key(m), promise_state(m, names, me))).collect();
    let promised = judge_all(judge, &known, promise_asks, PROMISE_QUESTION, PROMISE_THRESHOLD).await?;
    let promises: Vec<&SearchMatch> = sent.iter().filter(|m| promised[&message_key(m)]).collect();
    let fulfils_asks = promises
        .iter()
        .flat_map(|p| later_in_thread(p, sent).map(move |later| (fulfils_key(p, later), fulfils_state(p, later, names, me))))
        .collect();
    let fulfilled = judge_all(judge, &known, fulfils_asks, FULFILS_QUESTION, FULFILS_THRESHOLD).await?;
    let closed = |p: &SearchMatch| later_in_thread(p, sent).any(|later| fulfilled[&fulfils_key(p, later)]);
    let tracked = promises.iter().map(|p| Promise { message: (*p).clone(), closed: closed(p) }).collect();
    let _ = cache.save(VERDICTS_CACHE, &promised.into_iter().chain(fulfilled).collect::<Verdicts>());
    Ok(tracked)
}

async fn judge_all(
    judge: &impl Judge,
    known: &Verdicts,
    asks: Vec<(String, Value)>,
    question: Question,
    threshold: f64,
) -> Result<Verdicts, Unavailable> {
    let (cached, fresh): (Vec<_>, Vec<_>) = asks.into_iter().partition(|(key, _)| known.contains_key(key));
    let asked = futures_util::future::try_join_all(fresh.into_iter().map(|(key, state)| async move {
        let answers = judge.ask(&state, &[("yes", question)]).await?;
        Ok::<_, Unavailable>((key, answers.noul("yes")? >= threshold))
    }))
    .await?;
    let reused = cached.into_iter().map(|(key, _)| {
        let verdict = known[&key];
        (key, verdict)
    });
    Ok(reused.chain(asked).collect())
}

/// Your later messages in the promise's thread, the only ones cheap enough to check.
fn later_in_thread<'a>(promise: &'a SearchMatch, sent: &'a [SearchMatch]) -> impl Iterator<Item = &'a SearchMatch> {
    let root = thread_root(promise);
    sent.iter().filter(move |m| m.channel.id == promise.channel.id && thread_root(m) == root && newer(&m.ts, &promise.ts))
}

fn thread_root(m: &SearchMatch) -> String {
    permalink::parse(&m.permalink).map(|r| r.thread_root().to_owned()).unwrap_or_else(|_| m.ts.clone())
}

fn message_key(m: &SearchMatch) -> String {
    format!("{}/{}", m.channel.id, m.ts)
}

fn fulfils_key(promise: &SearchMatch, later: &SearchMatch) -> String {
    format!("{}>{}", message_key(promise), later.ts)
}

fn plain_text(m: &SearchMatch, names: &NameBook) -> String {
    mrkdwn::plain(&m.text, names).chars().take(STATE_TEXT_CHARS).collect()
}

fn promise_state(m: &SearchMatch, names: &NameBook, me: &str) -> Value {
    json!({"me": me, "conversation": names.channel_label(&m.channel.id), "text": plain_text(m, names)})
}

fn fulfils_state(promise: &SearchMatch, later: &SearchMatch, names: &NameBook, me: &str) -> Value {
    let conversation = names.channel_label(&promise.channel.id);
    json!({"me": me, "conversation": conversation, "promise": plain_text(promise, names), "later": plain_text(later, names)})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::SearchChannel;
    use crate::auth::Credentials;
    use crate::typesafe::stub::Stub;
    use wiremock::matchers::{body_string_contains, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ROOT: &str = "1758000000.000100";

    fn sent(ts: &str, text: &str, thread: Option<&str>) -> SearchMatch {
        let link = format!("https://acme.slack.com/archives/C1/p{}", ts.replace('.', ""));
        SearchMatch {
            ts: ts.into(),
            text: text.into(),
            user: "U1".into(),
            permalink: thread.map_or(link.clone(), |root| format!("{link}?thread_ts={root}")),
            channel: SearchChannel { id: "C1".into(), name: "ops".into() },
            ..Default::default()
        }
    }

    /// `I'll` makes a promise, `done` fulfils one; nothing else counts.
    fn keyword_judge(state: &Value) -> Result<Value, Unavailable> {
        let noul = match (state["text"].as_str(), state["later"].as_str()) {
            (Some(text), _) if text.contains("I'll") => 0.9,
            (_, Some(later)) if later.contains("done") => 0.8,
            _ => 0.1,
        };
        Ok(json!({"yes": {"noul": noul}}))
    }

    fn tracked(promises: &[Promise]) -> Vec<(&str, bool)> {
        promises.iter().map(|p| (p.message.text.as_str(), p.closed)).collect()
    }

    #[tokio::test]
    async fn a_later_reply_in_the_thread_closes_a_promise() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());
        let messages = vec![
            sent("1758000300.000000", "done, see the PR", Some(ROOT)),
            sent("1758000200.000000", "I'll send the numbers tomorrow", None),
            sent("1758000100.000000", "I'll look into the flaky test", Some(ROOT)),
            sent(ROOT, "anyone seen this?", None),
        ];
        let promises = track(&Stub(keyword_judge), &cache, &messages, &NameBook::default(), "vivien").await.unwrap();
        assert_eq!(tracked(&promises), [("I'll send the numbers tomorrow", false), ("I'll look into the flaky test", true)]);
    }

    #[tokio::test]
    async fn verdicts_are_cached_so_only_new_messages_are_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());
        let first = vec![sent("1758000100.000000", "I'll look into it", Some(ROOT))];
        let promises = track(&Stub(keyword_judge), &cache, &first, &NameBook::default(), "vivien").await.unwrap();
        let unreachable = Stub(|_| Err(Unavailable("asked again".into())));
        let again = track(&unreachable, &cache, &first, &NameBook::default(), "vivien").await.unwrap();
        assert_eq!(again, promises, "cached verdicts are reused, nothing is asked");
        let with_reply = [vec![sent("1758000300.000000", "done", Some(ROOT))], first].concat();
        let error = track(&unreachable, &cache, &with_reply, &NameBook::default(), "vivien").await.unwrap_err();
        assert_eq!(error, Unavailable("asked again".into()));
    }

    #[test]
    fn promise_state_carries_the_plain_text() {
        let state = promise_state(&sent(ROOT, "*will* send <https://x.io|it>", None), &NameBook::default(), "vivien");
        assert_eq!(state, json!({"me": "vivien", "conversation": "C1", "text": "will send it (https://x.io)"}));
    }

    fn ok(body: Value) -> ResponseTemplate {
        let mut body = body;
        body["ok"] = json!(true);
        ResponseTemplate::new(200).set_body_json(body)
    }

    #[tokio::test]
    async fn sent_since_reads_every_page_of_my_messages_and_drops_older_ones() {
        let server = MockServer::start().await;
        let full_page: Vec<Value> = (0..PAGE_SIZE).map(|i| json!({"ts": format!("1758000{i:03}.000000"), "text": "recent"})).collect();
        Mock::given(path("/search.messages"))
            .and(body_string_contains("from%3Ame"))
            .and(body_string_contains("page=1&"))
            .respond_with(ok(json!({"messages": {"total": 102, "matches": full_page}})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/search.messages"))
            .and(body_string_contains("page=2&"))
            .respond_with(ok(json!({"messages": {"total": 102, "matches": [
                {"ts": "1758000000.000000", "text": "oldest in window"},
                {"ts": "1757000000.000000", "text": "too old"}
            ]}})))
            .expect(1)
            .mount(&server)
            .await;
        let slack = Slack::new(&server.uri(), Credentials::new("t", None)).unwrap();
        let messages = sent_since(&slack, "1758000000.000000").await.unwrap();
        assert_eq!(messages.len(), PAGE_SIZE + 1);
        assert_eq!(messages.last().map(|m| m.text.as_str()), Some("oldest in window"));
    }
}
