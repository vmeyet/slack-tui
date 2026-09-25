mod channels;
mod confirm;
mod empty;
mod help;
mod input;
mod items;
mod messages;
mod pictures;
mod status;
mod style;
#[cfg(test)]
mod testing;
mod thread;

pub use empty::{Empty, draw_empty};
pub use style::{body_spans, user_style};

use super::app::{App, Focus};
use super::theme::Theme;
use pictures::Placement;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding};

/// Breathing room between the border and the text.
const MODAL_PAD_X: u16 = 2;
const MODAL_PAD_Y: u16 = 1;
/// The border on both sides plus that padding.
const MODAL_FRAME_W: u16 = 2 + 2 * MODAL_PAD_X;
const MODAL_FRAME_H: u16 = 2 + 2 * MODAL_PAD_Y;

pub fn draw(f: &mut Frame, app: &mut App) {
    let input_rows = u16::from(app.input.is_some() || app.palette.is_some() || app.react.is_some());
    let [main, input, status] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(input_rows), Constraint::Length(1)]).areas(f.area());
    let modal = app.inbox.is_some() || app.firehose.is_some() || app.jump.is_some() || app.help || app.pending_delete.is_some();
    let pictures = if app.zen { draw_reading(f, app, main, modal) } else { draw_panes(f, app, main, modal) };
    if !modal {
        pictures::draw(f, app, &pictures);
    }
    if app.input.is_some() {
        input::draw(f, app, input);
    } else if app.palette.is_some() {
        input::draw_palette(f, app, input);
    } else if app.react.is_some() {
        input::draw_react(f, app, input);
    }
    status::draw(f, app, status);
    draw_overlays(f, app, main);
}

/// Channels on the left, the conversation in the middle, the thread on the right when open;
/// every pane but the focused one recedes.
fn draw_panes(f: &mut Frame, app: &mut App, area: Rect, modal: bool) -> Vec<Placement> {
    let thread_w = if app.thread.is_some() { 40 } else { 0 };
    let [left, middle, right] =
        Layout::horizontal([Constraint::Length(26), Constraint::Min(30), Constraint::Percentage(thread_w)]).areas(area);
    channels::draw(f, app, left);
    let mut pictures = messages::draw(f, app, middle);
    if app.thread.is_some() {
        pictures.extend(thread::draw(f, app, right));
    }
    if app.focus != Focus::Channels || modal {
        fade(f, left, app.theme.faded);
    }
    if app.focus != Focus::Messages || modal {
        fade(f, middle, if modal { app.theme.faded } else { app.theme.muted });
    }
    if app.thread.is_some() && (app.focus != Focus::Thread || modal) {
        fade(f, right, app.theme.faded);
    }
    pictures
}

/// Reading mode: one centered column with the thread when open, the conversation otherwise.
/// Three quarters of the terminal, never narrower than 80 columns nor wider than 110.
fn draw_reading(f: &mut Frame, app: &mut App, area: Rect, modal: bool) -> Vec<Placement> {
    let width = (area.width * 3 / 4).clamp(80, 110).min(area.width);
    let column = Rect { x: area.x + (area.width - width) / 2, width, ..area };
    let pictures = if app.thread.is_some() { thread::draw(f, app, column) } else { messages::draw(f, app, column) };
    if modal {
        fade(f, area, app.theme.faded);
    }
    pictures
}

/// The boxes drawn over the panes: inbox, firehose, jump, the delete question and the key help.
fn draw_overlays(f: &mut Frame, app: &mut App, main: Rect) {
    let theme = app.theme;
    let elapsed = app.elapsed();
    if let Some(inbox) = &mut app.inbox {
        super::inbox::draw(f, inbox, &app.names, main, &theme, elapsed);
    }
    if let Some(view) = &mut app.firehose {
        f.render_widget(Clear, main);
        super::firehose::draw(f, view, &app.wall, &app.names, &app.highlighter, main, &theme);
    }
    if let Some(jump) = &mut app.jump {
        super::jump::draw(f, jump, main, &theme);
    }
    if let Some(pending) = &app.pending_delete {
        confirm::draw(f, &theme, &app.names, pending, f.area());
    }
    if app.help {
        help::draw(f, &theme, f.area());
    }
}

