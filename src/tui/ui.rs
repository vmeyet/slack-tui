use super::app::{self, App, Focus, Input, Kind, Live, SidebarRow};
use super::images::{Thumb, Thumbs};
use super::motion;
use super::theme::Theme;
use crate::api::File;
use crate::api::{Message, Reaction, SearchMatch};
use crate::mrkdwn;
use crate::render;
use crate::render::text::{self, Style as TextStyle, Styled};
use crate::render::time;
use crate::resolve::NameBook;
use ratatui::Frame;
use ratatui::layout::Size;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, HighlightSpacing, List, ListItem, Padding, Paragraph, Wrap};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};
use unicode_width::UnicodeWidthStr;

const TIME_W: usize = 5;
/// Two border columns plus the always-reserved cursor-bar column.
const BORDERS_AND_CURSOR_W: u16 = 3;
const NAME_W: usize = 12;
const THREAD_NAME_W: usize = 8;

pub fn draw(f: &mut Frame, app: &mut App) {
    let input_rows = u16::from(app.input.is_some() || app.palette.is_some());
    let [main, input, status] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(input_rows), Constraint::Length(1)]).areas(f.area());
    let modal = app.inbox.is_some() || app.firehose.is_some() || app.jump.is_some() || app.help;
    let mut pictures = Vec::new();
    if app.zen {
        pictures = draw_reading(f, app, main);
        if modal {
            fade(f, main, app.theme.faded);
        }
    } else {
        let thread_w = if app.thread.is_some() { 40 } else { 0 };
        let [left, middle, right] =
            Layout::horizontal([Constraint::Length(26), Constraint::Min(30), Constraint::Percentage(thread_w)]).areas(main);
        draw_channels(f, app, left);
        pictures.extend(draw_messages(f, app, middle));
        if app.thread.is_some() {
            pictures.extend(draw_thread(f, app, right));
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
    }
    if !modal {
        draw_pictures(f, app, &pictures);
    }
    if app.input.is_some() {
        draw_input(f, app, input);
    } else if app.palette.is_some() {
        draw_palette(f, app, input);
    }
    draw_status(f, app, status);
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
    if app.help {
        draw_help(f, &theme, f.area());
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

fn frame(app: &App, title: &str, focused: bool) -> Block<'static> {
    if app.zen { reading_pane(&app.theme, title) } else { pane(&app.theme, title, focused) }
}

fn draw_channels(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Channels;
    let visible = app.visible_channels();
    let rows = app::sidebar_rows(&visible, app.filter.is_empty());
    let inner_w = area.width.saturating_sub(5) as usize;
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            SidebarRow::Spacer => ListItem::new(Line::raw("")),
            SidebarRow::Header(name) => {
                ListItem::new(Line::from(Span::styled(format!(" {name}"), Style::new().fg(app.theme.faded).add_modifier(Modifier::BOLD))))
            }
            SidebarRow::Channel(i) => channel_row(app, visible[*i], inner_w),
        })
        .collect();
    let title = if app.filter.is_empty() { "channels".to_owned() } else { format!("channels /{}", app.filter) };
    let list = List::new(items)
        .block(pane(&app.theme, &title, focused))
        .highlight_style(row_highlight(&app.theme, focused))
        .highlight_symbol(cursor_bar(&app.theme, focused))
        .repeat_highlight_symbol(true)
        .highlight_spacing(HighlightSpacing::Always);
    let list_index = rows.iter().position(|r| *r == SidebarRow::Channel(app.channel_selected));
    app.channels_view.select(list_index);
    f.render_stateful_widget(list, area, &mut app.channels_view);
}

