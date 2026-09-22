use super::empty::{Empty, draw_empty};
use super::items::{BORDERS_AND_CURSOR_W, grouped_items, text_start, viewer};
use super::pictures::{self, Placement, Slot};
use super::style::{body_spans, user_style};
use super::{cursor_bar, frame, row_highlight};
use crate::api::SearchMatch;
use crate::mrkdwn;
use crate::render::text;
use crate::render::time;
use crate::tui::app::{App, Focus, Kind};
use crate::tui::motion;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, HighlightSpacing, List, ListItem};

const NAME_W: usize = 12;

pub(super) fn draw(f: &mut Frame, app: &mut App, area: Rect) -> Vec<Placement> {
    let focused = app.focus == Focus::Messages;
    let mut title = if app.search.is_some() {
        "search".to_owned()
    } else if app.current_channel.is_some() {
        app.current_label()
    } else {
        "messages".to_owned()
    };
    if app.loading {
        title.push_str(&format!(" {} loading", motion::spinner(app.elapsed())));
    }
    let width = area.width.saturating_sub(BORDERS_AND_CURSOR_W) as usize;
    let (items, slots): (Vec<ListItem>, Vec<Vec<Slot>>) = match &app.search {
        Some(results) => results.iter().map(|m| (search_item(&app.theme, m, width), vec![])).unzip(),
        None => grouped_items(&viewer(app), &app.messages, width, NAME_W, true, app.message_selected, app.zen).into_iter().unzip(),
    };
    let rows: Vec<(usize, Vec<Slot>)> = items.iter().map(ListItem::height).zip(slots).collect();
    let empty = items.is_empty();
    let pills = [typing_pill(app), new_below_pill(app)];
    let block = pills.into_iter().flatten().fold(frame(app, &title, focused), Block::title_bottom);
    let inner = block.inner(area);
    let list = List::new(items)
        .block(block)
        .highlight_style(row_highlight(&app.theme, focused))
        .highlight_symbol(cursor_bar(&app.theme, focused))
        .repeat_highlight_symbol(true)
        .highlight_spacing(HighlightSpacing::Always);
    app.messages_view.select((!empty).then_some(app.message_selected));
    f.render_stateful_widget(list, area, &mut app.messages_view);
    if let Some(state) = empty_state(app) {
        draw_empty(f, &app.theme, inner, motion::frame(app.elapsed()), &state);
    }
    let x = inner.x + 1 + text_start(NAME_W) as u16;
    pictures::placements(inner, x, app.messages_view.offset(), &rows)
}

/// Sits on the bottom border, under the last message, while someone types in the open conversation.
fn typing_pill(app: &App) -> Option<Line<'static>> {
    let line = app.typing_line()?;
    Some(Line::from(format!(" {line} ").fg(app.theme.faded)))
}

/// Sits on the bottom border while messages wait below the selection.
fn new_below_pill(app: &App) -> Option<Line<'static>> {
    let waiting = Some(app.new_below()).filter(|n| *n > 0)?;
    Some(Line::from(format!(" ↓ {waiting} new ").bold().fg(app.theme.accent)).right_aligned())
}

fn empty_state(app: &App) -> Option<Empty> {
    if app.loading {
        return None;
    }
    let state = match (&app.search, app.current_kind()) {
        (Some(results), _) if results.is_empty() => {
            Empty { title: "no results".into(), hint: "try other words with".into(), key: Some("s") }
        }
        (Some(_), _) => return None,
        _ if !app.messages.is_empty() => return None,
        (None, None) => Empty { title: "pick a conversation".into(), hint: "on the left, then".into(), key: Some("enter") },
        (None, Some(Kind::Dm | Kind::GroupDm)) => {
            Empty { title: "nothing here yet".into(), hint: format!("say hi to {} with", app.current_label()), key: Some("r") }
        }
        (None, Some(_)) => Empty {
            title: "nothing here yet".into(),
            hint: format!("be the first to post in {} with", app.current_label()),
            key: Some("r"),
        },
    };
    Some(state)
}

