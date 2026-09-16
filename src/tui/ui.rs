use super::app::{self, App, Focus, Input, Kind, Live, SidebarRow};
use super::theme::Theme;
use crate::api::{Message, Reaction, SearchMatch};
use crate::mrkdwn;
use crate::render;
use crate::render::text::{self, Style as TextStyle, Styled};
use crate::render::time;
use crate::resolve::NameBook;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, HighlightSpacing, List, ListItem, Padding, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

const TIME_W: usize = 5;
/// Two border columns plus the always-reserved cursor-bar column.
const BORDERS_AND_CURSOR_W: u16 = 3;
const NAME_W: usize = 12;

pub fn draw(f: &mut Frame, app: &mut App) {
    let input_rows = u16::from(app.input.is_some() || app.palette.is_some());
    let [main, input, status] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(input_rows), Constraint::Length(1)]).areas(f.area());
    let modal = app.inbox.is_some() || app.firehose.is_some() || app.jump.is_some() || app.help;
    if app.zen {
        draw_reading(f, app, main);
        if modal {
            fade(f, main, app.theme.faded);
        }
    } else {
        let thread_w = if app.thread.is_some() { 40 } else { 0 };
        let [left, middle, right] =
            Layout::horizontal([Constraint::Length(26), Constraint::Min(30), Constraint::Percentage(thread_w)]).areas(main);
        draw_channels(f, app, left);
        draw_messages(f, app, middle);
        if app.thread.is_some() {
            draw_thread(f, app, right);
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
    if app.input.is_some() {
        draw_input(f, app, input);
    } else if app.palette.is_some() {
        draw_palette(f, app, input);
    }
    draw_status(f, app, status);
    let theme = app.theme;
    if let Some(inbox) = &mut app.inbox {
        super::inbox::draw(f, inbox, &app.names, main, &theme);
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
fn draw_reading(f: &mut Frame, app: &mut App, area: Rect) {
    let width = (area.width * 3 / 4).clamp(80, 110).min(area.width);
    let column = Rect { x: area.x + (area.width - width) / 2, width, ..area };
    if app.thread.is_some() {
        draw_thread(f, app, column);
    } else {
        draw_messages(f, app, column);
    }
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

fn draw_messages(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Messages;
    let mut title = if app.search.is_some() {
        "search".to_owned()
    } else if app.current_channel.is_some() {
        app.current_label()
    } else {
        "messages".to_owned()
    };
    if app.loading {
        title.push_str(" · loading…");
    }
    let width = area.width.saturating_sub(BORDERS_AND_CURSOR_W) as usize;
    let items: Vec<ListItem> = match &app.search {
        Some(results) => results.iter().map(|m| search_item(&app.theme, m, width)).collect(),
        None => grouped_items(&viewer(app), &app.messages, width, NAME_W, true, app.message_selected, app.zen),
    };
    let empty = items.is_empty();
    let list = List::new(items)
        .block(frame(app, &title, focused))
        .highlight_style(row_highlight(&app.theme, focused))
        .highlight_symbol(cursor_bar(&app.theme, focused))
        .repeat_highlight_symbol(true)
        .highlight_spacing(HighlightSpacing::Always);
    app.messages_view.select((!empty).then_some(app.message_selected));
    f.render_stateful_widget(list, area, &mut app.messages_view);
    if empty && app.current_channel.is_none() {
        let hint = Paragraph::new("pick a conversation on the left, enter to open".fg(app.theme.muted)).block(Block::default());
        f.render_widget(hint, Rect { x: area.x + 2, y: area.y + 2, width: area.width.saturating_sub(4), height: 1 });
    }
}

fn draw_thread(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Thread;
    let Some(thread) = &app.thread else { return };
    let width = area.width.saturating_sub(BORDERS_AND_CURSOR_W) as usize;
    let items: Vec<ListItem> = grouped_items(&viewer(app), &thread.messages, width, 8, false, thread.selected, app.zen);
    let title = format!("thread · {} replies", thread.messages.len().saturating_sub(1));
    let list = List::new(items)
        .block(frame(app, &title, focused))
        .highlight_style(row_highlight(&app.theme, focused))
        .highlight_symbol(cursor_bar(&app.theme, focused))
        .repeat_highlight_symbol(true)
        .highlight_spacing(HighlightSpacing::Always);
    let selected = (!thread.messages.is_empty()).then_some(thread.selected);
    app.thread_view.select(selected);
    f.render_stateful_widget(list, area, &mut app.thread_view);
}

/// Who is looking at the screen, so their own marks stand out.
struct Viewer<'a> {
    names: &'a NameBook,
    me: &'a str,
    theme: &'a Theme,
}

fn viewer(app: &App) -> Viewer<'_> {
    Viewer { names: &app.names, me: &app.me, theme: &app.theme }
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
) -> Vec<ListItem<'static>> {
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

fn message_item(viewer: &Viewer, m: &Message, width: usize, name_w: usize, show_meta: bool, row: Row) -> ListItem<'static> {
    let names = viewer.names;
    let theme = viewer.theme;
    let author = m.user.as_deref().map(|u| names.user_label(u)).or_else(|| m.username.clone()).unwrap_or_else(|| "bot".into());
    let indent = TIME_W + 1 + name_w + 1;
    let styled = body(names, m);
    let mut lines: Vec<Line> = Vec::new();
    if row.gap {
        lines.push(Line::raw(""));
    }
    if row.new_day {
        let label = time::day_label(&m.ts);
        let dashes = "─".repeat(width.saturating_sub(label.len() + 4));
        lines.push(Line::from(Span::styled(format!("── {label} {dashes}"), Style::new().fg(theme.muted))));
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
    ListItem::new(lines)
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
    body(names, m).spans.last().is_some_and(|p| p.style == TextStyle::Block)
}

fn body(names: &NameBook, m: &Message) -> Styled {
    let text =
        if m.text.is_empty() { m.attachments.iter().map(|a| a.fallback.clone()).collect::<Vec<_>>().join("\n") } else { m.text.clone() };
    let mut styled = match m.subtype.as_deref() {
        Some("channel_join") => Styled::dim("joined the channel"),
        _ => text::from_segments(&mrkdwn::parse(&text, names), false),
    };
    for file in &m.files {
        styled.push_dim(&format!(" 📎 {}", if file.title.is_empty() { &file.name } else { &file.title }));
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
    let status = format!("{} ", app.status);
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
        app.status = "#general".into();
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
    fn selected_message_shows_its_header_even_when_it_continues_the_previous_one() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        let messages = vec![message("1694700000.000100", "U1", "first"), message("1694700010.000100", "U1", "second")];
        app.apply(Incoming::History { channel: "C1".into(), messages, names: NameBook::default() });
        let render = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
            terminal.draw(|f| draw(f, app)).unwrap();
            terminal.backend().to_string()
        };
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