fn channel_row(app: &App, c: &app::ChannelRow, width: usize) -> ListItem<'static> {
    let current = app.current_channel.as_deref() == Some(&c.id);
    let badge = app.badges.get(&c.id).copied().unwrap_or_default();
    let unread = badge.unread || app.unread.contains(&c.id);
    let badge_text = match badge.mentions {
        0 if unread && !c.muted => "●".to_owned(),
        0 => String::new(),
        n => format!("● {n}"),
    };
    let theme = &app.theme;
    let mut style = match c.kind {
        Kind::Public => Style::new(),
        Kind::Private => Style::new().fg(theme.warn),
        Kind::Dm => Style::new().fg(theme.mention),
        Kind::GroupDm => Style::new().fg(theme.link),
    };
    if c.muted {
        style = Style::new().fg(theme.faded);
    } else if current || unread {
        style = style.add_modifier(Modifier::BOLD);
    }
    let label_w = width.saturating_sub(badge_text.width() + if badge_text.is_empty() { 0 } else { 1 });
    let label = text::visible_fit(&c.label, label_w);
    let badge_style = if badge.mentions > 0 { Style::new().fg(theme.accent).bold() } else { Style::new().fg(theme.muted) };
    ListItem::new(Line::from(vec![
        Span::raw(" "),
        Span::styled(label, style),
        Span::raw(if badge_text.is_empty() { "" } else { " " }),
        Span::styled(badge_text, badge_style),
    ]))
}

/// The selected row carries no fill unless the user asked for one with `highlight`.
pub fn row_highlight(theme: &Theme, focused: bool) -> Style {
    theme.highlight.filter(|_| focused).map(|c| Style::new().bg(c)).unwrap_or_default()
}

/// Reading mode: one centered column with the thread when open, the conversation otherwise.
/// Three quarters of the terminal, never narrower than 80 columns nor wider than 110.
fn draw_reading(f: &mut Frame, app: &mut App, area: Rect) -> Vec<Placement> {
    let width = (area.width * 3 / 4).clamp(80, 110).min(area.width);
    let column = Rect { x: area.x + (area.width - width) / 2, width, ..area };
    if app.thread.is_some() { draw_thread(f, app, column) } else { draw_messages(f, app, column) }
}

/// Where a picture lands on screen this frame.
pub struct Placement {
    file: String,
    area: Rect,
}

/// Pictures go on last, after the panes and their fades: the terminal paints them as pixels
/// or placeholder cells, and both must stay exactly as the protocol wrote them.
fn draw_pictures(f: &mut Frame, app: &mut App, pictures: &[Placement]) {
    for p in pictures {
        if let Some(Thumb::Ready(protocol)) = app.thumbs.get_mut(&p.file) {
            let widget = StatefulImage::<StatefulProtocol>::default().resize(Resize::Fit(Some(image::imageops::FilterType::Triangle)));
            f.render_stateful_widget(widget, p.area, protocol);
        }
    }
}

/// Rows a message item reserved for a picture, counted from the item's first line.
struct Slot {
    line: usize,
    file: String,
    size: Size,
}

/// Screen areas of the reserved rows that are fully visible, walking items from the list's
/// scroll offset; a picture cut by the pane edge is skipped rather than squeezed.
fn placements(inner: Rect, x: u16, offset: usize, rows: &[(usize, Vec<Slot>)]) -> Vec<Placement> {
    let mut out = Vec::new();
    let mut y = usize::from(inner.y);
    let bottom = usize::from(inner.bottom());
    for (height, slots) in rows.iter().skip(offset) {
        for slot in slots {
            let top = y + slot.line;
            if top + usize::from(slot.size.height) <= bottom {
                let width = slot.size.width.min(inner.right().saturating_sub(x));
                out.push(Placement { file: slot.file.clone(), area: Rect::new(x, top as u16, width, slot.size.height) });
            }
        }
        y += height;
        if y >= bottom {
            break;
        }
    }
    out
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

fn draw_messages(f: &mut Frame, app: &mut App, area: Rect) -> Vec<Placement> {
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
    let block = match new_below_pill(app) {
        Some(pill) => frame(app, &title, focused).title_bottom(pill),
        None => frame(app, &title, focused),
    };
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
    let x = inner.x + 1 + (TIME_W + 1 + NAME_W + 1) as u16;
    placements(inner, x, app.messages_view.offset(), &rows)
}

/// Sits on the bottom border while messages wait below the selection.
fn new_below_pill(app: &App) -> Option<Line<'static>> {
    let waiting = Some(app.new_below()).filter(|n| *n > 0)?;
    Some(Line::from(format!(" ↓ {waiting} new ").bold().fg(app.theme.accent)).right_aligned())
}

