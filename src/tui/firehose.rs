use crate::firehose::{Highlighter, Line as LiveLine};
use crate::render::text;
use crate::render::time;
use crate::resolve::NameBook;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use std::collections::VecDeque;

pub const CAPACITY: usize = 1000;
const CHANNEL_COLORS: [Color; 6] = [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta, Color::Blue, Color::LightRed];

/// The wall view: follows the newest line unless the user scrolled up.
#[derive(Debug, Default)]
pub struct Firehose {
    pub selected: Option<usize>,
}

impl Firehose {
    pub fn following(&self) -> bool {
        self.selected.is_none()
    }

    pub fn move_by(&mut self, delta: i64, len: usize) {
        if len == 0 {
            self.selected = None;
            return;
        }
        let current = self.selected.unwrap_or(len - 1) as i64;
        let next = (current + delta).clamp(0, len as i64 - 1) as usize;
        self.selected = (next + 1 < len).then_some(next);
    }

    pub fn follow(&mut self) {
        self.selected = None;
    }
}

pub fn push(wall: &mut VecDeque<LiveLine>, line: LiveLine) {
    if wall.len() == CAPACITY {
        wall.pop_front();
    }
    wall.push_back(line);
}

pub fn draw(f: &mut Frame, view: &Firehose, wall: &VecDeque<LiveLine>, names: &NameBook, hl: &Highlighter, area: Rect, highlight: Color) {
    let status = if view.following() { "live" } else { "paused · G to follow" };
    let block = super::ui::pane(&format!("firehose · {} · {status}", wall.len()), true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;
    let items: Vec<ListItem> = wall.iter().map(|l| row(l, names, hl, width)).collect();
    let selected = if wall.is_empty() { None } else { Some(view.selected.unwrap_or(wall.len() - 1)) };
    let list = List::new(items).highlight_style(if view.following() {
        Style::new()
    } else {
        Style::new().bg(highlight).add_modifier(Modifier::BOLD)
    });
    let mut state = ListState::default().with_selected(selected);
    f.render_stateful_widget(list, inner, &mut state);
    if wall.is_empty() {
        f.render_widget(ratatui::widgets::Paragraph::new("  waiting for messages…".dim()), Rect { y: inner.y + 1, ..inner });
    }
}

fn row(l: &LiveLine, names: &NameBook, hl: &Highlighter, width: usize) -> ListItem<'static> {
    let label = names.channel_label(&l.channel);
    let author = l.user.as_deref().map(|u| names.user_label(u)).or_else(|| l.username.clone()).unwrap_or_else(|| "bot".into());
    let text = l.flat_text(names);
    let hit = hl.hits(&text);
    let head_width = 2 + 5 + 1 + 16 + 1 + 10 + 1 + if l.in_thread { 2 } else { 0 };
    let body = text::truncate(&text, width.saturating_sub(head_width).max(10));
    let mut spans = vec![
        Span::styled(if hit { "! " } else { "  " }, Style::new().black().on_yellow().bold()),
        Span::styled(time::hhmm(&l.ts), Style::new().dim()),
        Span::raw(" "),
        Span::styled(text::visible_fit(&label, 16), channel_style(&label)),
        Span::raw(" "),
        Span::styled(text::visible_fit(&author, 10), super::ui::user_style(&author)),
        Span::raw(" "),
    ];
    if l.in_thread {
        spans.push(Span::styled("↳ ", Style::new().dim()));
    }
    for (piece, h) in hl.split(&body) {
        spans.push(if h { Span::styled(piece, Style::new().black().on_yellow().bold()) } else { Span::raw(piece) });
    }
    if !hit {
        spans[0] = Span::raw("  ");
    }
    ListItem::new(Line::from(spans))
}

fn channel_style(label: &str) -> Style {
    let idx = label.trim().bytes().fold(7usize, |h, b| h.wrapping_mul(33).wrapping_add(b as usize)) % CHANNEL_COLORS.len();
    Style::new().fg(CHANNEL_COLORS[idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: u64) -> LiveLine {
        LiveLine {
            ts: format!("{}.000000", 1694700000 + n),
            channel: "C1".into(),
            user: None,
            username: Some("bot".into()),
            text: format!("event {n} on prod"),
            in_thread: n % 2 == 1,
            thread_ts: None,
        }
    }

    #[test]
    fn ring_buffer_and_scrolling() {
        let mut wall = VecDeque::new();
        for n in 0..(CAPACITY as u64 + 5) {
            push(&mut wall, line(n));
        }
        assert_eq!(wall.len(), CAPACITY);
        assert_eq!(wall.front().unwrap().text, "event 5 on prod");
        let mut view = Firehose::default();
        assert!(view.following());
        view.move_by(-1, wall.len());
        assert_eq!(view.selected, Some(CAPACITY - 2));
        view.move_by(10, wall.len());
        assert!(view.following());
        view.move_by(i64::MIN / 2, wall.len());
        assert_eq!(view.selected, Some(0));
        view.follow();
        assert!(view.following());
    }

    #[test]
    fn renders_hits_and_threads() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut wall = VecDeque::new();
        push(&mut wall, line(1));
        push(&mut wall, LiveLine { text: "quiet".into(), in_thread: false, ..line(2) });
        let hl = Highlighter::new(&["prod".into()]).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 6)).unwrap();
        terminal.draw(|f| draw(f, &Firehose::default(), &wall, &NameBook::default(), &hl, f.area(), Color::Indexed(236))).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("firehose · 2 · live"));
        assert!(out.contains("! "));
        assert!(out.contains("↳ event 1 on prod"));
        assert!(out.contains("quiet"));
    }
}
