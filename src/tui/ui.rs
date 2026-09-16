use super::app::{App, Focus, Input, Kind, Live};
use crate::api::{Message, SearchMatch};
use crate::mrkdwn;
use crate::render::text::{self, Style as TextStyle, Styled};
use crate::render::time;
use crate::resolve::NameBook;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

const TIME_W: usize = 5;
const NAME_W: usize = 10;
const USER_COLORS: [Color; 6] = [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta, Color::Blue, Color::LightRed];

pub fn draw(f: &mut Frame, app: &mut App) {
    let input_rows = u16::from(app.input.is_some());
    let [main, input, status] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(input_rows), Constraint::Length(1)]).areas(f.area());
    let thread_w = if app.thread.is_some() { 40 } else { 0 };
    let [left, middle, right] =
        Layout::horizontal([Constraint::Length(26), Constraint::Min(30), Constraint::Percentage(thread_w)]).areas(main);
    draw_channels(f, app, left);
    draw_messages(f, app, middle);
    if app.thread.is_some() {
        draw_thread(f, app, right);
    }
    if app.input.is_some() {
        draw_input(f, app, input);
    }
    draw_status(f, app, status);
    if let Some(inbox) = &app.inbox {
        super::inbox::draw(f, inbox, &app.names, main, app.highlight);
    }
    if let Some(jump) = &app.jump {
        super::jump::draw(f, jump, main, app.highlight);
    }
    if app.help {
        draw_help(f, f.area());
    }
}

fn pane(title: &str, focused: bool) -> Block<'static> {
    let style = if focused { Style::new().cyan() } else { Style::new().dim() };
    let title = if focused { format!(" {title} ").bold().cyan() } else { format!(" {title} ").into() };
    Block::bordered().border_style(style).title(title)
}

