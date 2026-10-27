//! The `:` command line: typed verbs with fuzzy tab completion and history.
use crate::inbox::Snooze;
use vtui::fuzzy;
use vtui::palette::Verb;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Join(String),
    Leave(Option<String>),
    Go(String),
    Msg { target: String, text: String },
    Compose,
    React(String),
    Edit,
    Delete,
    Thread,
    Search(String),
    Open,
    Copy,
    Export(Format),
    Read,
    Snooze(Snooze),
    Set { key: String, value: String },
    Help,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Markdown,
}

pub const VERBS: [Verb; 18] = [
    Verb { word: "join", does: "join a channel: :join #ops" },
    Verb { word: "leave", does: "leave the current channel, or :leave #ops" },
    Verb { word: "go", does: "open a conversation: :go #ops, :go @bob" },
    Verb { word: "msg", does: "send a message: :msg @bob on my way" },
    Verb { word: "compose", does: "write the message in $EDITOR" },
    Verb { word: "react", does: "react to the selected message: :react rocket" },
    Verb { word: "edit", does: "rewrite the selected message, yours only" },
    Verb { word: "delete", does: "delete the selected message, yours only, after a yes" },
    Verb { word: "thread", does: "open the selected message's thread" },
    Verb { word: "search", does: "search Slack: :search deploy failed" },
    Verb { word: "open", does: "open the selected message in Slack" },
    Verb { word: "copy", does: "copy the selected message's permalink" },
    Verb { word: "export", does: "save the conversation: :export json | md" },
    Verb { word: "read", does: "mark the conversation read" },
    Verb { word: "snooze", does: "snooze the selected inbox item: :snooze 1h | 3h | tomorrow | monday" },
    Verb { word: "set", does: "change and save a setting: :set theme=nord · :set highlight=#2a2a2a" },
    Verb { word: "help", does: "show the keys" },
    Verb { word: "quit", does: "leave" },
];

pub fn parse(line: &str) -> Result<Command, String> {
    let line = line.trim().trim_start_matches(':').trim();
    let (verb, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(v, r)| (v, r.trim()));
    let need =
        |what: &str| -> Result<String, String> { if rest.is_empty() { Err(format!(":{verb} needs {what}")) } else { Ok(rest.to_owned()) } };
    match verb {
        "" => Err("type a command, tab completes".into()),
        "join" | "j" => Ok(Command::Join(need("a channel")?)),
        "leave" => Ok(Command::Leave((!rest.is_empty()).then(|| rest.to_owned()))),
        "go" | "g" | "c" => Ok(Command::Go(need("a channel or @person")?)),
        "msg" | "m" | "dm" => {
            let (target, text) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(t, x)| (t, x.trim()));
            if target.is_empty() || text.is_empty() {
                return Err(":msg needs a target and a message".into());
            }
            Ok(Command::Msg { target: target.to_owned(), text: text.to_owned() })
        }
        "compose" => Ok(Command::Compose),
        "react" | "r" => Ok(Command::React(need("an emoji name")?.trim_matches(':').to_owned())),
        "edit" => Ok(Command::Edit),
        "delete" | "del" => Ok(Command::Delete),
        "thread" | "t" => Ok(Command::Thread),
        "search" | "s" | "/" => Ok(Command::Search(need("a query")?)),
        "open" | "o" => Ok(Command::Open),
        "copy" | "y" => Ok(Command::Copy),
        "export" | "e" => match rest {
            "" | "json" => Ok(Command::Export(Format::Json)),
            "md" | "markdown" => Ok(Command::Export(Format::Markdown)),
            other => Err(format!(":export takes json or md, not `{other}`")),
        },
        "read" => Ok(Command::Read),
        "snooze" | "z" => match rest {
            "1h" | "1" => Ok(Command::Snooze(Snooze::OneHour)),
            "3h" | "3" => Ok(Command::Snooze(Snooze::ThreeHours)),
            "tomorrow" | "tmr" => Ok(Command::Snooze(Snooze::Tomorrow)),
            "monday" | "week" => Ok(Command::Snooze(Snooze::NextWeek)),
            _ => Err(":snooze takes 1h, 3h, tomorrow or monday".into()),
        },
        "set" => {
            let Some((key, value)) = rest.split_once('=') else { return Err(":set needs key=value, e.g. highlight=#2a2a2a".into()) };
            Ok(Command::Set { key: key.trim().to_owned(), value: value.trim().to_owned() })
        }
        "help" | "h" | "?" => Ok(Command::Help),
        "quit" | "q" | "exit" => Ok(Command::Quit),
        unknown => {
            let close = fuzzy::suggestions(unknown, VERBS.iter().map(|verb| verb.word), 3);
            if close.is_empty() {
                return Err(format!("unknown command :{unknown}"));
            }
            Err(format!("unknown command :{unknown}, did you mean :{}?", close.join(", :")))
        }
    }
}

