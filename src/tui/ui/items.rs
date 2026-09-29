use super::pictures::Slot;
use super::style::{body_spans, user_style};
use crate::api::{Edited, Message, Reaction};
use crate::render;
use crate::render::text::{Piece, Style as TextStyle};
use crate::render::time;
use crate::resolve::NameBook;
use crate::tui::app::App;
use crate::tui::images::{Thumb, Thumbs};
use crate::tui::motion;
use crate::tui::theme::Theme;
use ratatui::layout::Size;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;
use std::collections::HashMap;

const TIME_W: usize = 5;
/// Two border columns plus the always-reserved cursor-bar column.
pub(super) const BORDERS_AND_CURSOR_W: u16 = 3;

/// The column a message's text starts on: past the time and the name, with a space after each.
pub(super) fn text_start(name_w: usize) -> usize {
    TIME_W + 1 + name_w + 1
}

/// Who is looking at the screen, so their own marks stand out.
/// `zen`: reading mode shows the time on the selected row alone, like a hover.
pub(super) struct Viewer<'a> {
    names: &'a NameBook,
    me: &'a str,
    theme: &'a Theme,
    thumbs: &'a Thumbs,
    spinner: &'static str,
    zen: bool,
}

pub(super) fn viewer(app: &App) -> Viewer<'_> {
    Viewer { names: &app.names, me: &app.me, theme: &app.theme, thumbs: &app.thumbs, spinner: motion::spinner(app.elapsed()), zen: app.zen }
}

/// How one message sits in its list.
#[derive(Clone, Copy)]
struct Row<'a> {
    /// Time and author on the first line; continuations drop it, the selected row always shows it.
    header: bool,
    /// The day line drawn above the first message of each day.
    day: Option<&'a str>,
    gap: bool,
    show_time: bool,
    selected: bool,
}

/// One list item per message, with a dim day line on the first message of each day and
/// no header on messages that continue the previous one. The selected message always shows
/// its header, with the time in accent, so the cursor bar is never the only mark.
/// Takes the pane's bodies from the last frame and hands back this frame's.
pub(super) fn grouped_items(
    viewer: &Viewer,
    messages: &[Message],
    bodies: Bodies,
    width: usize,
    name_w: usize,
    show_meta: bool,
    selected: usize,
) -> (Vec<(ListItem<'static>, Vec<Slot>)>, Bodies) {
    let bodies = bodies.refresh(viewer, messages, width.saturating_sub(text_start(name_w)));
    let mut items = Vec::with_capacity(messages.len());
    let mut prev: Option<&Message> = None;
    let mut last_day = String::new();
    let mut prev_blank_row = false;
    for (i, m) in messages.iter().enumerate() {
        let body = bodies.of(m);
        let day = time::day_label(&m.ts);
        let new_day = day != last_day;
        let continued = !new_day && prev.is_some_and(|p| render::same_run(p, m));
        let gap = prev.is_some() && !prev_blank_row && (new_day || !continued);
        let row = Row {
            header: !continued || i == selected,
            day: new_day.then_some(day.as_str()),
            gap,
            show_time: !viewer.zen || i == selected,
            selected: i == selected,
        };
        prev_blank_row = ends_on_a_blank_row(m, body, show_meta);
        items.push(message_item(viewer, m, width, name_w, show_meta, row, body));
        last_day = day;
        prev = Some(m);
    }
    (items, bodies)
}

fn message_item(
    viewer: &Viewer,
    m: &Message,
    width: usize,
    name_w: usize,
    show_meta: bool,
    row: Row,
    body: &Body,
) -> (ListItem<'static>, Vec<Slot>) {
    let theme = viewer.theme;
    let author = &body.author;
    let indent = text_start(name_w);
    let mut lines: Vec<Line> = Vec::new();
    if row.gap {
        lines.push(Line::raw(""));
    }
    if let Some(label) = row.day {
        let dashes = "─".repeat(width.saturating_sub(label.len() + 4));
        lines.push(Line::from(vec![
            Span::styled("── ", Style::new().fg(theme.border)),
            Span::styled(label.to_owned(), Style::new().fg(theme.faded)),
            Span::styled(format!(" {dashes}"), Style::new().fg(theme.border)),
        ]));
    }
    let time_style = if row.selected { Style::new().fg(theme.accent).bold() } else { Style::new().fg(theme.muted) };
    for (i, chunks) in body.lines.iter().enumerate() {
        let mut spans = if i == 0 && row.header {
            vec![
                Span::styled(if row.show_time { body.time.clone() } else { " ".repeat(TIME_W) }, time_style),
                Span::raw(" "),
                Span::styled(render::fit_right(author, name_w), user_style(theme, author)),
                Span::raw(" "),
            ]
        } else {
            vec![Span::raw(" ".repeat(indent))]
        };
        spans.extend(body_spans(theme, chunks, width.saturating_sub(indent)));
        lines.push(Line::from(spans));
    }
    let mut slots = Vec::new();
    for picture in &body.pictures {
        let note = match viewer.thumbs.get(&picture.file) {
            Some(Thumb::Ready(_)) => String::new(),
            Some(Thumb::Failed) => "image unavailable".to_owned(),
            _ => format!("{} loading image", viewer.spinner),
        };
        slots.push(Slot { line: lines.len(), file: picture.file.clone(), size: picture.size });
        for r in 0..picture.size.height {
            let text = if r == 0 { note.clone() } else { String::new() };
            lines.push(Line::from(vec![Span::raw(" ".repeat(indent)), Span::styled(text, Style::new().fg(theme.faded))]));
        }
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(format!("📎 {}", picture.label), Style::new().fg(theme.faded)),
        ]));
    }
    if !m.reactions.is_empty() {
        let mut spans = vec![Span::raw(" ".repeat(indent))];
        spans.extend(reaction_pills(theme, &m.reactions, viewer.me));
        lines.push(Line::from(spans));
    }
    if show_meta && m.is_thread_root() {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(format!("↳ {} replies", m.reply_count), Style::new().fg(theme.accent)),
        ]));
    }
    (ListItem::new(lines), slots)
}

