use crate::inbox::{Item, Kind, Snooze, State};
use crate::mrkdwn;
use crate::render::text::{self, Style as TextStyle};
use crate::render::time;
use crate::resolve::NameBook;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph};

#[derive(Debug, Default)]
pub struct Inbox {
    pub items: Vec<Item>,
    pub selected: usize,
    pub loading: bool,
    pub picking_snooze: bool,
    pub state: State,
    pub flash: String,
    pub view: ListState,
}

impl Inbox {
    pub fn new(state: State) -> Self {
        Self { state, loading: true, ..Default::default() }
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    pub fn take_selected(&mut self) -> Option<Item> {
        if self.items.is_empty() {
            return None;
        }
        let item = self.items.remove(self.selected);
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
        Some(item)
    }

    pub fn set_items(&mut self, items: Vec<Item>) {
        self.state.forget_expired(crate::inbox::local_now());
        self.items = self.state.visible(items, crate::inbox::local_now());
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
        self.loading = false;
    }

    pub fn move_by(&mut self, delta: i64) {
        if self.items.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as i64).saturating_add(delta).clamp(0, self.items.len() as i64 - 1) as usize;
    }

    pub fn snooze_selected(&mut self, preset: Snooze) -> Option<Item> {
        let item = self.take_selected()?;
        self.state.snooze(&item.key, preset.until(crate::inbox::local_now()));
        self.picking_snooze = false;
        self.flash = format!("snoozed until {}", preset.label());
        Some(item)
    }

    pub fn read_selected(&mut self) -> Option<Item> {
        let item = self.take_selected()?;
        self.state.mark_read(&item.key, &item.ts);
        self.flash = format!("marked {} read", item.label);
        Some(item)
    }

    pub fn read_all(&mut self) -> Vec<Item> {
        let items = std::mem::take(&mut self.items);
        for item in &items {
            self.state.mark_read(&item.key, &item.ts);
        }
        self.selected = 0;
        self.flash = format!("marked {} read", items.len());
        items
    }
}

pub fn draw(f: &mut Frame, inbox: &mut Inbox, names: &NameBook, area: Rect, highlight: Color) {
    let popup = centered(area, 92, 90);
    f.render_widget(Clear, popup);
    let title = match (inbox.loading, inbox.items.len()) {
        (true, _) => " inbox · loading… ".to_owned(),
        (false, 0) => " inbox · all clear ".to_owned(),
        (false, n) => format!(" inbox · {n} "),
    };
    let block = super::ui::pane(title.trim(), true);
    let inner = block.inner(popup);
    f.render_widget(block, popup);
    let [list_area, hint_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    if inbox.items.is_empty() && !inbox.loading {
        f.render_widget(Paragraph::new("  nothing waiting for you ✨".dim()), Rect { y: list_area.y + 1, ..list_area });
    }
    let width = list_area.width as usize;
    let items: Vec<ListItem> = inbox.items.iter().map(|i| item_lines(i, names, width)).collect();
    let list = List::new(items)
        .highlight_style(Style::new().bg(highlight).add_modifier(Modifier::BOLD))
        .highlight_symbol(super::ui::cursor_bar(true))
        .repeat_highlight_symbol(true)
        .highlight_spacing(HighlightSpacing::Always);
    inbox.view.select((!inbox.items.is_empty()).then_some(inbox.selected));
    f.render_stateful_widget(list, list_area, &mut inbox.view);
    let hints = "→ read · ← snooze · r reply · enter open · o slack · a all read · R refresh · esc close";
    let flash = if inbox.flash.is_empty() { String::new() } else { format!(" {} ·", inbox.flash) };
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(flash, Style::new().green()), Span::styled(format!(" {hints}"), Style::new().dim())])),
        hint_area,
    );
    if inbox.picking_snooze {
        draw_snooze_picker(f, area);
    }
}