/// Quiet rounded frame; focus is carried by the title alone.
pub fn pane(theme: &Theme, title: &str, focused: bool) -> Block<'static> {
    let title = if focused { format!(" {title} ").bold().fg(theme.accent) } else { format!(" {title} ").fg(theme.muted) };
    Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(theme.border)).title(title)
}

/// Reading mode has no frame: just the title, a breath of space, then the text.
fn reading_pane(theme: &Theme, title: &str) -> Block<'static> {
    Block::new().title(format!(" {title}").bold().fg(theme.accent)).padding(Padding::new(1, 1, 1, 0))
}

/// A focused pane with room between its border and the text, for boxes drawn over the panes.
fn modal_pane(theme: &Theme, title: &str) -> Block<'static> {
    pane(theme, title, true).padding(Padding::new(MODAL_PAD_X, MODAL_PAD_X, MODAL_PAD_Y, MODAL_PAD_Y))
}

fn frame(app: &App, title: &str, focused: bool) -> Block<'static> {
    if app.zen { reading_pane(&app.theme, title) } else { pane(&app.theme, title, focused) }
}

/// The selected row carries no fill unless the user asked for one with `highlight`.
pub fn row_highlight(theme: &Theme, focused: bool) -> Style {
    theme.highlight.filter(|_| focused).map(|c| Style::new().bg(c)).unwrap_or_default()
}

/// The selected row's ▎ bar, shown only in the focused pane; the column is always reserved so
/// content never shifts when focus moves.
pub fn cursor_bar(theme: &Theme, focused: bool) -> Line<'static> {
    if focused { Line::from(Span::styled("▎", Style::new().fg(theme.accent))) } else { Line::from(" ") }
}

/// Repaints an area in one quiet grey so a pane recedes when it is not the focus, or when a modal is up.
pub fn fade(f: &mut Frame, area: Rect, color: Color) {
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_fg(color);
                cell.modifier.remove(Modifier::BOLD);
            }
        }
    }
}

/// A box of `width` by `height` cells in the middle of `area`, shrunk to fit when the terminal is smaller.
pub fn centered_cells(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 2, width, height }
}

/// A box taking that percentage of `area`, in the middle of it.
pub fn centered_pct(area: Rect, width: u16, height: u16) -> Rect {
    centered_cells(area, area.width * width / 100, area.height * height / 100)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::testing::{message, render_at};
    use crate::api::Reaction;
    use crate::resolve::NameBook;
    use crate::tui::app::{self, App, ChannelRow, Focus, Incoming, Input, Kind, Thread};
    use crate::tui::field::Field;

    #[test]
    fn full_layout_snapshot() {
        let mut app = App::new();
        let rows = vec![
            ChannelRow::new("C1", "#general", Kind::Public),
            ChannelRow::new("C2", "🔒vault", Kind::Private),
            ChannelRow::new("D1", "@bob", Kind::Dm),
        ];
        app.apply(Incoming::Channels {
            rows,
            people: vec![],
            names: NameBook::default(),
            badges: std::collections::HashMap::from([("D1".to_string(), app::Badge { unread: true, mentions: 3 })]),
            me: "U1".into(),
        });
        app.current_channel = Some("C1".into());
        let mut root = message(
            "1694700000.000100",
            "U1",
            "Deploy *v2* is out, see <https://acme.io|notes>. A long line that needs wrapping inside the pane for sure.",
        );
        root.reply_count = 2;
        root.thread_ts = Some(root.ts.clone());
        root.reactions = vec![Reaction { name: "rocket".into(), count: 3, users: vec!["U1".into()] }];
        app.apply(Incoming::History {
            channel: "C1".into(),
            messages: vec![root.clone(), message("1694700100.000200", "U2", "ok")],
            names: NameBook::default(),
        });
        app.thread = Some(Thread {
            channel: "C1".into(),
            root_ts: root.ts.clone(),
            messages: vec![root, message("1694700050.000300", "U2", "reply")],
            selected: 1,
        });
        app.focus = Focus::Thread;
        app.input = Some(Input::Reply { channel: "C1".into(), thread_ts: None, label: "#general".into() });
        app.buffer = Field::new("typing…");
        let out = render_at(&mut app, 110, 18);
        let stable = regex::Regex::new(r"\d\d:\d\d").unwrap().replace_all(&out, "HH:MM").to_string();
        insta::assert_snapshot!(stable);
    }
}
