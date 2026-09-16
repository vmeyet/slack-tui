use super::Theme;
use crate::mrkdwn::Segment;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Plain,
    Dim,
    Bold,
    Italic,
    Strike,
    Code,
    Link,
    Mention,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styled {
    pub spans: Vec<(String, Style)>,
}

impl Styled {
    pub fn dim(s: &str) -> Self {
        Self { spans: vec![(s.to_owned(), Style::Dim)] }
    }

    pub fn push_dim(&mut self, s: &str) {
        self.spans.push((s.to_owned(), Style::Dim));
    }

    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|(t, _)| t.as_str()).collect()
    }

    /// Word-wraps to `width` visible columns, returning plain lines carrying no colour.
    pub fn wrap(&self, width: usize) -> Vec<String> {
        self.wrap_with(width, &Theme::plain(width))
    }

    pub fn wrap_with(&self, width: usize, theme: &Theme) -> Vec<String> {
        self.wrap_styled(width).iter().map(|line| line.iter().map(|(t, s)| paint(theme, t, *s)).collect()).collect()
    }

    /// Word-wraps keeping the style of every chunk, for renderers that paint themselves.
    pub fn wrap_styled(&self, width: usize) -> Vec<Vec<(String, Style)>> {
        wrap_spans(&self.spans, width)
    }
}

pub fn from_segments(segments: &[Segment]) -> Styled {
    let spans = segments
        .iter()
        .map(|s| match s {
            Segment::Text(t) => (t.clone(), Style::Plain),
            Segment::Bold(t) => (t.clone(), Style::Bold),
            Segment::Italic(t) => (t.clone(), Style::Italic),
            Segment::Strike(t) => (t.clone(), Style::Strike),
            Segment::Code(t) => (t.clone(), Style::Code),
            Segment::Pre(t) => (format!("\n{t}\n"), Style::Code),
            Segment::Link { label, url } if label == url => (url.clone(), Style::Link),
            Segment::Link { label, url } => (format!("{label} ({url})"), Style::Link),
            Segment::Mention(n) => (format!("@{n}"), Style::Mention),
            Segment::Channel(n) => (format!("#{n}"), Style::Mention),
        })
        .collect();
    Styled { spans }
}

pub fn paint(theme: &Theme, text: &str, style: Style) -> String {
    match style {
        Style::Plain => text.to_owned(),
        Style::Dim => theme.dim(text),
        Style::Bold => theme.bold(text),
        Style::Italic => theme.italic(text),
        Style::Strike => theme.strike(text),
        Style::Code => theme.code(text),
        Style::Link => theme.link(text),
        Style::Mention => theme.mention(text),
    }
}

/// Pads or truncates to exactly `width` columns.
pub fn visible_fit(s: &str, width: usize) -> String {
    super::fit(s, width)
}

/// Truncates to at most `width` columns, never pads.
pub fn truncate(s: &str, width: usize) -> String {
    if s.width() <= width { s.to_owned() } else { super::fit(s, width) }
}

pub fn visible_width(s: &str) -> usize {
    strip_ansi(s).width()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

type Chunks = Vec<(String, Style)>;

fn wrap_spans(spans: &[(String, Style)], width: usize) -> Vec<Chunks> {
    let mut lines: Vec<Chunks> = Vec::new();
    let mut line: Chunks = Vec::new();
    let mut used = 0;
    for (text, style) in spans {
        for (i, paragraph) in text.split('\n').enumerate() {
            if i > 0 {
                lines.push(trim_line(std::mem::take(&mut line)));
                used = 0;
            }
            for word in split_words(paragraph) {
                let w = word.width();
                if used > 0 && used + w > width {
                    lines.push(trim_line(std::mem::take(&mut line)));
                    used = 0;
                    if word.trim().is_empty() {
                        continue;
                    }
                }
                push_chunk(&mut line, word, *style);
                used += w;
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(trim_line(line));
    }
    lines
}

fn push_chunk(line: &mut Chunks, word: &str, style: Style) {
    match line.last_mut() {
        Some((text, s)) if *s == style => text.push_str(word),
        _ => line.push((word.to_owned(), style)),
    }
}

fn trim_line(mut line: Chunks) -> Chunks {
    if let Some((text, _)) = line.last_mut() {
        let trimmed = text.trim_end().to_owned();
        *text = trimmed;
    }
    line.retain(|(t, _)| !t.is_empty());
    line
}

fn split_words(s: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if c == ' ' {
            if start < i {
                words.push(&s[start..i]);
            }
            words.push(&s[i..i + 1]);
            start = i + 1;
        }
    }
    if start < s.len() {
        words.push(&s[start..]);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_word_boundaries() {
        let s = Styled { spans: vec![("the quick brown fox jumps".into(), Style::Plain)] };
        assert_eq!(s.wrap(10), vec!["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn newlines_force_breaks_and_styles_span_words() {
        let s = Styled { spans: vec![("a b".into(), Style::Plain), ("\nc".into(), Style::Bold)] };
        assert_eq!(s.wrap(80), vec!["a b", "c"]);
        let t = Theme { color: true, width: 80 };
        let colored = s.wrap_with(80, &t);
        assert!(colored[1].contains("\x1b[1m"));
        assert_eq!(visible_width(&colored[1]), 1);
    }

    #[test]
    fn long_word_is_kept_whole() {
        let s = Styled { spans: vec![("abcdefghijkl x".into(), Style::Plain)] };
        assert_eq!(s.wrap(5), vec!["abcdefghijkl", "x"]);
    }

    #[test]
    fn empty_gives_one_empty_line() {
        assert_eq!(Styled::default().wrap(5), vec![""]);
    }
}
