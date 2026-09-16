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

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub text: String,
    pub style: Style,
    pub url: Option<String>,
}

impl Piece {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self { text: text.into(), style, url: None }
    }

    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self { text: text.into(), style: Style::Link, url: Some(url.into()) }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styled {
    pub spans: Vec<Piece>,
}

impl Styled {
    pub fn dim(s: &str) -> Self {
        Self { spans: vec![Piece::new(s, Style::Dim)] }
    }

    pub fn push_dim(&mut self, s: &str) {
        self.spans.push(Piece::new(s, Style::Dim));
    }

    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|p| p.text.as_str()).collect()
    }

    pub fn urls(&self) -> Vec<String> {
        self.spans.iter().filter_map(|p| p.url.clone()).collect()
    }

    /// Word-wraps to `width` visible columns, returning plain lines carrying no colour.
    pub fn wrap(&self, width: usize) -> Vec<String> {
        self.wrap_with(width, &Theme::plain(width))
    }

    pub fn wrap_with(&self, width: usize, theme: &Theme) -> Vec<String> {
        self.wrap_styled(width).iter().map(|line| line.iter().map(|p| paint(theme, p)).collect()).collect()
    }

    /// Word-wraps keeping the style of every chunk, for renderers that paint themselves.
    pub fn wrap_styled(&self, width: usize) -> Vec<Vec<Piece>> {
        wrap_spans(&self.spans, width)
    }
}

/// `show_urls` prints `label (url)`; otherwise a labelled link keeps only its label.
pub fn from_segments(segments: &[Segment], show_urls: bool) -> Styled {
    let spans = segments
        .iter()
        .map(|s| match s {
            Segment::Text(t) => Piece::new(t.clone(), Style::Plain),
            Segment::Bold(t) => Piece::new(t.clone(), Style::Bold),
            Segment::Italic(t) => Piece::new(t.clone(), Style::Italic),
            Segment::Strike(t) => Piece::new(t.clone(), Style::Strike),
            Segment::Code(t) => Piece::new(t.clone(), Style::Code),
            Segment::Pre(t) => Piece::new(format!("\n{t}\n"), Style::Code),
            Segment::Link { label, url } if label == url => Piece::link(url.clone(), url.clone()),
            Segment::Link { label, url } if show_urls => Piece::link(format!("{label} ({url})"), url.clone()),
            Segment::Link { label, url } => Piece::link(label.clone(), url.clone()),
            Segment::Mention(n) => Piece::new(format!("@{n}"), Style::Mention),
            Segment::Channel(n) => Piece::new(format!("#{n}"), Style::Mention),
            Segment::Emoji(n) => Piece::new(crate::emoji::render(n), Style::Plain),
        })
        .collect();
    Styled { spans }
}

pub fn paint(theme: &Theme, piece: &Piece) -> String {
    let text = &piece.text;
    match piece.style {
        Style::Plain => text.to_owned(),
        Style::Dim => theme.dim(text),
        Style::Bold => theme.bold(text),
        Style::Italic => theme.italic(text),
        Style::Strike => theme.strike(text),
        Style::Code => theme.code(text),
        Style::Link => theme.hyperlink(text, piece.url.as_deref().unwrap_or("")),
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

type Chunks = Vec<Piece>;

/// A word is a run of styled pieces with no whitespace between them, so a styled
/// mention glued to punctuation never breaks in the middle.
struct Word {
    pieces: Chunks,
    width: usize,
    blank: bool,
}

fn wrap_spans(spans: &[Piece], width: usize) -> Vec<Chunks> {
    let mut lines: Vec<Chunks> = Vec::new();
    for paragraph in paragraphs(spans) {
        let mut line: Chunks = Vec::new();
        let mut used = 0;
        for word in words(&paragraph) {
            if used > 0 && used + word.width > width {
                lines.push(trim_line(std::mem::take(&mut line)));
                used = 0;
                if word.blank {
                    continue;
                }
            }
            for piece in &word.pieces {
                push_chunk(&mut line, piece);
            }
            used += word.width;
        }
        lines.push(trim_line(line));
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

fn paragraphs(spans: &[Piece]) -> Vec<Chunks> {
    let mut out: Vec<Chunks> = vec![Vec::new()];
    for piece in spans {
        for (i, part) in piece.text.split('\n').enumerate() {
            if i > 0 {
                out.push(Vec::new());
            }
            if !part.is_empty() {
                out.last_mut().expect("one paragraph").push(Piece { text: part.to_owned(), ..piece.clone() });
            }
        }
    }
    out
}

fn words(paragraph: &Chunks) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut open = false;
    for piece in paragraph {
        for word in split_words(&piece.text) {
            let blank = word.trim().is_empty();
            let part = Piece { text: word.to_owned(), ..piece.clone() };
            if blank || !open {
                words.push(Word { pieces: vec![part], width: word.width(), blank });
            } else {
                let last = words.last_mut().expect("open word");
                last.pieces.push(part);
                last.width += word.width();
            }
            open = !blank;
        }
    }
    words
}

fn push_chunk(line: &mut Chunks, piece: &Piece) {
    match line.last_mut() {
        Some(last) if last.style == piece.style && last.url == piece.url => last.text.push_str(&piece.text),
        _ => line.push(piece.clone()),
    }
}

fn trim_line(mut line: Chunks) -> Chunks {
    if let Some(last) = line.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    line.retain(|p| !p.text.is_empty());
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
        let s = Styled { spans: vec![Piece::new("the quick brown fox jumps", Style::Plain)] };
        assert_eq!(s.wrap(10), vec!["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn newlines_force_breaks_and_styles_span_words() {
        let s = Styled { spans: vec![Piece::new("a b", Style::Plain), Piece::new("\nc", Style::Bold)] };
        assert_eq!(s.wrap(80), vec!["a b", "c"]);
        let t = Theme { color: true, width: 80, show_urls: false };
        let colored = s.wrap_with(80, &t);
        assert!(colored[1].contains("\x1b[1m"));
        assert_eq!(visible_width(&colored[1]), 1);
    }

    #[test]
    fn long_word_is_kept_whole() {
        let s = Styled { spans: vec![Piece::new("abcdefghijkl x", Style::Plain)] };
        assert_eq!(s.wrap(5), vec!["abcdefghijkl", "x"]);
    }

    #[test]
    fn styled_pieces_glued_to_punctuation_stay_together() {
        let s = Styled {
            spans: vec![Piece::new("with ", Style::Plain), Piece::new("@Gabriel", Style::Mention), Piece::new(":", Style::Plain)],
        };
        assert_eq!(s.wrap(13), vec!["with", "@Gabriel:"]);
        assert_eq!(s.wrap_styled(20)[0].len(), 3);
    }

    #[test]
    fn links_keep_label_only_unless_asked() {
        let segs = vec![Segment::Link { label: "docs".into(), url: "https://a.io".into() }];
        assert_eq!(from_segments(&segs, false).plain_text(), "docs");
        assert_eq!(from_segments(&segs, true).plain_text(), "docs (https://a.io)");
        assert_eq!(from_segments(&segs, false).urls(), vec!["https://a.io"]);
    }

    #[test]
    fn empty_gives_one_empty_line() {
        assert_eq!(Styled::default().wrap(5), vec![""]);
    }
}
