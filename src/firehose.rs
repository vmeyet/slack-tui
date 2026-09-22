//! One line per message across every conversation, like tailing a log.
use crate::api::Message;
use crate::api::rtm::Event;
use crate::mrkdwn::{self, Names, Segment};
use crate::resolve::NameBook;
use crate::typesafe::{Judge, Question, Unavailable};
use anyhow::{Context, Result};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const STATE_TEXT_CHARS: usize = 2000;
const TAG_QUESTION: [(&str, Question); 1] = [(
    "tag",
    Question::Choice(
        "How should `me` treat this live Slack message?",
        &[
            ("incident", "Something is broken or on fire: an outage, failed deploy, alert or escalation."),
            ("question-for-me", "A question or request aimed at `me`, by name or by a clear role."),
            ("fyi", "Worth knowing but asks nothing: news, updates, decisions."),
            ("noise", "Chatter, bots and routine notices nobody needs to read."),
        ],
    ),
)];

/// What Jev made of a live message; `noise` hides by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tag {
    Incident,
    QuestionForMe,
    Fyi,
    Noise,
}

impl Tag {
    const ALL: [Tag; 4] = [Tag::Incident, Tag::QuestionForMe, Tag::Fyi, Tag::Noise];

    pub fn label(self) -> &'static str {
        match self {
            Tag::Incident => "incident",
            Tag::QuestionForMe => "question-for-me",
            Tag::Fyi => "fyi",
            Tag::Noise => "noise",
        }
    }
}

pub async fn classify(judge: &impl Judge, state: &Value) -> Result<Tag, Unavailable> {
    let answers = judge.ask(state, &TAG_QUESTION).await?;
    let name = answers.choice("tag")?;
    Tag::ALL.into_iter().find(|t| t.label() == name).ok_or_else(|| Unavailable(format!("unknown tag `{name}`")))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Line {
    pub ts: String,
    pub channel: String,
    pub user: Option<String>,
    pub username: Option<String>,
    pub text: String,
    pub in_thread: bool,
    pub thread_ts: Option<String>,
    /// Arrives after the line, once Jev answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Tag>,
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
            tag: None,
        }
    }

    pub fn author(&self, names: &NameBook) -> String {
        self.user.as_deref().map(|u| names.user_label(u)).or_else(|| self.username.clone()).unwrap_or_else(|| "bot".into())
    }

    pub fn is_noise(&self) -> bool {
        self.tag == Some(Tag::Noise)
    }

    /// What Jev reads to tag the line: who `me` is, and who said what where.
    pub fn tag_state(&self, names: &NameBook, me: &str) -> Value {
        let text: String = self.flat_text(names).chars().take(STATE_TEXT_CHARS).collect();
        let channel = names.channel_label(&self.channel);
        json!({"me": me, "channel": channel, "author": self.author(names), "in_thread": self.in_thread, "text": text})
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

    #[tokio::test]
    async fn classify_reads_the_chosen_tag() {
        use crate::typesafe::stub::Stub;
        let pick = |state: &Value| {
            let choice = if state["text"].as_str().unwrap_or_default().contains("down") { "incident" } else { "noise" };
            Ok(json!({"tag": {"choice": choice}}))
        };
        let line = Line { text: "prod is down".into(), ..Line::from_message("C1", &Message { ts: "1".into(), ..Default::default() }) };
        let state = line.tag_state(&NameBook::default(), "vivien");
        assert_eq!(state, json!({"me": "vivien", "channel": "C1", "author": "bot", "in_thread": false, "text": "prod is down"}));
        assert_eq!(classify(&Stub(pick), &state).await, Ok(Tag::Incident));
        assert_eq!(classify(&Stub(pick), &json!({"text": "lunch"})).await, Ok(Tag::Noise));
        let odd = Stub(|_| Ok(json!({"tag": {"choice": "spam"}})));
        assert_eq!(classify(&odd, &state).await, Err(Unavailable("unknown tag `spam`".into())));
        assert_eq!(serde_json::to_value(Tag::QuestionForMe).unwrap(), json!("question-for-me"));
    }
}
