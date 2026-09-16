//! One line per message across every conversation, like tailing a log.
use crate::api::Message;
use crate::api::rtm::Event;
use crate::mrkdwn::{self, Names, Segment};
use anyhow::{Context, Result};
use regex::{Regex, RegexBuilder};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Line {
    pub ts: String,
    pub channel: String,
    pub user: Option<String>,
    pub username: Option<String>,
    pub text: String,
    pub in_thread: bool,
    pub thread_ts: Option<String>,
}

impl Line {
    pub fn from_event(event: &Event) -> Option<Line> {
        let Event::Message { channel, message } = event else { return None };
        Some(Line::from_message(channel, message))
    }

    pub fn from_message(channel: &str, m: &Message) -> Line {
        Line {
            ts: m.ts.clone(),
            channel: channel.to_owned(),
            user: m.user.clone(),
            username: m.username.clone(),
            text: m.text.clone(),
            in_thread: m.is_reply(),
            thread_ts: m.thread_ts.clone(),
        }
    }

    /// The text as one line, mentions decoded and links reduced to their label.
    pub fn flat_text(&self, names: &dyn Names) -> String {
        let flat: String = mrkdwn::parse(&self.text, names)
            .into_iter()
            .map(|s| match s {
                Segment::Text(t) | Segment::Bold(t) | Segment::Italic(t) | Segment::Strike(t) | Segment::Code(t) | Segment::Pre(t) => t,
                Segment::Link { label, .. } => label,
                Segment::Mention(n) => format!("@{n}"),
                Segment::Channel(n) => format!("#{n}"),
                Segment::Emoji(n) => crate::emoji::render(&n),
            })
            .collect();
        flat.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// Case-insensitive patterns that light up a line, from config and `--highlight`.
#[derive(Clone, Debug, Default)]
pub struct Highlighter {
    patterns: Vec<Regex>,
}

impl Highlighter {
    pub fn new(patterns: &[String]) -> Result<Self> {
        let patterns = patterns
            .iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| RegexBuilder::new(p).case_insensitive(true).build().with_context(|| format!("bad highlight regex `{p}`")))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { patterns })
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn hits(&self, text: &str) -> bool {
        self.patterns.iter().any(|p| p.is_match(text))
    }

    /// The text cut into `(piece, highlighted)` runs, in order.
    pub fn split(&self, text: &str) -> Vec<(String, bool)> {
        let mut marks = vec![false; text.len()];
        for p in &self.patterns {
            for m in p.find_iter(text) {
                marks[m.start()..m.end()].iter_mut().for_each(|b| *b = true);
            }
        }
        let mut runs: Vec<(String, bool)> = Vec::new();
        for (i, c) in text.char_indices() {
            let hit = marks[i];
            match runs.last_mut() {
                Some((run, h)) if *h == hit => run.push(c),
                _ => runs.push((c.to_string(), hit)),
            }
        }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighter_splits_case_insensitively() {
        let h = Highlighter::new(&["prod".into(), "error|failed".into()]).unwrap();
        assert!(h.hits("Deploy FAILED on prod"));
        assert!(!h.hits("all good"));
        assert_eq!(
            h.split("Deploy FAILED on prod!"),
            vec![
                ("Deploy ".to_owned(), false),
                ("FAILED".to_owned(), true),
                (" on ".to_owned(), false),
                ("prod".to_owned(), true),
                ("!".to_owned(), false)
            ]
        );
        assert_eq!(h.split("émoji prod"), vec![("émoji ".to_owned(), false), ("prod".to_owned(), true)]);
    }

    #[test]
    fn bad_regex_is_reported_and_blank_ignored() {
        assert!(Highlighter::new(&["(".into()]).unwrap_err().to_string().contains("bad highlight regex"));
        assert!(Highlighter::new(&["  ".into()]).unwrap().is_empty());
    }

    #[test]
    fn line_from_event_flattens_text() {
        let m = Message {
            ts: "1".into(),
            user: Some("U1".into()),
            text: "hello\n  <https://a.io|world>".into(),
            thread_ts: Some("0".into()),
            ..Default::default()
        };
        let line = Line::from_event(&Event::Message { channel: "C1".into(), message: m }).unwrap();
        assert!(line.in_thread);
        assert_eq!(line.flat_text(&mrkdwn::NoNames), "hello world");
        assert!(Line::from_event(&Event::Connected).is_none());
    }
}