fn item_lines(item: &Item, names: &NameBook, width: usize) -> ListItem<'static> {
    let (icon, what) = match item.kind {
        Kind::Dm => ("✉", format!("{} new", item.unread.len())),
        Kind::Mention => ("@", "mention".to_owned()),
        Kind::Thread => ("⤷", format!("{} new in thread", item.unread.len())),
    };
    let head = Line::from(vec![
        Span::styled(format!(" {icon} "), Style::new().cyan().bold()),
        Span::styled(item.label.clone(), Style::new().bold()),
        Span::styled(format!("  {}  ·  {what}", time::relative(&item.ts)), Style::new().dim()),
    ]);
    let mut lines = vec![head];
    for m in item.unread.iter().rev().take(2).collect::<Vec<_>>().into_iter().rev() {
        let author = m.user.as_deref().map(|u| names.user_label(u)).or_else(|| m.username.clone()).unwrap_or_else(|| "bot".into());
        let styled = text::from_segments(&mrkdwn::parse(&m.text, names), false);
        let mut pieces = styled.wrap_styled(width.saturating_sub(6 + author.len()).max(10));
        let first = pieces.drain(..1).next().unwrap_or_default();
        let mut spans = vec![Span::raw("   "), Span::styled(format!("{author}: "), Style::new().magenta())];
        spans.extend(first.into_iter().map(|p| Span::styled(p.text, style_of(p.style))));
        if !pieces.is_empty() {
            spans.push(Span::styled("…", Style::new().dim()));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));
    ListItem::new(lines)
}

fn draw_snooze_picker(f: &mut Frame, area: Rect) {
    let lines: Vec<Line> = Snooze::ALL
        .iter()
        .enumerate()
        .map(|(i, s)| Line::from(vec![Span::styled(format!("  {}  ", i + 1), Style::new().cyan().bold()), Span::raw(s.label().to_owned())]))
        .collect();
    let height = lines.len() as u16 + 2;
    let popup = Rect {
        x: area.x + area.width.saturating_sub(30) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: 30.min(area.width),
        height,
    };
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(super::ui::BORDER))
                .title(" snooze for ".bold().yellow()),
        ),
        popup,
    );
}

fn centered(area: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let w = area.width * pct_w / 100;
    let h = area.height * pct_h / 100;
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn style_of(s: TextStyle) -> Style {
    match s {
        TextStyle::Plain => Style::new(),
        TextStyle::Dim => Style::new().dim(),
        TextStyle::Bold => Style::new().bold(),
        TextStyle::Italic => Style::new().italic(),
        TextStyle::Strike => Style::new().crossed_out(),
        TextStyle::Code => Style::new().yellow(),
        TextStyle::Link => Style::new().blue().underlined(),
        TextStyle::Mention => Style::new().magenta(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Message;

    fn item(key: &str, kind: Kind) -> Item {
        Item {
            key: key.into(),
            kind,
            channel: "C1".into(),
            label: format!("#{key}"),
            thread_ts: None,
            ts: "5.0".into(),
            unread: vec![Message { ts: "5.0".into(), text: "hi".into(), ..Default::default() }],
        }
    }

    #[test]
    fn read_and_snooze_remove_and_remember() {
        let mut inbox = Inbox::new(State::default());
        inbox.set_items(vec![item("a", Kind::Dm), item("b", Kind::Mention), item("c", Kind::Thread)]);
        inbox.move_by(1);
        let read = inbox.read_selected().unwrap();
        assert_eq!(read.key, "b");
        assert_eq!(inbox.items.len(), 2);
        assert_eq!(inbox.selected, 1);
        let snoozed = inbox.snooze_selected(Snooze::OneHour).unwrap();
        assert_eq!(snoozed.key, "c");
        assert_eq!(inbox.selected, 0);
        inbox.set_items(vec![item("a", Kind::Dm), item("b", Kind::Mention), item("c", Kind::Thread)]);
        assert_eq!(inbox.items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["a"]);
        assert_eq!(inbox.read_all().len(), 1);
        assert!(inbox.items.is_empty());
        assert!(inbox.read_selected().is_none());
    }

    #[test]
    fn renders_without_panicking_on_narrow_terminals() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut inbox = Inbox::new(State::default());
        inbox.set_items(vec![item("a", Kind::Dm)]);
        inbox.picking_snooze = true;
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        terminal.draw(|f| draw(f, &mut inbox, &NameBook::default(), f.area(), Color::Indexed(236))).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("inbox · 1"));
        assert!(out.contains("snooze for"));
    }
}