/// Reactions as a quiet dim row; the ones you joined stand out in bold accent.
/// Custom emoji have no glyph and show their bare name.
fn reaction_pills(theme: &Theme, reactions: &[Reaction], me: &str) -> Vec<Span<'static>> {
    let pill = |r: &Reaction| {
        let mine = !me.is_empty() && r.users.iter().any(|u| u == me);
        let style = if mine { Style::new().fg(theme.accent).bold() } else { Style::new().fg(theme.muted) };
        let glyph = crate::emoji::glyph(&r.name).map_or_else(|| r.name.clone(), str::to_owned);
        Span::styled(format!("{glyph} {}", r.count), style)
    };
    let mut spans = Vec::with_capacity(reactions.len() * 2);
    for r in reactions {
        if !spans.is_empty() {
            spans.push(Span::raw("   "));
        }
        spans.push(pill(r));
    }
    spans
}

/// Each message's parsed and wrapped text from the last frame, so a frame only redoes what changed.
#[derive(Debug, Default)]
pub struct Bodies {
    names: NameBook,
    by_ts: HashMap<String, Body>,
}

impl Bodies {
    /// Keeps the bodies still true to their message and pane width, builds the missing ones and drops the rest.
    fn refresh(self, viewer: &Viewer, messages: &[Message], text_w: usize) -> Bodies {
        let mut kept = if self.names.same(viewer.names) { self.by_ts } else { HashMap::new() };
        let by_ts = messages
            .iter()
            .map(|m| {
                let body = kept.remove(&m.ts).filter(|b| b.fits(m, text_w)).unwrap_or_else(|| Body::new(viewer, m, text_w));
                (m.ts.clone(), body)
            })
            .collect();
        Bodies { names: viewer.names.clone(), by_ts }
    }

    #[allow(clippy::expect_used)]
    fn of(&self, m: &Message) -> &Body {
        self.by_ts.get(&m.ts).expect("refreshed for every message")
    }
}

/// What one message shows: its text wrapped to the pane with a 📎 mark per attachment shown inline,
/// and the files drawn as pictures instead, each with the cells it takes.
#[derive(Debug)]
struct Body {
    text: String,
    edited: Option<Edited>,
    text_w: usize,
    time: String,
    author: String,
    lines: Vec<Vec<Piece>>,
    pictures: Vec<Picture>,
    ends_in_code: bool,
}

#[derive(Debug)]
struct Picture {
    file: String,
    label: String,
    size: Size,
}

