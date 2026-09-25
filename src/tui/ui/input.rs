use crate::tui::app::{App, Input, REACT_PAGE};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let label = match &app.input {
        Some(Input::Reply { label, .. }) => format!("reply to {label}"),
        Some(Input::InboxReply { item }) => format!("reply to {}", item.label),
        Some(Input::Edit { .. }) => "edit".into(),
        Some(Input::Filter) => "filter".into(),
        Some(Input::Search) => "search".into(),
        None => return,
    };
    let (before, under, after) = app.buffer.split();
    let line = Line::from(vec![
        Span::styled(format!(" {label} ▸ "), Style::new().fg(app.theme.accent).bold()),
        Span::raw(before.to_owned()),
        caret(&app.theme, under),
        Span::raw(after.to_owned()),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// Where the cursor is: on the character it covers, or a bar of its own past the last one.
fn caret(theme: &Theme, under: &str) -> Span<'static> {
    if under.is_empty() {
        return Span::styled("▌", Style::new().fg(theme.accent));
    }
    Span::styled(under.to_owned(), Style::new().fg(theme.base).bg(theme.accent))
}

/// The picker: the search typed so far, then the page of choices around the selected one, mine
/// standing out and the selected one framed; on the strip, the selected one's name after it.
pub(super) fn draw_react(f: &mut Frame, app: &App, area: Rect) {
    let Some(pick) = &app.react else { return };
    let theme = &app.theme;
    let faded = Style::new().fg(theme.faded);
    let mut spans = vec![Span::styled(" react ▸ ", Style::new().fg(theme.accent).bold())];
    if let Some(search) = &pick.search {
        spans.push(Span::raw(format!("/{}", search.query)));
        spans.push(Span::styled("▌ ", Style::new().fg(theme.accent)));
    }
    let first = pick.selected - pick.selected % REACT_PAGE;
    for (i, name) in pick.choices().iter().enumerate().skip(first).take(REACT_PAGE) {
        let style = if i == pick.selected {
            Style::new().fg(theme.base).bg(theme.accent)
        } else if app.is_mine(&pick.ts, name) {
            Style::new().fg(theme.success).bold()
        } else {
            Style::new()
        };
        spans.push(Span::styled(format!(" {} ", choice_label(i, name, pick.search.is_some())), style));
    }
    match (&pick.search, pick.choices().get(pick.selected)) {
        (_, None) => spans.push(Span::styled(" no emoji by that name", faded)),
        (None, Some(name)) => spans.push(Span::styled(format!("  {name}"), faded)),
        (Some(_), Some(_)) => {}
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// On the strip the number that picks it and the glyph; in a search the glyph and the name.
/// A custom emoji has no glyph, so its name stands in.
fn choice_label(i: usize, name: &str, searching: bool) -> String {
    match (searching, crate::emoji::glyph(name)) {
        (false, Some(glyph)) => format!("{} {glyph}", i + 1),
        (false, None) => format!("{} :{name}:", i + 1),
        (true, Some(glyph)) => format!("{glyph} {name}"),
        (true, None) => format!(":{name}:"),
    }
}

pub(super) fn draw_palette(f: &mut Frame, app: &App, area: Rect) {
    let Some(palette) = &app.palette else { return };
    let line = Line::from(vec![
        Span::styled(" : ", Style::new().fg(app.theme.accent).bold()),
        Span::raw(palette.input.clone()),
        Span::styled("▌", Style::new().fg(app.theme.accent)),
        Span::styled(app.palette_ghost().unwrap_or_default(), Style::new().fg(app.theme.faded)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::render::text;
    use crate::resolve::NameBook;
    use crate::tui::app::{ChannelRow, Incoming, Kind};
    use crate::tui::field::Field;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use std::collections::HashMap;

    /// Every cell of the input row with its colors, so a test sees what the cursor and the ghost paint.
    fn input_row(app: &mut App) -> Vec<(String, Color, Color)> {
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| crate::tui::ui::draw(f, app)).unwrap();
        let buf = terminal.backend().buffer();
        let row = buf.area.height - 2;
        (0..buf.area.width).map(|x| buf.cell((x, row)).expect("input row")).map(|c| (c.symbol().to_owned(), c.fg, c.bg)).collect()
    }

    fn shown(row: &[(String, Color, Color)]) -> String {
        row.iter().map(|(s, ..)| s.as_str()).collect()
    }

    fn filtering(text: &str) -> App {
        let mut app = App::new();
        app.input = Some(Input::Filter);
        app.buffer = Field::new(text);
        app
    }

    #[test]
    fn the_cursor_paints_the_character_it_sits_on() {
        let mut app = filtering("héllo");
        app.buffer.left();
        app.buffer.left();
        let row = input_row(&mut app);
        let painted: Vec<&String> = row.iter().filter(|(.., bg)| *bg == app.theme.accent).map(|(s, ..)| s).collect();
        assert_eq!(painted, ["l"]);
        assert!(shown(&row).contains("héllo"), "{}", shown(&row));
        assert!(!shown(&row).contains('▌'), "no bar while the cursor covers a character: {}", shown(&row));
    }

    #[test]
    fn the_cursor_is_a_bar_past_the_last_character() {
        let mut app = filtering("hey");
        assert!(shown(&input_row(&mut app)).contains("hey▌"));
    }

    #[test]
    fn a_wide_glyph_under_the_cursor_is_painted_where_it_is_drawn() {
        let mut app = filtering("🚀 ok");
        app.buffer.start();
        let row = input_row(&mut app);
        let painted = row.iter().position(|(.., bg)| *bg == app.theme.accent).expect("cursor painted");
        assert_eq!(row[painted].0, "🚀");
        assert_eq!(painted, text::visible_width(" filter ▸ "));
    }

    #[test]
    fn the_picker_numbers_the_strip_and_names_the_selected_one() {
        let mut app = App::new();
        app.react = Some(crate::tui::app::Pick {
            channel: "C1".into(),
            ts: "1".into(),
            strip: vec!["rocket".into(), "partyparrot".into()],
            selected: 1,
            search: None,
        });
        let row = input_row(&mut app);
        let text = shown(&row);
        assert!(text.starts_with(" react ▸  1 🚀") && text.contains(" 2 :partyparrot:   partyparrot"), "{text}");
        let framed: String = row.iter().filter(|(.., bg)| *bg == app.theme.accent).map(|(s, ..)| s.as_str()).collect();
        assert_eq!(framed, " 2 :partyparrot: ");
    }

    #[test]
    fn the_palette_shows_the_rest_of_the_command_in_grey_after_the_cursor() {
        let mut app = App::new();
        app.apply(Incoming::Channels {
            rows: vec![ChannelRow::new("C1", "#general", Kind::Public)],
            people: vec![],
            names: NameBook::default(),
            badges: HashMap::new(),
            me: "U1".into(),
        });
        for c in ":go #gen".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let row = input_row(&mut app);
        assert!(shown(&row).contains(" : go #gen▌eral"), "{}", shown(&row));
        let grey: String = row.iter().filter(|(_, fg, _)| *fg == app.theme.faded).map(|(s, ..)| s.as_str()).collect();
        assert_eq!(grey, "eral", "only the suggestion is dimmed");
    }
}