/// The caption under the ghost, chosen from what the pane is showing.
pub struct Empty {
    pub title: String,
    pub hint: String,
    pub key: Option<&'static str>,
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

const GHOST_H: u16 = 4;
/// Ghost, its trail, a blank line, then the two caption lines.
const EMPTY_H: u16 = GHOST_H + 3;
const EMPTY_MIN_W: u16 = 28;

/// Every row is the same width so the face never drifts from the hem. Blinks once in a while.
pub fn ghost(frame: u32) -> [&'static str; GHOST_H as usize] {
    let eyes = if frame % 16 == 14 { "│ ─ ─ │" } else { "│ ◠ ◠ │" };
    ["╭─────╮", eyes, "│  ‿  │", "╰┬┬┬┬┬╯"]
}

/// Up for four frames, down for four: a slow float.
fn floating(frame: u32) -> bool {
    (frame / 4) % 2 == 1
}

/// A small ghost centered in the pane with a two-line caption; only the caption when the pane
/// is too small for it to breathe.
pub fn draw_empty(f: &mut Frame, theme: &Theme, area: Rect, frame: u32, state: &Empty) {
    let full = area.height >= EMPTY_H + 2 && area.width >= EMPTY_MIN_W;
    let mut lines: Vec<Line> = Vec::new();
    if full {
        let up = floating(frame);
        let tone = if up { theme.muted } else { theme.faded };
        if !up {
            lines.push(Line::raw(""));
        }
        lines.extend(ghost(frame).into_iter().map(|row| Line::from(Span::styled(row, Style::new().fg(tone)))));
        if up {
            lines.push(Line::from(Span::styled("·   ·", Style::new().fg(theme.faded))));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(Span::styled(state.title.clone(), Style::new().fg(theme.muted).bold())));
    let mut hint = vec![Span::styled(state.hint.clone(), Style::new().fg(theme.muted))];
    if let Some(key) = state.key {
        hint.push(Span::raw("  "));
        hint.push(Span::styled(key, Style::new().fg(theme.accent).bold()));
    }
    lines.push(Line::from(hint));
    let height = lines.len() as u16;
    let top = area.y + area.height.saturating_sub(height) / 2;
    let slot = Rect { y: top, height: height.min(area.height), ..area };
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), slot);
}

fn draw_thread(f: &mut Frame, app: &mut App, area: Rect) -> Vec<Placement> {
    let focused = app.focus == Focus::Thread;
    let Some(thread) = &app.thread else { return vec![] };
    let width = area.width.saturating_sub(BORDERS_AND_CURSOR_W) as usize;
    let (items, slots): (Vec<ListItem>, Vec<Vec<Slot>>) =
        grouped_items(&viewer(app), &thread.messages, width, THREAD_NAME_W, false, thread.selected, app.zen).into_iter().unzip();
    let rows: Vec<(usize, Vec<Slot>)> = items.iter().map(ListItem::height).zip(slots).collect();
    let title = format!("thread · {} replies", thread.messages.len().saturating_sub(1));
    let block = frame(app, &title, focused);
    let inner = block.inner(area);
    let list = List::new(items)
        .block(block)
        .highlight_style(row_highlight(&app.theme, focused))
        .highlight_symbol(cursor_bar(&app.theme, focused))
        .repeat_highlight_symbol(true)
        .highlight_spacing(HighlightSpacing::Always);
    let selected = (!thread.messages.is_empty()).then_some(thread.selected);
    app.thread_view.select(selected);
    f.render_stateful_widget(list, area, &mut app.thread_view);
    let x = inner.x + 1 + (TIME_W + 1 + THREAD_NAME_W + 1) as u16;
    placements(inner, x, app.thread_view.offset(), &rows)
}