fn search_item(theme: &Theme, m: &SearchMatch, width: usize) -> ListItem<'static> {
    let head = Line::from(vec![
        Span::styled(format!("#{}", m.channel.name), Style::new().fg(theme.accent)),
        Span::raw(" · "),
        Span::styled(format!("{} {}", time::day_label(&m.ts), time::hhmm(&m.ts)), Style::new().fg(theme.muted)),
        Span::raw("  "),
        Span::styled(m.username.clone(), user_style(theme, &m.username)),
    ]);
    let mut lines = vec![head];
    let styled = text::from_segments(&mrkdwn::parse(&m.text, &mrkdwn::NoNames), false);
    for chunks in styled.wrap_styled(width.saturating_sub(4).max(10)).into_iter().take(3) {
        let mut spans = vec![Span::raw("    ")];
        spans.extend(body_spans(theme, &chunks, width.saturating_sub(4)));
        lines.push(Line::from(spans));
    }
    ListItem::new(lines)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::api::Message;
    use crate::resolve::NameBook;
    use crate::tui::app::Incoming;
    use crate::tui::ui::testing::{message, render};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn loading_title_spins_with_the_clock() {
        let mut app = App::new();
        assert!(render(&mut app).contains("messages ⠋ loading"));
        app.now += motion::SPINNER_FRAME;
        assert!(render(&mut app).contains("messages ⠙ loading"));
    }

    #[test]
    fn new_messages_below_the_selection_show_on_the_bottom_border() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        let history =
            |messages: &[Message]| Incoming::History { channel: "C1".into(), messages: messages.to_vec(), names: NameBook::default() };
        let messages = [
            message("1694700000.000100", "U1", "first"),
            message("1694700010.000100", "U2", "second"),
            message("1694700020.000100", "U2", "third"),
        ];
        app.apply(history(&messages[..2]));
        app.message_selected = 0;
        app.apply(history(&messages));
        let out = render(&mut app);
        assert!(out.lines().any(|l| l.contains('╰') && l.contains("↓ 1 new")), "{out}");
        app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
        assert!(!render(&mut app).contains("new"));
    }

    fn watching_c1() -> App {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        let messages = vec![message("1694700000.000100", "U1", "first")];
        app.apply(Incoming::History { channel: "C1".into(), messages, names: NameBook::default() });
        app
    }

    fn typing(app: &mut App, user: &str) {
        app.apply(Incoming::Live(Box::new(crate::api::rtm::Event::Typing { channel: "C1".into(), user: user.into() })));
    }

    #[test]
    fn a_typist_shows_on_the_bottom_border_under_the_last_message() {
        let mut app = watching_c1();
        typing(&mut app, "U2");
        let out = render(&mut app);
        assert!(out.lines().any(|l| l.contains('╰') && l.contains("U2 is typing···")), "{out}");
        app.now += std::time::Duration::from_secs(5);
        assert!(!render(&mut app).contains("typing"));
    }

    #[test]
    fn an_expired_typist_stops_waking_the_loop() {
        let mut app = watching_c1();
        assert_eq!(app.redraw_in(), None);
        typing(&mut app, "U2");
        assert_eq!(app.redraw_in(), Some(motion::FRAME));
        app.now += std::time::Duration::from_secs(5);
        assert_eq!(app.redraw_in(), None);
    }

    #[test]
    fn selected_message_shows_its_header_even_when_it_continues_the_previous_one() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        let messages = vec![message("1694700000.000100", "U1", "first"), message("1694700010.000100", "U1", "second")];
        app.apply(Incoming::History { channel: "C1".into(), messages, names: NameBook::default() });
        app.message_selected = 0;
        assert_eq!(render(&mut app).matches("U1").count(), 1);
        app.message_selected = 1;
        assert_eq!(render(&mut app).matches("U1").count(), 2);
    }

    #[test]
    fn a_message_is_read_from_its_blocks_not_from_the_flat_text() {
        let sent = crate::markdown::to_blocks("ship it\n```\nls -la\n```", &crate::markdown::NoMentions);
        let show = |blocks: Vec<crate::blocks::Block>| {
            let mut app = App::new();
            app.current_channel = Some("C1".into());
            app.focus = Focus::Messages;
            let m = Message { blocks, ..message("1694700000.000100", "U1", &sent.text) };
            app.apply(Incoming::History { channel: "C1".into(), messages: vec![m], names: NameBook::default() });
            render(&mut app)
        };
        let rich = show(serde_json::from_value(serde_json::json!(sent.blocks)).unwrap());
        assert!(rich.contains("ship it") && rich.contains("▎ ls -la"), "{rich}");
        assert!(!show(vec![]).contains("▎ ls -la"), "the flat text has no code block to show");
    }

    #[test]
    fn an_edited_message_carries_its_mark_and_a_bot_post_reads_its_attachment() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        let edited = Message { edited: Some(crate::api::Edited::default()), ..message("1694700000.000100", "U1", "ship it") };
        let attachment = crate::api::Attachment { title: "Build".into(), text: String::new(), fallback: "deploy failed".into() };
        let bot = Message { attachments: vec![attachment], ..message("1694700010.000100", "U2", "") };
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![edited, bot], names: NameBook::default() });
        let out = render(&mut app);
        assert!(out.contains("ship it (edited)"), "{out}");
        assert!(out.contains("deploy failed"), "{out}");
    }
}
