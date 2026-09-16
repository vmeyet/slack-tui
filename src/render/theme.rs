use owo_colors::{AnsiColors, OwoColorize};
use std::io::IsTerminal;

#[derive(Clone, Debug)]
pub struct Theme {
    pub color: bool,
    pub width: usize,
    /// Print `label (url)` instead of a clickable label.
    pub show_urls: bool,
}

const USER_COLORS: [AnsiColors; 6] =
    [AnsiColors::Cyan, AnsiColors::Green, AnsiColors::Yellow, AnsiColors::Magenta, AnsiColors::Blue, AnsiColors::BrightRed];

impl Theme {
    pub fn detect() -> Self {
        let tty = std::io::stdout().is_terminal();
        let color = tty && std::env::var_os("NO_COLOR").is_none();
        let width = std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse().ok())
            .or_else(|| crossterm::terminal::size().ok().map(|(w, _)| w as usize))
            .filter(|w| *w > 0)
            .unwrap_or(100);
        Self { color, width, show_urls: false }
    }

    pub fn plain(width: usize) -> Self {
        Self { color: false, width, show_urls: false }
    }

    pub fn with_show_urls(mut self, show_urls: bool) -> Self {
        self.show_urls = show_urls;
        self
    }

    /// A label that opens `url` on terminals speaking OSC 8, plain underlined text elsewhere.
    pub fn hyperlink(&self, label: &str, url: &str) -> String {
        let text = self.paint(label, |s| s.blue().underline().to_string());
        if self.color && !url.is_empty() { format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\") } else { text }
    }

    fn paint(&self, s: &str, f: impl FnOnce(&str) -> String) -> String {
        if self.color { f(s) } else { s.to_owned() }
    }

    pub fn dim(&self, s: &str) -> String {
        self.paint(s, |s| s.dimmed().to_string())
    }
    pub fn bold(&self, s: &str) -> String {
        self.paint(s, |s| s.bold().to_string())
    }
    pub fn italic(&self, s: &str) -> String {
        self.paint(s, |s| s.italic().to_string())
    }
    pub fn strike(&self, s: &str) -> String {
        self.paint(s, |s| s.strikethrough().to_string())
    }
    pub fn title(&self, s: &str) -> String {
        self.paint(s, |s| s.bold().white().to_string())
    }
    pub fn accent(&self, s: &str) -> String {
        self.paint(s, |s| s.cyan().to_string())
    }
    pub fn time(&self, s: &str) -> String {
        self.paint(s, |s| s.dimmed().to_string())
    }
    pub fn code(&self, s: &str) -> String {
        self.paint(s, |s| s.yellow().to_string())
    }
    pub fn link(&self, s: &str) -> String {
        self.paint(s, |s| s.blue().underline().to_string())
    }
    pub fn link_labelled(&self, label: &str, url: &str) -> String {
        if label == url || url.is_empty() {
            return self.link(label);
        }
        if self.show_urls {
            return format!("{} {}", self.paint(label, |s| s.underline().to_string()), self.dim(&format!("({url})")));
        }
        self.hyperlink(label, url)
    }
    pub fn mention(&self, s: &str) -> String {
        self.paint(s, |s| s.magenta().to_string())
    }
    pub fn ok(&self, s: &str) -> String {
        self.paint(s, |s| s.green().bold().to_string())
    }
    pub fn err(&self, s: &str) -> String {
        self.paint(s, |s| s.red().bold().to_string())
    }
    pub fn user(&self, name: &str) -> String {
        let idx = name.trim().bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)) % USER_COLORS.len();
        self.paint(name, |s| s.color(USER_COLORS[idx]).bold().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labelled_links_follow_the_setting() {
        let plain = Theme::plain(80);
        assert_eq!(plain.link_labelled("docs", "https://a.io"), "docs");
        assert_eq!(plain.with_show_urls(true).link_labelled("docs", "https://a.io"), "docs (https://a.io)");
        assert_eq!(Theme::plain(80).link_labelled("https://a.io", "https://a.io"), "https://a.io");
        let color = Theme { color: true, width: 80, show_urls: false };
        let out = color.link_labelled("docs", "https://a.io");
        assert!(out.starts_with("\x1b]8;;https://a.io\x1b\\"), "{out:?}");
        assert!(out.ends_with("\x1b]8;;\x1b\\"));
    }
}
