use anyhow::{Result, bail};
use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageRef {
    pub channel: String,
    pub ts: String,
    pub thread_ts: Option<String>,
}

impl MessageRef {
    /// The thread this message belongs to: its parent when it is a reply, itself otherwise.
    pub fn thread_root(&self) -> &str {
        self.thread_ts.as_deref().unwrap_or(&self.ts)
    }
}

static PERMALINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https://[^/]+/archives/([A-Z0-9]+)/p(\d{16,})(?:\?(.*))?$").unwrap());
static TS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{10}\.\d{6}$").unwrap());

pub fn is_permalink(s: &str) -> bool {
    PERMALINK.is_match(s)
}

pub fn is_ts(s: &str) -> bool {
    TS.is_match(s)
}

pub fn parse(url: &str) -> Result<MessageRef> {
    let Some(caps) = PERMALINK.captures(url.trim()) else {
        bail!("not a Slack message permalink: {url}");
    };
    let digits = &caps[2];
    let ts = format!("{}.{}", &digits[..10], &digits[10..]);
    let thread_ts = caps.get(3).and_then(|q| query_value(q.as_str(), "thread_ts")).filter(|t| t != &ts);
    Ok(MessageRef { channel: caps[1].to_owned(), ts, thread_ts })
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| v.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_permalink() {
        let r = parse("https://acme.slack.com/archives/C0123ABC/p1694700000123456").unwrap();
        assert_eq!(r, MessageRef { channel: "C0123ABC".into(), ts: "1694700000.123456".into(), thread_ts: None });
        assert_eq!(r.thread_root(), "1694700000.123456");
    }

    #[test]
    fn parses_reply_permalink() {
        let r = parse("https://acme.slack.com/archives/C0123ABC/p1694700001000000?thread_ts=1694700000.123456&cid=C0123ABC").unwrap();
        assert_eq!(r.thread_ts.as_deref(), Some("1694700000.123456"));
        assert_eq!(r.thread_root(), "1694700000.123456");
    }

    #[test]
    fn root_permalink_with_own_thread_ts_has_no_parent() {
        let r = parse("https://acme.slack.com/archives/C1/p1694700000123456?thread_ts=1694700000.123456").unwrap();
        assert_eq!(r.thread_ts, None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("https://acme.slack.com/messages/C1").is_err());
        assert!(!is_permalink("hello"));
        assert!(is_ts("1694700000.123456"));
        assert!(!is_ts("1694700000"));
    }
}
