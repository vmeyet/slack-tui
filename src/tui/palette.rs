//! The `:` command line: typed verbs with fuzzy tab completion and history.
use crate::fuzzy;
use crate::inbox::Snooze;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Join(String),
    Leave(Option<String>),
    Go(String),
    Msg { target: String, text: String },
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

pub const VERBS: [(&str, &str); 17] = [
    ("join", "join a channel: :join #ops"),
    ("leave", "leave the current channel, or :leave #ops"),
    ("go", "open a conversation: :go #ops, :go @bob"),
    ("msg", "send a message: :msg @bob on my way"),
    ("react", "react to the selected message: :react rocket"),
    ("edit", "rewrite the selected message, yours only"),
    ("delete", "delete the selected message, yours only, after a yes"),
    ("thread", "open the selected message's thread"),
    ("search", "search Slack: :search deploy failed"),
    ("open", "open the selected message in Slack"),
    ("copy", "copy the selected message's permalink"),
    ("export", "save the conversation: :export json | md"),
    ("read", "mark the conversation read"),
    ("snooze", "snooze the selected inbox item: :snooze 1h | 3h | tomorrow | monday"),
    ("set", "change and save a setting: :set theme=nord · :set highlight=#2a2a2a"),
    ("help", "show the keys"),
    ("quit", "leave"),
];

pub fn parse(line: &str) -> Result<Command, String> {
    let line = line.trim().trim_start_matches(':').trim();
    let (verb, rest) = line.split_once(char::is_whitespace).map(|(v, r)| (v, r.trim())).unwrap_or((line, ""));
    let need =
        |what: &str| -> Result<String, String> { if rest.is_empty() { Err(format!(":{verb} needs {what}")) } else { Ok(rest.to_owned()) } };
    match verb {
        "" => Err("type a command, tab completes".into()),
        "join" | "j" => Ok(Command::Join(need("a channel")?)),
        "leave" => Ok(Command::Leave((!rest.is_empty()).then(|| rest.to_owned()))),
        "go" | "g" | "c" => Ok(Command::Go(need("a channel or @person")?)),
        "msg" | "m" | "dm" => {
            let (target, text) = rest.split_once(char::is_whitespace).map(|(t, x)| (t, x.trim())).unwrap_or((rest, ""));
            if target.is_empty() || text.is_empty() {
                return Err(":msg needs a target and a message".into());
            }
            Ok(Command::Msg { target: target.to_owned(), text: text.to_owned() })
        }
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
            let close = fuzzy::suggestions(unknown, VERBS.iter().map(|(v, _)| *v), 3);
            match close.is_empty() {
                true => Err(format!("unknown command :{unknown}")),
                false => Err(format!("unknown command :{unknown}, did you mean :{}?", close.join(", :"))),
            }
        }
    }
}

/// What the token under the cursor can complete to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Verb,
    Conversation,
    Channel,
    Person,
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
        ("go" | "g" | "c", 0) => Slot::Conversation,
        ("msg" | "m" | "dm", 0) => Slot::Conversation,
        ("react" | "r", 0) => Slot::Emoji,
        ("export" | "e", 0) => Slot::Literal(&["json", "md"]),
        ("snooze" | "z", 0) => Slot::Literal(&["1h", "3h", "tomorrow", "monday"]),
        ("set", 0) => Slot::Literal(&["theme=", "highlight=", "images=", "show_url="]),
        _ => Slot::Free,
    }
}

#[derive(Debug, Default)]
pub struct Palette {
    pub input: String,
    pub history: Vec<String>,
    history_at: Option<usize>,
    cycle: Option<Cycle>,
}

#[derive(Debug)]
struct Cycle {
    prefix: String,
    options: Vec<String>,
    at: usize,
}

impl Palette {
    pub fn with_history(history: Vec<String>) -> Self {
        Self { history, ..Default::default() }
    }

    pub fn type_char(&mut self, c: char) {
        self.input.push(c);
        self.cycle = None;
    }

    pub fn backspace(&mut self) {
        self.input.pop();
        self.cycle = None;
    }

    /// Replaces the token being typed with the next candidate; `candidates` supplies the
    /// labels for the slot under the cursor.
    pub fn complete(&mut self, candidates: &[String], backwards: bool) {
        if self.cycle.is_none() {
            let (prefix, token) = split_last_token(&self.input);
            let ranked: Vec<String> = if token.is_empty() {
                candidates.iter().take(50).cloned().collect()
            } else {
                fuzzy::rank(token, candidates.iter().map(|c| (c.clone(), c.clone()))).into_iter().map(|(_, c)| c).take(50).collect()
            };
            if ranked.is_empty() {
                return;
            }
            self.cycle = Some(Cycle { prefix: prefix.to_owned(), options: ranked, at: 0 });
        } else if let Some(cycle) = &mut self.cycle {
            let n = cycle.options.len();
            cycle.at = if backwards { (cycle.at + n - 1) % n } else { (cycle.at + 1) % n };
        }
        let cycle = self.cycle.as_ref().expect("set above");
        let chosen = &cycle.options[cycle.at];
        let trailing = if chosen.ends_with('=') { "" } else { " " };
        self.input = format!("{}{chosen}{trailing}", cycle.prefix);
    }

