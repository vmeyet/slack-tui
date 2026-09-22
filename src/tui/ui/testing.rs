#![allow(clippy::unwrap_used)]
use crate::api::Message;
use crate::tui::app::App;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

pub fn message(ts: &str, user: &str, text: &str) -> Message {
    Message { ts: ts.into(), user: Some(user.into()), text: text.into(), ..Default::default() }
}

pub fn render_at(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| super::draw(f, app)).unwrap();
    terminal.backend().to_string()
}

pub fn render(app: &mut App) -> String {
    render_at(app, 80, 10)
}

/// The test backend quotes every row and may add a note after it; tests read the cells alone.
pub fn cells(row: &str) -> &str {
    row.trim_start_matches('"').split('"').next().unwrap_or_default()
}