fn draw_channels(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Channels;
    let visible = app.visible_channels();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|c| {
            let current = app.current_channel.as_deref() == Some(&c.id);
            let marker = if current {
                "▸ "
            } else if app.unread.contains(&c.id) {
                "● "
            } else {
                "  "
            };
            let color = match c.kind {
                Kind::Public => Color::Reset,
                Kind::Private => Color::Yellow,
                Kind::Dm => Color::Magenta,
                Kind::GroupDm => Color::Blue,
            };
            let mut style = Style::new().fg(color);
            if current || app.unread.contains(&c.id) {
                style = style.add_modifier(Modifier::BOLD);
            }
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::styled(text::visible_fit(&c.label, area.width.saturating_sub(5) as usize), style),
            ]))
        })
        .collect();
    let title = if app.filter.is_empty() { "channels".to_owned() } else { format!("channels /{}", app.filter) };
    let list = List::new(items).block(pane(&title, focused)).highlight_style(highlight(app, focused));
    let mut state = ListState::default().with_selected(Some(app.channel_selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn highlight(app: &App, focused: bool) -> Style {
    if focused { Style::new().bg(app.highlight).add_modifier(Modifier::BOLD) } else { Style::new().bg(Color::Indexed(234)) }
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
    let width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = match &app.search {
        Some(results) => results.iter().map(|m| search_item(m, width)).collect(),
        None => app.messages.iter().map(|m| message_item(&app.names, m, width, NAME_W, true)).collect(),
    };
    let empty = items.is_empty();
    let list = List::new(items).block(pane(&title, focused)).highlight_style(highlight(app, focused));
    let mut state = ListState::default().with_selected((!empty).then_some(app.message_selected));
    f.render_stateful_widget(list, area, &mut state);
    if empty && app.current_channel.is_none() {
        let hint = Paragraph::new("pick a conversation on the left, enter to open".dim()).block(Block::default());
        f.render_widget(hint, Rect { x: area.x + 2, y: area.y + 2, width: area.width.saturating_sub(4), height: 1 });
    }
}

fn draw_thread(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Thread;
    let Some(thread) = &app.thread else { return };
    let width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = thread.messages.iter().map(|m| message_item(&app.names, m, width, 8, false)).collect();
    let title = format!("thread · {} replies", thread.messages.len().saturating_sub(1));
    let list = List::new(items).block(pane(&title, focused)).highlight_style(highlight(app, focused));
    let mut state = ListState::default().with_selected((!thread.messages.is_empty()).then_some(thread.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn message_item(names: &NameBook, m: &Message, width: usize, name_w: usize, show_meta: bool) -> ListItem<'static> {
    let author = m.user.as_deref().map(|u| names.user_label(u)).or_else(|| m.username.clone()).unwrap_or_else(|| "bot".into());
    let indent = TIME_W + 1 + name_w + 1;
    let styled = body(names, m);
    let mut lines: Vec<Line> = Vec::new();
    for (i, chunks) in styled.wrap_styled(width.saturating_sub(indent).max(10)).iter().enumerate() {
        let mut spans = if i == 0 {
            vec![
                Span::styled(time::hhmm(&m.ts), Style::new().dim()),
                Span::raw(" "),
                Span::styled(text::visible_fit(&author, name_w), user_style(&author)),
                Span::raw(" "),
            ]
        } else {
            vec![Span::raw(" ".repeat(indent))]
        };
        spans.extend(chunks.iter().map(|p| Span::styled(p.text.clone(), style_of(p.style))));
        lines.push(Line::from(spans));
    }
    if !m.reactions.is_empty() {
        let r: Vec<String> = m.reactions.iter().map(|r| format!("{} {}", crate::emoji::render(&r.name), r.count)).collect();
        lines.push(Line::from(vec![Span::raw(" ".repeat(indent)), Span::styled(r.join("  "), Style::new().dim())]));
    }
    if show_meta && m.is_thread_root() {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(format!("↳ {} replies", m.reply_count), Style::new().cyan()),
        ]));
    }
    ListItem::new(lines)
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

fn search_item(m: &SearchMatch, width: usize) -> ListItem<'static> {
    let head = Line::from(vec![
        Span::styled(format!("#{}", m.channel.name), Style::new().cyan()),
        Span::raw(" · "),
        Span::styled(format!("{} {}", time::day_label(&m.ts), time::hhmm(&m.ts)), Style::new().dim()),
        Span::raw("  "),
        Span::styled(m.username.clone(), user_style(&m.username)),
    ]);
    let mut lines = vec![head];
    let styled = text::from_segments(&mrkdwn::parse(&m.text, &mrkdwn::NoNames), false);
    for chunks in styled.wrap_styled(width.saturating_sub(4).max(10)).into_iter().take(3) {
        let mut spans = vec![Span::raw("    ")];
        spans.extend(chunks.into_iter().map(|p| Span::styled(p.text, style_of(p.style))));
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
        Span::styled(format!(" {label} ▸ "), Style::new().cyan().bold()),
        Span::raw(app.buffer.clone()),
        Span::styled("▌", Style::new().cyan()),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let hints = match (app.input.is_some(), app.focus) {
        (true, _) => "enter send · esc cancel",
        (_, Focus::Channels) => "j/k move · enter open · ^k jump · i inbox · / filter · s search · R refresh · ? help · q quit",
        (_, Focus::Messages) => {
            "j/k move · enter thread · r reply · t thread reply · e react · o open · u link · y copy · s search · ? help"
        }
        (_, Focus::Thread) => "j/k move · r reply · e react · o open · u link · y copy · esc close · ? help",
    };
    let (dot, dot_style) = match &app.live {
        Live::Live => ("● ", Style::new().green()),
        Live::Connecting => ("○ ", Style::new().dim()),
        Live::Polling(_) => ("↻ ", Style::new().yellow()),
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
        Span::styled(right, Style::new().dim()),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_help(f: &mut Frame, area: Rect) {
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
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).block(pane("keys", true)), popup);
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

fn user_style(name: &str) -> Style {
    let idx = name.trim().bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)) % USER_COLORS.len();
    Style::new().fg(USER_COLORS[idx]).bold()
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
    fn full_layout_snapshot() {
        let mut app = App::new();
        let rows = vec![
            ChannelRow { id: "C1".into(), label: "#general".into(), kind: Kind::Public },
            ChannelRow { id: "C2".into(), label: "🔒vault".into(), kind: Kind::Private },
            ChannelRow { id: "D1".into(), label: "@bob".into(), kind: Kind::Dm },
        ];
        app.apply(Incoming::Channels { rows, people: vec![], names: NameBook::default() });
        app.current_channel = Some("C1".into());
        let mut root = message(
            "1694700000.000100",
            "U1",
            "Deploy *v2* is out, see <https://acme.io|notes>. A long line that needs wrapping inside the pane for sure.",
        );
        root.reply_count = 2;
        root.thread_ts = Some(root.ts.clone());
        root.reactions = vec![Reaction { name: "rocket".into(), count: 3, users: vec![] }];
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
