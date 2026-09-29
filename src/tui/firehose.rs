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
use std::ops::Range;

pub const CAPACITY: usize = 1000;

/// The wall view: follows the newest line unless the user scrolled up.
#[derive(Debug, Default)]
pub struct Firehose {
    pub selected: Option<usize>,
    /// First line on screen; it holds until the selection leaves the screen.
    offset: usize,
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

pub fn push(wall: &mut VecDeque<LiveLine>, mut line: LiveLine, names: &NameBook, hl: &Highlighter) {
    line.flat = line.flat_text(names);
    line.hit = hl.hits(&line.flat);
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
    let selected = view.selected.unwrap_or(lines.len().saturating_sub(1));
    let shown = window(view.offset, selected, lines.len(), inner.height as usize);
    view.offset = shown.start;
    let items: Vec<ListItem> = lines[shown.clone()].iter().map(|l| row(theme, l, names, hl, width)).collect();
    let list = List::new(items)
        .highlight_symbol(super::ui::cursor_bar(theme, !view.following()))
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(if view.following() { Style::new() } else { super::ui::row_highlight(theme, true).bold() });
    let mut state = ListState::default().with_selected((!shown.is_empty()).then(|| selected - shown.start));
    f.render_stateful_widget(list, inner, &mut state);
    if lines.is_empty() {
        f.render_widget(ratatui::widgets::Paragraph::new("  waiting for messages…".fg(theme.muted)), Rect { y: inner.y + 1, ..inner });
    }
}

/// The lines that fit in `height` rows: the last offset holds unless the selection left it.
fn window(offset: usize, selected: usize, len: usize, height: usize) -> Range<usize> {
    let height = height.max(1);
    let start = offset.clamp((selected + 1).saturating_sub(height), selected);
    start..(start + height).min(len)
}

fn row(theme: &Theme, l: &LiveLine, names: &NameBook, hl: &Highlighter, width: usize) -> ListItem<'static> {
    let hit_style = Style::new().fg(theme.base).bg(theme.warn).bold();
    let label = names.channel_label(&l.channel);
    let author = l.author(names);
    let (mark, body_style) = tag_look(theme, l.tag);
    let head_width = 2 + 5 + 1 + 2 + 16 + 1 + 10 + 1 + if l.in_thread { 2 } else { 0 };
    let body = text::truncate(&l.flat, width.saturating_sub(head_width).max(10));
    let mut spans = vec![
        Span::styled(if l.hit { "! " } else { "  " }, hit_style),
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
    if !l.hit {
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
            ts: format!("{}.000000", 1_694_700_000 + n),
            channel: "C1".into(),
            user: None,
            username: Some("bot".into()),
            text: format!("event {n} on prod"),
            in_thread: n % 2 == 1,
            thread_ts: None,
            tag: None,
            flat: String::new(),
            hit: false,
        }
    }

    fn wall_of(lines: impl IntoIterator<Item = LiveLine>, hl: &Highlighter) -> VecDeque<LiveLine> {
        let mut wall = VecDeque::new();
        for l in lines {
            push(&mut wall, l, &NameBook::default(), hl);
        }
        wall
    }

    fn draw_at(view: &mut Firehose, wall: &VecDeque<LiveLine>, hl: &Highlighter, width: u16, height: u16) -> String {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, view, wall, &NameBook::default(), hl, f.area(), &Theme::default())).unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn ring_buffer_and_scrolling() {
        let wall = wall_of((0..(CAPACITY as u64 + 5)).map(line), &Highlighter::default());
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
    fn a_pushed_line_keeps_its_flat_text_and_hit() {
        let hl = Highlighter::new(&["prod".into()]).unwrap();
        let wall = wall_of([LiveLine { text: "*deploy*\n  to <https://a.io|prod>".into(), ..line(0) }], &hl);
        assert_eq!((wall[0].flat.as_str(), wall[0].hit), ("deploy to prod", true));
        let quiet = wall_of([LiveLine { text: "all good".into(), ..line(0) }], &hl);
        assert!(!quiet[0].hit);
    }

    #[test]
    fn the_window_holds_its_offset_until_the_selection_leaves_it() {
        assert_eq!(window(0, 3, 10, 5), 0..5);
        assert_eq!(window(0, 7, 10, 5), 3..8);
        assert_eq!(window(6, 2, 10, 5), 2..7);
        assert_eq!(window(8, 9, 10, 5), 8..10);
        assert_eq!(window(4, 0, 0, 5), 0..0);
        assert_eq!(window(0, 3, 10, 0), 3..4);
    }

    #[test]
    fn a_full_wall_draws_the_newest_lines_then_the_oldest_when_scrolled_up() {
        let wall = wall_of((0..CAPACITY as u64).map(line), &Highlighter::default());
        let mut view = Firehose::default();
        let out = draw_at(&mut view, &wall, &Highlighter::default(), 80, 6);
        assert!(out.contains("event 999 on prod") && out.contains("event 996 on prod") && !out.contains("event 995 "), "{out}");
        view.move_by(i64::MIN / 2, wall.len());
        let out = draw_at(&mut view, &wall, &Highlighter::default(), 80, 6);
        assert!(out.contains("event 0 on prod") && out.contains("event 3 on prod") && !out.contains("event 4 "), "{out}");
        view.move_by(4, wall.len());
        let out = draw_at(&mut view, &wall, &Highlighter::default(), 80, 6);
        assert!(out.contains("event 1 on prod") && out.contains("event 4 on prod") && !out.contains("event 0 "), "{out}");
    }

    #[test]
    fn renders_hits_and_threads() {
        let hl = Highlighter::new(&["prod".into()]).unwrap();
        let wall = wall_of([line(1), LiveLine { text: "quiet".into(), in_thread: false, ..line(2) }], &hl);
        let out = draw_at(&mut Firehose::default(), &wall, &hl, 80, 6);
        assert!(out.contains("firehose · 2 · live"));
        assert!(out.contains("! "));
        assert!(out.contains("↳ event 1 on prod"));
        assert!(out.contains("quiet"));
    }

    #[test]
    fn noise_hides_until_shown_and_incidents_are_marked() {
        let lines = [LiveLine { text: "lunch anyone".into(), ..line(2) }, LiveLine { text: "db is down".into(), ..line(4) }];
        let mut wall = wall_of(lines, &Highlighter::default());
        tag(&mut wall, "C1", &line(2).ts, Tag::Noise);
        tag(&mut wall, "C1", &line(4).ts, Tag::Incident);
        tag(&mut wall, "C1", "gone", Tag::Fyi);
        let mut view = Firehose::default();
        let draw_wall = |view: &mut Firehose| draw_at(view, &wall, &Highlighter::default(), 90, 6);
        let out = draw_wall(&mut view);
        assert!(out.contains("firehose · 1 · live · 1 noise hidden · n shows"), "{out}");
        assert!(out.contains("▲ C1"), "{out}");
        assert!(!out.contains("lunch anyone"));
        view.show_noise = true;
        let out = draw_wall(&mut view);
        assert!(out.contains("lunch anyone") && out.contains("n hides noise"), "{out}");
    }
}
