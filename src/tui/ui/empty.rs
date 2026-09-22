use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// The caption under the ghost, chosen from what the pane is showing.
pub struct Empty {
    pub title: String,
    pub hint: String,
    pub key: Option<&'static str>,
}

const GHOST_H: u16 = 4;
/// Ghost, its trail, a blank line, then the two caption lines.
const EMPTY_H: u16 = GHOST_H + 3;
const EMPTY_MIN_W: u16 = 28;

/// Every row is the same width so the face never drifts from the hem. Blinks once in a while.
fn ghost(frame: u32) -> [&'static str; GHOST_H as usize] {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::resolve::NameBook;
    use crate::tui::app::{App, ChannelRow, Incoming, Kind};
    use crate::tui::ui::testing::render_at;
    use std::collections::HashMap;
    use unicode_width::UnicodeWidthStr;

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
            badges: HashMap::new(),
            me: "U1".into(),
        });
        app.current_channel = Some("D1".into());
        app.apply(Incoming::History { channel: "D1".into(), messages: vec![], names: NameBook::default() });
        let out = render_at(&mut app, 90, 20);
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
        let out = render_at(&mut app, 60, 8);
        assert!(out.contains("pick a conversation") && out.contains("enter"), "{out}");
        assert!(!out.contains("╭─────╮"), "{out}");
    }
}