/// Who is looking at the screen, so their own marks stand out.
struct Viewer<'a> {
    names: &'a NameBook,
    me: &'a str,
    theme: &'a Theme,
    thumbs: &'a Thumbs,
    spinner: &'static str,
}

fn viewer(app: &App) -> Viewer<'_> {
    Viewer { names: &app.names, me: &app.me, theme: &app.theme, thumbs: &app.thumbs, spinner: motion::spinner(app.elapsed()) }
}

/// How one message sits in its list.
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
fn grouped_items(
    viewer: &Viewer,
    messages: &[Message],
    width: usize,
    name_w: usize,
    show_meta: bool,
    selected: usize,
    zen: bool,
) -> Vec<(ListItem<'static>, Vec<Slot>)> {
    let mut items = Vec::with_capacity(messages.len());
    let mut prev: Option<&Message> = None;
    let mut last_day = String::new();
    for (i, m) in messages.iter().enumerate() {
        let day = time::day_label(&m.ts);
        let new_day = day != last_day;
        last_day = day;
        let continued = render::continues(prev, m);
        let gap = prev.is_some_and(|p| !ends_with_code_block(viewer.names, p)) && (new_day || !continued);
        let row = Row { header: !continued || i == selected, new_day, gap, show_time: !zen || i == selected, selected: i == selected };
        items.push(message_item(viewer, m, width, name_w, show_meta, row));
        prev = Some(m);
    }
    items
}

