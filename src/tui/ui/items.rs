use super::pictures::Slot;
use super::style::{body_spans, user_style};
use crate::api::{File, Message, Reaction};
use crate::render;
use crate::render::text::{Style as TextStyle, Styled};
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

const TIME_W: usize = 5;
/// Two border columns plus the always-reserved cursor-bar column.
pub(super) const BORDERS_AND_CURSOR_W: u16 = 3;

/// The column a message's text starts on: past the time and the name, with a space after each.
pub(super) fn text_start(name_w: usize) -> usize {
    TIME_W + 1 + name_w + 1
}

/// Who is looking at the screen, so their own marks stand out.
pub(super) struct Viewer<'a> {
    names: &'a NameBook,
    me: &'a str,
    theme: &'a Theme,
    thumbs: &'a Thumbs,
    spinner: &'static str,
}

pub(super) fn viewer(app: &App) -> Viewer<'_> {
    Viewer { names: &app.names, me: &app.me, theme: &app.theme, thumbs: &app.thumbs, spinner: motion::spinner(app.elapsed()) }
}

/// How one message sits in its list.
#[derive(Clone, Copy)]
struct Row {
    /// Time and author on the first line; continuations drop it, the selected row always shows it.
    header: bool,
    new_day: bool,
    gap: bool,
    show_time: bool,
    selected: bool,
}

/// One list item per message, with a dim day line on the first message of each day and
/// no header on messages that continue the previous one. The selected message always shows
/// its header, with the time in accent, so the cursor bar is never the only mark.
/// `zen`: reading mode shows the time on the selected row alone, like a hover.
pub(super) fn grouped_items(
    viewer: &Viewer,
    messages: &[Message],
    width: usize,
    name_w: usize,
    show_meta: bool,
    selected: usize,
    zen: bool,
) -> Vec<(ListItem<'static>, Vec<Slot>)> {
    let indent = text_start(name_w);
    let mut items = Vec::with_capacity(messages.len());
    let mut prev: Option<&Message> = None;
    let mut last_day = String::new();
    let mut prev_blank_row = false;
    for (i, m) in messages.iter().enumerate() {
        let day = time::day_label(&m.ts);
        let new_day = day != last_day;
        last_day = day;
        let continued = render::continues(prev, m);
        let body = body(viewer, m, width.saturating_sub(indent) as u16);
        let gap = prev.is_some() && !prev_blank_row && (new_day || !continued);
        let row = Row { header: !continued || i == selected, new_day, gap, show_time: !zen || i == selected, selected: i == selected };
        prev_blank_row = ends_on_a_blank_row(m, &body, show_meta);
        items.push(message_item(viewer, m, width, name_w, show_meta, row, &body));
        prev = Some(m);
    }
    items
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
    let author = m.user.as_deref().map(|u| viewer.names.user_label(u)).or_else(|| m.username.clone()).unwrap_or_else(|| "bot".into());
    let indent = text_start(name_w);
    let mut lines: Vec<Line> = Vec::new();
    if row.gap {
        lines.push(Line::raw(""));
    }
    if row.new_day {
        let label = time::day_label(&m.ts);
        let dashes = "─".repeat(width.saturating_sub(label.len() + 4));
        lines.push(Line::from(vec![
            Span::styled("── ", Style::new().fg(theme.border)),
            Span::styled(label, Style::new().fg(theme.faded)),
            Span::styled(format!(" {dashes}"), Style::new().fg(theme.border)),
        ]));
    }
    let time_style = if row.selected { Style::new().fg(theme.accent).bold() } else { Style::new().fg(theme.muted) };
    for (i, chunks) in body.styled.wrap_styled(width.saturating_sub(indent).max(10)).iter().enumerate() {
        let mut spans = if i == 0 && row.header {
            vec![
                Span::styled(if row.show_time { time::hhmm(&m.ts) } else { " ".repeat(TIME_W) }, time_style),
                Span::raw(" "),
                Span::styled(render::fit_right(&author, name_w), user_style(theme, &author)),
                Span::raw(" "),
            ]
        } else {
            vec![Span::raw(" ".repeat(indent))]
        };
        spans.extend(body_spans(theme, chunks, width.saturating_sub(indent)));
        lines.push(Line::from(spans));
    }
    let mut slots = Vec::new();
    for &(file, size) in &body.pictures {
        let note = match viewer.thumbs.get(&file.id) {
            Some(Thumb::Ready(_)) => String::new(),
            Some(Thumb::Failed) => "image unavailable".to_owned(),
            _ => format!("{} loading image", viewer.spinner),
        };
        slots.push(Slot { line: lines.len(), file: file.id.clone(), size });
        for r in 0..size.height {
            let text = if r == 0 { note.clone() } else { String::new() };
            lines.push(Line::from(vec![Span::raw(" ".repeat(indent)), Span::styled(text, Style::new().fg(theme.faded))]));
        }
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(format!("📎 {}", file.label()), Style::new().fg(theme.faded)),
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

/// What one message shows: its text with a 📎 mark per attachment shown inline, and the files
/// drawn as pictures instead, each with the cells it takes. Built once per message per frame.
struct Body<'a> {
    styled: Styled,
    pictures: Vec<(&'a File, Size)>,
}

fn body<'a>(viewer: &Viewer, m: &'a Message, text_w: u16) -> Body<'a> {
    let mut styled = render::body(viewer.names, m, false);
    let mut pictures = Vec::new();
    for file in &m.files {
        match viewer.thumbs.cells(file, text_w) {
            Some(size) => pictures.push((file, size)),
            None => styled.push_dim(&format!(" 📎 {}", file.label())),
        }
    }
    Body { styled, pictures }
}

/// A code block ends on a blank row, so the next message needs no gap — unless a picture,
/// the reactions or the reply count were drawn under it.
fn ends_on_a_blank_row(m: &Message, body: &Body, show_meta: bool) -> bool {
    let drawn_under = !body.pictures.is_empty() || !m.reactions.is_empty() || (show_meta && m.is_thread_root());
    !drawn_under && body.styled.spans.last().is_some_and(|p| p.style == TextStyle::Block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

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
