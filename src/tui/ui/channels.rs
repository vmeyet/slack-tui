use super::{cursor_bar, pane, row_highlight};
use crate::render;
use crate::tui::app::{self, App, Focus, Kind, SidebarRow};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{HighlightSpacing, List, ListItem};
use unicode_width::UnicodeWidthStr;

pub(super) fn draw(f: &mut Frame, app: &mut App, area: Rect) {
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
    let label_w = width.saturating_sub(badge_text.width() + usize::from(!badge_text.is_empty()));
    let label = render::fit(&c.label, label_w);
    let badge_style = if badge.mentions > 0 { Style::new().fg(theme.accent).bold() } else { Style::new().fg(theme.muted) };
    ListItem::new(Line::from(vec![
        Span::raw(" "),
        Span::styled(label, style),
        Span::raw(if badge_text.is_empty() { "" } else { " " }),
        Span::styled(badge_text, badge_style),
    ]))
}