fn message_item(viewer: &Viewer, m: &Message, width: usize, name_w: usize, show_meta: bool, row: Row) -> (ListItem<'static>, Vec<Slot>) {
    let names = viewer.names;
    let theme = viewer.theme;
    let author = m.user.as_deref().map(|u| names.user_label(u)).or_else(|| m.username.clone()).unwrap_or_else(|| "bot".into());
    let indent = TIME_W + 1 + name_w + 1;
    let avail = width.saturating_sub(indent) as u16;
    let (pictures, others): (Vec<&File>, Vec<&File>) = m.files.iter().partition(|f| viewer.thumbs.cells(f, avail).is_some());
    let styled = body(names, m, others);
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
    for (i, chunks) in styled.wrap_styled(width.saturating_sub(indent).max(10)).iter().enumerate() {
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
    for file in pictures {
        let size = viewer.thumbs.cells(file, avail).expect("partitioned as a picture");
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
        let glyph = crate::emoji::glyph(&r.name).map(str::to_owned).unwrap_or_else(|| r.name.clone());
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

/// A block already leaves its own blank line behind, so the next message needs no gap.
fn ends_with_code_block(names: &NameBook, m: &Message) -> bool {
    body(names, m, &m.files).spans.last().is_some_and(|p| p.style == TextStyle::Block)
}

/// The message text plus a 📎 line per attached file that is not drawn as a picture.
fn body<'a>(names: &NameBook, m: &Message, files: impl IntoIterator<Item = &'a File>) -> Styled {
    let text =
        if m.text.is_empty() { m.attachments.iter().map(|a| a.fallback.clone()).collect::<Vec<_>>().join("\n") } else { m.text.clone() };
    let mut styled = match m.subtype.as_deref() {
        Some("channel_join") => Styled::dim("joined the channel"),
        _ => text::from_segments(&mrkdwn::parse(&text, names), false),
    };
    for file in files {
        styled.push_dim(&format!(" 📎 {}", file.label()));
    }
    styled
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

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let label = match &app.input {
        Some(Input::Reply { label, .. }) => format!("reply to {label}"),
        Some(Input::InboxReply { item }) => format!("reply to {}", item.label),
        Some(Input::React { .. }) => "react with".into(),
        Some(Input::Filter) => "filter".into(),
        Some(Input::Search) => "search".into(),
        None => return,
    };
    let line = Line::from(vec![
        Span::styled(format!(" {label} ▸ "), Style::new().fg(app.theme.accent).bold()),
        Span::raw(app.buffer.clone()),
        Span::styled("▌", Style::new().fg(app.theme.accent)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_palette(f: &mut Frame, app: &App, area: Rect) {
    let Some(palette) = &app.palette else { return };
    let line = Line::from(vec![
        Span::styled(" : ", Style::new().fg(app.theme.accent).bold()),
        Span::raw(palette.input.clone()),
        Span::styled("▌", Style::new().fg(app.theme.accent)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let completion = app.palette.as_ref().and_then(|p| p.hint());
    let hints = match (app.input.is_some(), app.focus) {
        _ if app.palette.is_some() => completion.as_deref().unwrap_or("tab cycle · → accept · ↑ history · enter run · esc cancel"),
        (true, _) => "enter send · esc cancel",
        (_, Focus::Channels) => "j/k move · enter open · ^k jump · i inbox · f firehose · / filter · s search · ? help · q quit",
        (_, Focus::Messages) => {
            "j/k move · enter thread · r reply · t thread reply · e react · o open · u link · y copy · s search · ? help"
        }
        (_, Focus::Thread) => "j/k move · r reply · e react · o open · u link · y copy · esc close · ? help",
    };
    let (dot, dot_style) = match &app.live {
        Live::Live => ("● ", Style::new().fg(app.theme.success)),
        Live::Connecting => ("○ ", Style::new().fg(app.theme.muted)),
        Live::Polling(_) => ("↻ ", Style::new().fg(app.theme.warn)),
    };
    let status = format!("{} ", app.status_line());
    let room = area.width as usize;
    let used = 1 + dot.len() + text::visible_width(&status);
    let right = text::truncate(hints, room.saturating_sub(used + 1));
    let pad = room.saturating_sub(used + text::visible_width(&right));
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(dot, dot_style),
        Span::styled(status, Style::new().bold()),
        Span::raw(" ".repeat(pad)),
        Span::styled(right, Style::new().fg(app.theme.muted)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_help(f: &mut Frame, theme: &Theme, area: Rect) {
    let lines = [
        "  tab           cycle panes",
        "  → / l         open channel · open the message's thread",
        "  ← / h         close thread · back to channels",
        "  j k  g G      move · top · bottom",
        "  enter         open channel · open thread · jump to result",
        "  r             reply in the focused conversation",
        "  t             reply in the selected message's thread",
        "  e             react (type the emoji name)",
        "  o / y         open in Slack · copy permalink",
        "  u             open the message's link in the browser",
        "  s   /         search · filter channels",
        "  i             inbox: unread DMs, mentions, thread replies",
        "  f             firehose: every channel as one live ticker",
        "  z             reading mode: one centered conversation, nothing else",
        "  :             command line: :join :go :msg :react :search :export :read …",
        "  ⌘k / ctrl-k   jump to a channel, person or thread · > searches",
        "  R             refresh",
        "  esc           close thread · clear search or filter",
        "  q             quit",
        "",
        "  messages are markdown: **bold** _italic_ `code` @user #channel",
    ];
    let height = lines.len() as u16 + 2;
    let width = 62;
    let popup = Rect {
        x: area.width.saturating_sub(width) / 2,
        y: area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    };
    f.render_widget(Clear, popup);
    let text: Vec<Line> = lines.iter().map(|l| Line::raw(*l)).collect();
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).block(pane(theme, "keys", true)), popup);
}

/// One wrapped line of message text as spans. A code line gets its bar and a fill to `width`
/// so the block reads as one surface.
pub fn body_spans(theme: &Theme, chunks: &[text::Piece], width: usize) -> Vec<Span<'static>> {
    if let [code] = chunks
        && code.style == TextStyle::Block
    {
        let fill = " ".repeat(width.saturating_sub(text::BLOCK_BAR_W + code.text.width()));
        return vec![
            Span::styled("▎ ", Style::new().fg(theme.faded).bg(theme.surface)),
            Span::styled(code.text.clone(), Style::new().bg(theme.surface)),
            Span::styled(fill, Style::new().bg(theme.surface)),
        ];
    }
    chunks.iter().map(|p| Span::styled(p.text.clone(), style_of(theme, p.style))).collect()
}

pub fn style_of(theme: &Theme, s: TextStyle) -> Style {
    match s {
        TextStyle::Plain => Style::new(),
        TextStyle::Dim => Style::new().fg(theme.muted),
        TextStyle::Bold => Style::new().bold(),
        TextStyle::Italic => Style::new().italic(),
        TextStyle::Strike => Style::new().crossed_out(),
        TextStyle::Code => Style::new().fg(theme.code).bg(theme.surface),
        TextStyle::Block => Style::new().bg(theme.surface),
        TextStyle::Link => Style::new().fg(theme.link).underlined(),
        TextStyle::Mention => Style::new().fg(theme.mention),
    }
}

pub fn user_style(theme: &Theme, name: &str) -> Style {
    Style::new().fg(theme.user(name)).bold()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Reaction;
    use crate::tui::app::{ChannelRow, Incoming, Thread};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn message(ts: &str, user: &str, text: &str) -> Message {
        Message { ts: ts.into(), user: Some(user.into()), text: text.into(), ..Default::default() }
    }

    #[test]
    fn code_rows_get_a_bar_and_a_full_width_fill() {
        let theme = Theme::default();
        let block = body_spans(&theme, &[text::Piece::new("x", TextStyle::Block)], 8);
        let texts: Vec<&str> = block.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["▎ ", "x", "     "]);
        assert_eq!(block[0].style.fg, Some(theme.faded));
        assert!(block[1..].iter().all(|s| s.style.bg == Some(theme.surface)));
        let blank = body_spans(&theme, &[text::Piece::new("", TextStyle::Block)], 4);
        assert_eq!(blank.iter().map(|s| s.content.as_ref()).collect::<Vec<_>>(), vec!["▎ ", "", "  "]);
        let inline = body_spans(&theme, &[text::Piece::new("git", TextStyle::Code)], 8);
        assert_eq!(inline.len(), 1);
        assert_eq!((inline[0].style.fg, inline[0].style.bg), (Some(theme.code), Some(theme.surface)));
    }

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
        app.buffer = "typing…".into();
        let backend = TestBackend::new(110, 18);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let stable = regex::Regex::new(r"\d\d:\d\d").unwrap().replace_all(&terminal.backend().to_string(), "HH:MM").to_string();
        insta::assert_snapshot!(stable);
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

    #[test]
    fn ghost_rows_share_one_width_and_blink_rarely() {
        for frame in 0..32 {
            let rows = ghost(frame);
            assert!(rows.iter().all(|r| r.width() == rows[0].width()), "frame {frame}: {rows:?}");
        }
        assert!(ghost(0)[1].contains("◠ ◠"));
        assert!(ghost(14)[1].contains("─ ─"));
        assert_eq!((0..32).filter(|f| ghost(*f)[1].contains("─ ─")).count(), 2);
    }

    #[test]
    fn empty_dm_shows_a_centered_ghost_with_a_contextual_caption() {
        let mut app = App::new();
        app.apply(Incoming::Channels {
            rows: vec![ChannelRow::new("D1", "@bob", Kind::Dm)],
            people: vec![],
            names: NameBook::default(),
            badges: Default::default(),
            me: "U1".into(),
        });
        app.current_channel = Some("D1".into());
        app.apply(Incoming::History { channel: "D1".into(), messages: vec![], names: NameBook::default() });
        let mut terminal = Terminal::new(TestBackend::new(90, 20)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("╭─────╮") && out.contains("│ ◠ ◠ │") && out.contains("╰┬┬┬┬┬╯"), "{out}");
        assert!(out.contains("nothing here yet") && out.contains("say hi to D1 with  r"), "{out}");
        let pane_middle: i32 = 26 + (90 - 26) / 2;
        let face = out.lines().find(|l| l.contains("◠ ◠")).unwrap();
        let face_at = face.chars().position(|c| c == '◠').unwrap();
        assert!((face_at as i32 - pane_middle).abs() <= 3, "face at {face_at}, pane middle {pane_middle}");
    }

    #[test]
    fn tiny_pane_keeps_only_the_caption() {
        let mut app = App::new();
        app.loading = false;
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("pick a conversation") && out.contains("enter"), "{out}");
        assert!(!out.contains("╭─────╮"), "{out}");
    }

    #[test]
    fn picture_rows_are_reserved_then_painted_under_the_text() {
        let mut app = App::new();
        app.thumbs = Thumbs::with(ratatui_image::picker::Picker::halfblocks());
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        let shot = File {
            id: "F1".into(),
            name: "shot.png".into(),
            mimetype: "image/png".into(),
            thumb_360: "https://files.slack.com/s.png".into(),
            thumb_360_w: 400,
            thumb_360_h: 200,
            ..Default::default()
        };
        let with_shot = Message { files: vec![shot], ..message("1694700000.000100", "U1", "look") };
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![with_shot], names: NameBook::default() });
        let render = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            terminal.draw(|f| draw(f, app)).unwrap();
            terminal.backend().to_string()
        };
        let out = render(&mut app);
        assert!(out.contains("⠋ loading image") && out.contains("📎 shot.png"), "{out}");
        assert!(!out.contains(" 📎 shot.png\n"), "no inline attachment line for a picture");
        let gradient = image::RgbImage::from_fn(400, 200, |_, y| image::Rgb([y as u8, y as u8, y as u8]));
        app.apply(Incoming::Thumb { id: "F1".into(), image: Some(image::DynamicImage::ImageRgb8(gradient)) });
        let out = render(&mut app);
        let lines: Vec<&str> = out.lines().collect();
        let text_row = lines.iter().position(|l| l.contains("look")).unwrap();
        let painted = |l: &str| l.contains('▀') || l.contains('▄');
        let first_pixels = lines.iter().position(|l| painted(l)).expect("halfblocks painted");
        assert_eq!(first_pixels, text_row + 1, "{out}");
        assert_eq!(lines.iter().filter(|l| painted(l)).count(), 10, "400x200 at 10x20 cells is 40x10");
        assert!(!out.contains("loading image"), "{out}");
    }

    fn render(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().to_string()
    }

    fn status_bar(app: &mut App) -> String {
        render(app).lines().last().unwrap_or_default().to_owned()
    }

    #[test]
    fn loading_title_spins_with_the_clock() {
        let mut app = App::new();
        assert!(render(&mut app).contains("messages ⠋ loading"));
        app.now += motion::SPINNER_FRAME;
        assert!(render(&mut app).contains("messages ⠙ loading"));
    }

    #[test]
    fn toast_shows_in_the_status_bar_until_it_ends() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        let bar = status_bar(&mut app);
        assert!(bar.contains("C1"), "{bar}");
        app.apply(Incoming::Toast("permalink copied".into()));
        let bar = status_bar(&mut app);
        assert!(bar.contains("permalink copied") && !bar.contains("C1"), "{bar}");
        app.now += std::time::Duration::from_secs(2);
        let bar = status_bar(&mut app);
        assert!(bar.contains("C1") && !bar.contains("permalink copied"), "{bar}");
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
    fn help_overlay_and_empty_state_render() {
        let mut app = App::new();
        app.help = true;
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("keys"));
        assert!(out.contains("reply in the focused conversation"));
    }
}