impl Body {
    fn new(viewer: &Viewer, m: &Message, text_w: usize) -> Body {
        let mut styled = render::body(viewer.names, m, false);
        let mut pictures = Vec::new();
        for file in &m.files {
            match viewer.thumbs.cells(file, text_w as u16) {
                Some(size) => pictures.push(Picture { file: file.id.clone(), label: file.label().to_owned(), size }),
                None => styled.push_dim(&format!(" 📎 {}", file.label())),
            }
        }
        Body {
            text: m.text.clone(),
            edited: m.edited.clone(),
            text_w,
            time: time::hhmm(&m.ts),
            author: m.user.as_deref().map(|u| viewer.names.user_label(u)).or_else(|| m.username.clone()).unwrap_or_else(|| "bot".into()),
            lines: styled.wrap_styled(text_w.max(10)),
            pictures,
            ends_in_code: styled.spans.last().is_some_and(|p| p.style == TextStyle::Block),
        }
    }

    /// An edit only ever changes the text and the edited mark; the rest of a message stays put.
    fn fits(&self, m: &Message, text_w: usize) -> bool {
        self.text_w == text_w && self.text == m.text && self.edited == m.edited
    }
}

/// A code block ends on a blank row, so the next message needs no gap — unless a picture,
/// the reactions or the reply count were drawn under it.
fn ends_on_a_blank_row(m: &Message, body: &Body, show_meta: bool) -> bool {
    let drawn_under = !body.pictures.is_empty() || !m.reactions.is_empty() || (show_meta && m.is_thread_root());
    !drawn_under && body.ends_in_code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::ui::testing::message;
    use ratatui::style::Modifier;

    fn refresh(app: &App, bodies: Bodies, m: &Message, text_w: usize) -> Bodies {
        bodies.refresh(&viewer(app), std::slice::from_ref(m), text_w)
    }

    fn rows(bodies: &Bodies, m: &Message) -> Vec<String> {
        bodies.of(m).lines.iter().map(|row| row.iter().map(|p| p.text.as_str()).collect()).collect()
    }

    #[test]
    fn a_body_is_kept_until_the_names_change() {
        let mut app = App::new();
        let m = message("1694700000.000100", "U1", "ship it");
        let bodies = refresh(&app, Bodies::default(), &m, 40);
        let bot = Message { user: None, username: Some("deploybot".into()), ..m.clone() };
        let bodies = refresh(&app, bodies, &bot, 40);
        assert_eq!(bodies.of(&bot).author, "U1", "nothing the body depends on changed");
        app.names = NameBook::default();
        assert_eq!(refresh(&app, bodies, &bot, 40).of(&bot).author, "deploybot");
    }

    #[test]
    fn an_edit_or_a_new_width_rebuilds_the_body() {
        let app = App::new();
        let m = message("1694700000.000100", "U1", "ship it now");
        let bodies = refresh(&app, Bodies::default(), &m, 40);
        assert_eq!(rows(&bodies, &m), ["ship it now"]);
        let edited = Message { text: "ship it later".into(), edited: Some(Edited::default()), ..m.clone() };
        let bodies = refresh(&app, bodies, &edited, 40);
        assert_eq!(rows(&bodies, &edited), ["ship it later (edited)"]);
        let bodies = refresh(&app, bodies, &edited, 12);
        assert_eq!(rows(&bodies, &edited), ["ship it", "later", "(edited)"]);
    }

    #[test]
    fn a_body_leaves_with_its_message() {
        let app = App::new();
        let m = message("1694700000.000100", "U1", "ship it");
        let bodies = refresh(&app, Bodies::default(), &m, 40);
        assert!(bodies.refresh(&viewer(&app), &[], 40).by_ts.is_empty());
    }

    #[test]
    fn reaction_pills_glow_when_mine() {
        let reactions = vec![
            Reaction { name: "rocket".into(), count: 3, users: vec!["U1".into(), "U2".into()] },
            Reaction { name: "merged".into(), count: 1, users: vec!["U2".into()] },
        ];
        let theme = Theme::default();
        let pills = reaction_pills(&theme, &reactions, "U1");
        let texts: Vec<&str> = pills.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["🚀 3", "   ", "merged 1"]);
        assert!(pills[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(pills[0].style.fg, Some(theme.accent));
        assert!(!pills[2].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(pills[2].style.fg, Some(theme.muted));
        assert!(pills.iter().all(|s| s.style.bg.is_none()));
        assert!(reaction_pills(&theme, &reactions, "").iter().all(|s| !s.style.add_modifier.contains(Modifier::BOLD)));
    }
}
