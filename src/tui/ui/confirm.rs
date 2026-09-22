use super::{MODAL_FRAME_H, MODAL_FRAME_W, centered_cells, modal_pane};
use crate::mrkdwn;
use crate::render::text;
use crate::resolve::NameBook;
use crate::tui::app::MyMessage;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

const CONFIRM_W: u16 = 56;
const CONFIRM_ANSWER: &str = "y deletes it · any other key keeps it";

/// The message about to go, on one line, and the only key that deletes it.
pub(super) fn draw(f: &mut Frame, theme: &Theme, names: &NameBook, message: &MyMessage, area: Rect) {
    let room = (CONFIRM_W - MODAL_FRAME_W) as usize;
    let preview = text::truncate(&mrkdwn::plain(&message.text, names).replace('\n', " "), room);
    let lines = vec![Line::from(Span::raw(preview)), Line::raw(""), Line::from(Span::styled(CONFIRM_ANSWER, Style::new().fg(theme.muted)))];
    let popup = centered_cells(area, CONFIRM_W, lines.len() as u16 + MODAL_FRAME_H);
    let block = modal_pane(theme, "delete this message?");
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;
    use crate::tui::ui::testing::render;

    #[test]
    fn the_delete_box_shows_the_message_and_the_key_that_deletes_it() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.pending_delete = Some(MyMessage { channel: "C1".into(), ts: "1".into(), text: "ship it".into() });
        let out = render(&mut app);
        assert!(out.contains("delete this message?"), "{out}");
        assert!(out.contains("ship it"), "{out}");
        assert!(out.contains(CONFIRM_ANSWER), "{out}");
    }
}
