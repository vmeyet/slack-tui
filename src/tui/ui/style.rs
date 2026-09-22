use crate::render::text::{self, Style as TextStyle};
use crate::tui::theme::Theme;
use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

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

fn style_of(theme: &Theme, s: TextStyle) -> Style {
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
}
