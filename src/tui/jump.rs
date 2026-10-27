//! The `⌘k` jump: a palette that finds a channel, a person or a thread, or searches Slack after a `>`.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use vtui::fuzzy;
use vtui::palette::MAX_SHOWN;
use vtui::palette::view::{self, Pick, Popup};
use vtui::theme::Theme;

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Channel(String),
    Person(String),
    Thread { channel: String, ts: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub label: String,
    pub target: Target,
}

/// What the line looks for: a place to go, or after a `>` the words to search Slack for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Find,
    Search,
}

impl vtui::palette::Mode for Mode {
    fn switched_by(self, first: char) -> Option<Self> {
        (self == Self::Find && first == '>').then_some(Self::Search)
    }

    fn takes_commands(self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
pub struct Jump {
    pub palette: vtui::palette::Palette<Mode>,
    pub channels: Vec<Candidate>,
    pub people: Vec<Candidate>,
    pub threads: Vec<Candidate>,
}

impl Jump {
    /// The words to search Slack for, once a `>` started the line.
    pub fn search(&self) -> Option<&str> {
        (self.palette.mode == Mode::Search).then(|| self.palette.input.trim())
    }

    /// Best matches for the query, channels first on an empty query.
    pub fn matches(&self) -> Vec<Candidate> {
        if self.palette.mode == Mode::Search {
            return vec![];
        }
        let all = self.channels.iter().chain(&self.people).chain(&self.threads);
        if self.palette.input.is_empty() {
            return all.take(MAX_SHOWN).cloned().collect();
        }
        let ranked = fuzzy::rank(&self.palette.input, all.map(|c| (c.label.as_str(), c)));
        ranked.into_iter().map(|(_, c)| c.clone()).take(MAX_SHOWN).collect()
    }

    pub fn selected_candidate(&self) -> Option<Candidate> {
        self.matches().into_iter().nth(self.palette.selected)
    }

    pub fn move_by(&mut self, delta: isize) {
        let len = self.matches().len();
        self.palette.move_by(delta, len);
    }

    pub fn type_char(&mut self, c: char) {
        self.palette.type_char(c);
    }

    /// `false` once there is nothing left to erase, which closes the jump.
    pub fn backspace(&mut self) -> bool {
        self.palette.backspace()
    }
}

pub fn draw(f: &mut Frame, jump: &Jump, area: Rect, theme: &Theme) {
    let (sign, footer) = match jump.palette.mode {
        Mode::Find => ("", "  ↑↓ pick · enter opens · > searches Slack · esc"),
        Mode::Search => (">", "  enter searches · backspace goes back · esc"),
    };
    let prompt = view::prompt(theme, &jump.palette, sign, |_| vec![]);
    let popup = Popup { title: "jump · > to search", prompt, footer };
    view::draw(f, theme, area, popup, |room| match jump.search() {
        Some(query) => vec![Line::from(Span::styled(format!("  enter searches Slack for “{query}”"), Style::new().fg(theme.muted)))],
        None => pick_lines(jump, theme, room),
    });
}

fn pick_lines(jump: &Jump, theme: &Theme, room: usize) -> Vec<Line<'static>> {
    let matches = jump.matches();
    let picks: Vec<Pick> = matches
        .iter()
        .map(|c| {
            let (icon, colour) = match c.target {
                Target::Channel(_) => ("#", theme.muted),
                Target::Person(_) => ("@", theme.mention),
                Target::Thread { .. } => ("⤷", theme.accent),
            };
            Pick { icon, colour, label: &c.label, detail: "" }
        })
        .collect();
    view::pick_lines(theme, &picks, jump.palette.selected, room)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn jump() -> Jump {
        Jump {
            channels: vec![
                Candidate { label: "#general".into(), target: Target::Channel("C1".into()) },
                Candidate { label: "#vivien-vault".into(), target: Target::Channel("C2".into()) },
            ],
            people: vec![Candidate { label: "@vivien".into(), target: Target::Person("U1".into()) }],
            threads: vec![Candidate {
                label: "#general · deploy plan".into(),
                target: Target::Thread { channel: "C1".into(), ts: "1".into() },
            }],
            ..Default::default()
        }
    }

    fn typed(text: &str) -> Jump {
        let mut j = jump();
        text.chars().for_each(|c| j.type_char(c));
        j
    }

    #[test]
    fn empty_query_lists_everything_in_order() {
        let labels: Vec<String> = jump().matches().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, ["#general", "#vivien-vault", "@vivien", "#general · deploy plan"]);
    }

    #[test]
    fn query_ranks_and_selection_follows() {
        let mut j = typed("vvt");
        assert_eq!(j.selected_candidate().unwrap().target, Target::Channel("C2".into()));
        j.move_by(5);
        assert_eq!(j.palette.selected, j.matches().len() - 1);
        j.backspace();
        assert_eq!(j.palette.selected, 0);
    }

    #[test]
    fn a_leading_chevron_searches_and_lists_nothing() {
        let j = typed(">deploy failed ");
        assert_eq!(j.search(), Some("deploy failed"));
        assert_eq!(j.matches(), [] as [Candidate; 0]);
    }

    #[test]
    fn backspace_on_an_empty_search_goes_back_to_finding() {
        let mut j = typed(">");
        assert!(j.backspace());
        assert_eq!(j.search(), None);
        assert!(!j.backspace(), "nothing left: the jump closes");
    }
}
