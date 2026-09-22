use super::theme::Theme;
use crate::firehose::{Highlighter, Line as LiveLine, Tag};
use crate::render;
use crate::render::text;
use crate::render::time;
use crate::resolve::NameBook;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{HighlightSpacing, List, ListItem, ListState};
use std::collections::VecDeque;

pub const CAPACITY: usize = 1000;

/// The wall view: follows the newest line unless the user scrolled up.
#[derive(Debug, Default)]
pub struct Firehose {
    pub selected: Option<usize>,
    pub view: ListState,
    pub show_noise: bool,
}

impl Firehose {
    /// The lines on screen: what Jev tagged as noise stays out unless asked for.
    pub fn visible<'a>(&self, wall: &'a VecDeque<LiveLine>) -> Vec<&'a LiveLine> {
        wall.iter().filter(|l| self.show_noise || !l.is_noise()).collect()
    }

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

/// A tag lands on its line if the line is still on the wall.
pub fn tag(wall: &mut VecDeque<LiveLine>, channel: &str, ts: &str, tag: Tag) {
    if let Some(line) = wall.iter_mut().find(|l| l.channel == channel && l.ts == ts) {
        line.tag = Some(tag);
    }
}

pub fn draw(f: &mut Frame, view: &mut Firehose, wall: &VecDeque<LiveLine>, names: &NameBook, hl: &Highlighter, area: Rect, theme: &Theme) {
    let lines = view.visible(wall);
    let status = if view.following() { "live" } else { "paused · G to follow" };
    let noise = match (view.show_noise, wall.len() - lines.len()) {
        (true, _) => " · n hides noise".to_owned(),
        (false, 0) => String::new(),
        (false, hidden) => format!(" · {hidden} noise hidden · n shows"),
    };
    let block = super::ui::pane(theme, &format!("firehose · {} · {status}{noise}", lines.len()), true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;
    let items: Vec<ListItem> = lines.iter().map(|l| row(theme, l, names, hl, width)).collect();
    let selected = if lines.is_empty() { None } else { Some(view.selected.unwrap_or(lines.len() - 1)) };
    let list = List::new(items)
        .highlight_symbol(super::ui::cursor_bar(theme, !view.following()))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(if view.following() { Style::new() } else { super::ui::row_highlight(theme, true).bold() });
    view.view.select(selected);
    f.render_stateful_widget(list, inner, &mut view.view);
    if lines.is_empty() {
        f.render_widget(ratatui::widgets::Paragraph::new("  waiting for messages…".fg(theme.muted)), Rect { y: inner.y + 1, ..inner });
    }
}

fn row(theme: &Theme, l: &LiveLine, names: &NameBook, hl: &Highlighter, width: usize) -> ListItem<'static> {
    let hit_style = Style::new().fg(theme.base).bg(theme.warn).bold();
    let label = names.channel_label(&l.channel);
    let author = l.author(names);
    let text = l.flat_text(names);
    let hit = hl.hits(&text);
    let (mark, body_style) = tag_look(theme, l.tag);
    let head_width = 2 + 5 + 1 + 2 + 16 + 1 + 10 + 1 + if l.in_thread { 2 } else { 0 };
    let body = text::truncate(&text, width.saturating_sub(head_width).max(10));
    let mut spans = vec![
        Span::styled(if hit { "! " } else { "  " }, hit_style),
        Span::styled(time::hhmm(&l.ts), Style::new().fg(theme.muted)),
        Span::raw(" "),
        mark,
        Span::styled(render::fit(&label, 16), Style::new().fg(theme.user(&label))),
        Span::raw(" "),
        Span::styled(render::fit(&author, 10), super::ui::user_style(theme, &author)),
        Span::raw(" "),
    ];
    if l.in_thread {
        spans.push(Span::styled("↳ ", Style::new().fg(theme.muted)));
    }
    for (piece, h) in hl.split(&body) {
        spans.push(Span::styled(piece, if h { hit_style } else { body_style }));
    }
    if !hit {
        spans[0] = Span::raw("  ");
    }
    ListItem::new(Line::from(spans))
}

/// A two-column mark before the channel, and the style of the text: incidents in red, noise receded.
fn tag_look(theme: &Theme, tag: Option<Tag>) -> (Span<'static>, Style) {
    let danger = Style::new().fg(theme.danger).bold();
    match tag {
        Some(Tag::Incident) => (Span::styled("▲ ", danger), danger),
        Some(Tag::QuestionForMe) => (Span::styled("? ", Style::new().fg(theme.accent).bold()), Style::new()),
        Some(Tag::Noise) => (Span::raw("  "), Style::new().fg(theme.faded)),
        Some(Tag::Fyi) | None => (Span::raw("  "), Style::new()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
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
            tag: None,
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
        let mut view = Firehose::default();
        terminal.draw(|f| draw(f, &mut view, &wall, &NameBook::default(), &hl, f.area(), &Theme::default())).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("firehose · 2 · live"));
        assert!(out.contains("! "));
        assert!(out.contains("↳ event 1 on prod"));
        assert!(out.contains("quiet"));
    }

    #[test]
    fn noise_hides_until_shown_and_incidents_are_marked() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut wall = VecDeque::new();
        push(&mut wall, LiveLine { text: "lunch anyone".into(), ..line(2) });
        push(&mut wall, LiveLine { text: "db is down".into(), ..line(4) });
        tag(&mut wall, "C1", &line(2).ts, Tag::Noise);
        tag(&mut wall, "C1", &line(4).ts, Tag::Incident);
        tag(&mut wall, "C1", "gone", Tag::Fyi);
        let mut view = Firehose::default();
        let draw_wall = |view: &mut Firehose| {
            let mut terminal = Terminal::new(TestBackend::new(90, 6)).unwrap();
            terminal.draw(|f| draw(f, view, &wall, &NameBook::default(), &Highlighter::default(), f.area(), &Theme::default())).unwrap();
            terminal.backend().to_string()
        };
        let out = draw_wall(&mut view);
        assert!(out.contains("firehose · 1 · live · 1 noise hidden · n shows"), "{out}");
        assert!(out.contains("▲ C1"), "{out}");
        assert!(!out.contains("lunch anyone"));
        view.show_noise = true;
        let out = draw_wall(&mut view);
        assert!(out.contains("lunch anyone") && out.contains("n hides noise"), "{out}");
    }
}
