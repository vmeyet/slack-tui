use super::items::{BORDERS_AND_CURSOR_W, grouped_items, text_start, viewer};
use super::pictures::{self, Placement, Slot};
use super::{cursor_bar, frame, row_highlight};
use crate::tui::app::{App, Focus};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{HighlightSpacing, List, ListItem};

const THREAD_NAME_W: usize = 8;

pub(super) fn draw(f: &mut Frame, app: &mut App, area: Rect) -> Vec<Placement> {
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
    let x = inner.x + 1 + text_start(THREAD_NAME_W) as u16;
    pictures::placements(inner, x, app.thread_view.offset(), &rows)
}
