use crate::fuzzy;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};

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

pub const MAX_SHOWN: usize = 12;

#[derive(Debug, Default)]
pub struct Jump {
    pub query: String,
    pub selected: usize,
    pub channels: Vec<Candidate>,
    pub people: Vec<Candidate>,
    pub threads: Vec<Candidate>,
}

impl Jump {
    pub fn is_search(&self) -> bool {
        self.query.starts_with('>')
    }

    /// Best matches for the query, channels first on an empty query.
    pub fn matches(&self) -> Vec<Candidate> {
        if self.is_search() {
            return vec![];
        }
        let all = self.channels.iter().chain(&self.people).chain(&self.threads).cloned();
        if self.query.is_empty() {
            return all.take(MAX_SHOWN).collect();
        }
        fuzzy::rank(&self.query, all.map(|c| (c.label.clone(), c))).into_iter().map(|(_, c)| c).take(MAX_SHOWN).collect()
    }

    pub fn selected_candidate(&self) -> Option<Candidate> {
        self.matches().into_iter().nth(self.selected)
    }

    pub fn move_by(&mut self, delta: i64) {
        let len = self.matches().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as i64).saturating_add(delta).clamp(0, len as i64 - 1) as usize;
    }

    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }
}

pub fn draw(f: &mut Frame, jump: &Jump, area: Rect, highlight: Color) {
    let width = (area.width * 3 / 5).clamp(30.min(area.width), area.width);
    let height = (MAX_SHOWN as u16 + 3).min(area.height);
    let popup = Rect { x: area.x + (area.width - width) / 2, y: area.y + area.height.saturating_sub(height) / 3, width, height };
    f.render_widget(Clear, popup);
    let block = Block::bordered().border_style(Style::new().cyan()).title(" jump · > to search ".bold().cyan());
    let inner = block.inner(popup);
    f.render_widget(block, popup);
    let prompt = Line::from(vec![
        Span::styled(" › ", Style::new().cyan().bold()),
        Span::raw(jump.query.clone()),
        Span::styled("▌", Style::new().cyan()),
    ]);
    f.render_widget(Paragraph::new(prompt), Rect { height: 1, ..inner });
    let list_area = Rect { y: inner.y + 1, height: inner.height.saturating_sub(1), ..inner };
    if jump.is_search() {
        f.render_widget(Paragraph::new(format!("   enter searches Slack for “{}”", jump.query[1..].trim()).dim()), list_area);
        return;
    }
    let items: Vec<ListItem> = jump
        .matches()
        .into_iter()
        .map(|c| {
            let (icon, style) = match c.target {
                Target::Channel(_) => ("#", Style::new()),
                Target::Person(_) => ("@", Style::new().magenta()),
                Target::Thread { .. } => ("⤷", Style::new().cyan()),
            };
            ListItem::new(Line::from(vec![Span::styled(format!(" {icon} "), style.bold()), Span::raw(c.label)]))
        })
        .collect();
    let empty = items.is_empty();
    let list = List::new(items).highlight_style(Style::new().bg(highlight).add_modifier(Modifier::BOLD));
    let mut state = ListState::default().with_selected((!empty).then_some(jump.selected));
    f.render_stateful_widget(list, list_area, &mut state);
    if empty {
        f.render_widget(Paragraph::new("   no match".dim()), list_area);
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn empty_query_lists_everything_in_order() {
        let labels: Vec<String> = jump().matches().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, ["#general", "#vivien-vault", "@vivien", "#general · deploy plan"]);
    }

    #[test]
    fn query_ranks_and_selection_follows() {
        let mut j = jump();
        for c in "vvt".chars() {
            j.type_char(c);
        }
        assert_eq!(j.selected_candidate().unwrap().target, Target::Channel("C2".into()));
        j.move_by(5);
        assert_eq!(j.selected, j.matches().len() - 1);
        j.backspace();
        assert_eq!(j.selected, 0);
    }

    #[test]
    fn search_prefix_has_no_matches() {
        let mut j = jump();
        j.type_char('>');
        assert!(j.is_search());
        assert!(j.matches().is_empty());
    }
}