    /// The grey text zsh-style autosuggestion would show after the cursor: the rest of the
    /// best completion for the token being typed, or nothing when the token is empty or complete.
    pub fn ghost(&self, candidates: &[String]) -> Option<String> {
        if self.cycle.is_some() {
            return None;
        }
        let (_, token) = split_last_token(&self.input);
        if token.is_empty() {
            return None;
        }
        let lower = token.to_lowercase();
        let by_prefix = candidates.iter().find(|c| c.to_lowercase().starts_with(&lower) && c.len() > token.len());
        let best = by_prefix.cloned().or_else(|| fuzzy::best(token, candidates.iter().map(|c| (c.clone(), c.clone()))))?;
        if best.to_lowercase().starts_with(&lower) { Some(best[token.len()..].to_owned()) } else { None }
    }

    /// Accepts the ghost text, as `→` does in a shell.
    pub fn accept(&mut self, candidates: &[String]) {
        if let Some(rest) = self.ghost(candidates) {
            self.input.push_str(&rest);
            if !self.input.ends_with('=') {
                self.input.push(' ');
            }
        }
    }

    pub fn hint(&self) -> Option<String> {
        let cycle = self.cycle.as_ref()?;
        let shown: Vec<String> = cycle
            .options
            .iter()
            .enumerate()
            .skip(cycle.at)
            .take(4)
            .map(|(i, o)| if i == cycle.at { format!("[{o}]") } else { o.clone() })
            .collect();
        Some(shown.join("  "))
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let at = self.history_at.map(|i| i.saturating_sub(1)).unwrap_or(self.history.len() - 1);
        self.history_at = Some(at);
        self.input = self.history[at].clone();
        self.cycle = None;
    }

    pub fn history_down(&mut self) {
        let Some(at) = self.history_at else { return };
        if at + 1 >= self.history.len() {
            self.history_at = None;
            self.input.clear();
        } else {
            self.history_at = Some(at + 1);
            self.input = self.history[at + 1].clone();
        }
        self.cycle = None;
    }

    /// Takes the line, remembers it, and leaves the palette empty for the next command.
    pub fn submit(&mut self) -> String {
        let line = std::mem::take(&mut self.input).trim().to_owned();
        if !line.is_empty() && self.history.last() != Some(&line) {
            self.history.push(line.clone());
        }
        self.history_at = None;
        self.cycle = None;
        line
    }
}

fn split_last_token(input: &str) -> (&str, &str) {
    match input.rfind(char::is_whitespace) {
        Some(i) => (&input[..i + 1], &input[i + 1..]),
        None => ("", input),
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn tab_cycles_fuzzy_completions() {
        let mut p = Palette::default();
        for c in "join gen".chars() {
            p.type_char(c);
        }
        let candidates = vec!["#general".to_string(), "#engineering".to_string(), "#general-fr".to_string()];
        p.complete(&candidates, false);
        assert_eq!(p.input, "join #general ");
        assert!(p.hint().unwrap().starts_with("[#general]"));
        p.complete(&candidates, false);
        assert_eq!(p.input, "join #general-fr ");
        p.complete(&candidates, true);
        assert_eq!(p.input, "join #general ");
        p.type_char('x');
        assert_eq!(p.hint(), None);
    }

    #[test]
    fn ghost_text_suggests_and_accepts() {
        let mut p = Palette::default();
        let verbs: Vec<String> = VERBS.iter().map(|(v, _)| (*v).to_owned()).collect();
        for c in "jo".chars() {
            p.type_char(c);
        }
        assert_eq!(p.ghost(&verbs).as_deref(), Some("in"));
        p.accept(&verbs);
        assert_eq!(p.input, "join ");
        assert_eq!(p.ghost(&verbs), None);
        let channels = vec!["#general".to_string(), "#general-fr".to_string()];
        for c in "#GEN".chars() {
            p.type_char(c);
        }
        assert_eq!(p.ghost(&channels).as_deref(), Some("eral"));
        p.type_char('x');
        assert_eq!(p.ghost(&channels), None);
    }

    #[test]
    fn history_recall() {
        let mut p = Palette::default();
        for c in "go #a".chars() {
            p.type_char(c);
        }
        assert_eq!(p.submit(), "go #a");
        p.input = "go #b".into();
        p.submit();
        p.submit();
        assert_eq!(p.history, ["go #a", "go #b"]);
        p.history_up();
        assert_eq!(p.input, "go #b");
        p.history_up();
        assert_eq!(p.input, "go #a");
        p.history_up();
        assert_eq!(p.input, "go #a");
        p.history_down();
        assert_eq!(p.input, "go #b");
        p.history_down();
        assert_eq!(p.input, "");
    }
}