/// What the token under the cursor can complete to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Verb,
    Conversation,
    Channel,
    Emoji,
    Literal(&'static [&'static str]),
    Free,
}

pub fn slot(line: &str) -> Slot {
    let line = line.trim_start_matches(':');
    let mut tokens = line.split_whitespace();
    let verb = tokens.next().unwrap_or("");
    let args_done = tokens.count();
    let editing_verb = !line.contains(char::is_whitespace);
    if editing_verb {
        return Slot::Verb;
    }
    let arg_index = if line.ends_with(char::is_whitespace) { args_done } else { args_done.saturating_sub(1) };
    match (verb, arg_index) {
        ("join" | "j" | "leave", 0) => Slot::Channel,
        ("go" | "g" | "c" | "msg" | "m" | "dm", 0) => Slot::Conversation,
        ("react" | "r", 0) => Slot::Emoji,
        ("export" | "e", 0) => Slot::Literal(&["json", "md"]),
        ("snooze" | "z", 0) => Slot::Literal(&["1h", "3h", "tomorrow", "monday"]),
        ("set", 0) => Slot::Literal(&["theme=", "highlight=", "images=", "show_url="]),
        _ => Slot::Free,
    }
}

/// The `:` line's only mode: everything typed is a command.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Commands;

impl vtui::palette::Mode for Commands {
    fn switched_by(self, _first: char) -> Option<Self> {
        None
    }

    fn takes_commands(self) -> bool {
        true
    }
}

pub type Palette = vtui::palette::Palette<Commands>;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn parses_verbs_and_aliases() {
        assert_eq!(parse(":join #ops"), Ok(Command::Join("#ops".into())));
        assert_eq!(parse("msg @bob on my way"), Ok(Command::Msg { target: "@bob".into(), text: "on my way".into() }));
        assert_eq!(parse("r :rocket:"), Ok(Command::React("rocket".into())));
        assert_eq!(parse("export md"), Ok(Command::Export(Format::Markdown)));
        assert_eq!(parse("export"), Ok(Command::Export(Format::Json)));
        assert_eq!(parse("snooze tomorrow"), Ok(Command::Snooze(Snooze::Tomorrow)));
        assert_eq!(parse("set highlight=#2a2a2a"), Ok(Command::Set { key: "highlight".into(), value: "#2a2a2a".into() }));
        assert_eq!(parse("leave"), Ok(Command::Leave(None)));
        assert_eq!(parse("q"), Ok(Command::Quit));
    }

    #[test]
    fn errors_are_helpful() {
        assert_eq!(parse("msg @bob"), Err(":msg needs a target and a message".into()));
        assert_eq!(parse("join"), Err(":join needs a channel".into()));
        assert!(parse("jion #x").unwrap_err().contains("did you mean :join"));
        assert_eq!(parse("export pdf"), Err(":export takes json or md, not `pdf`".into()));
    }

    #[test]
    fn slots_follow_the_verb() {
        assert_eq!(slot("jo"), Slot::Verb);
        assert_eq!(slot("join "), Slot::Channel);
        assert_eq!(slot("join #ge"), Slot::Channel);
        assert_eq!(slot("msg @b"), Slot::Conversation);
        assert_eq!(slot("msg @bob hi"), Slot::Free);
        assert_eq!(slot("react ro"), Slot::Emoji);
        assert_eq!(slot("export "), Slot::Literal(&["json", "md"]));
    }
}
