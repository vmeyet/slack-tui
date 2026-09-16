//! Light markdown typed by a human, turned into Block Kit rich_text blocks.
use anyhow::{Result, bail};
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

pub const MAX_BLOCKS: usize = 50;
const MAX_HEADER: usize = 150;

pub trait Mentions {
    fn user(&self, handle: &str) -> Option<String>;
    fn channel(&self, name: &str) -> Option<String>;
}

pub struct NoMentions;

impl Mentions for NoMentions {
    fn user(&self, _: &str) -> Option<String> {
        None
    }
    fn channel(&self, _: &str) -> Option<String> {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub blocks: Vec<Value>,
    pub text: String,
}

static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^( *)(?:[-*•]|(\d+)[.)]) +(.*)$").unwrap());
static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{1,6} +(.+?)\s*$").unwrap());
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https?://[^\s<>]+").unwrap());
static MD_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([^\]]+)\]\(([^)\s]+)\)").unwrap());
static HANDLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][\w.-]*").unwrap());
static CHANNEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9_-]*").unwrap());
static EMOJI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:([a-z0-9_+-]+):").unwrap());

pub fn to_blocks(markdown: &str, mentions: &dyn Mentions) -> Rendered {
    let mut doc = Doc { mentions, blocks: vec![], rich: vec![], text: String::new() };
    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            let end = (i + 1..lines.len()).find(|&j| lines[j].trim_start().starts_with("```")).unwrap_or(lines.len());
            doc.preformatted(&lines[i + 1..end].join("\n"));
            i = end + 1;
        } else if let Some(caps) = HEADING.captures(line) {
            doc.header(&caps[1]);
            i += 1;
        } else if matches!(line.trim(), "---" | "***" | "___") {
            doc.divider();
            i += 1;
        } else if LIST_ITEM.is_match(line) {
            let end = (i..lines.len()).find(|&j| !LIST_ITEM.is_match(lines[j])).unwrap_or(lines.len());
            doc.list(&lines[i..end]);
            i = end;
        } else if line.starts_with('>') {
            let end = (i..lines.len()).find(|&j| !lines[j].starts_with('>')).unwrap_or(lines.len());
            let quoted: Vec<&str> = lines[i..end].iter().map(|l| l[1..].strip_prefix(' ').unwrap_or(&l[1..])).collect();
            doc.quote(&quoted.join("\n"));
            i = end;
        } else if line.trim().is_empty() {
            doc.paragraph_break();
            i += 1;
        } else {
            let end = (i..lines.len()).find(|&j| is_block_start(lines[j])).unwrap_or(lines.len());
            doc.paragraph(&lines[i..end].join("\n"));
            i = end;
        }
    }
    doc.finish()
}

fn is_block_start(line: &str) -> bool {
    line.trim().is_empty()
        || line.trim_start().starts_with("```")
        || HEADING.is_match(line)
        || LIST_ITEM.is_match(line)
        || line.starts_with('>')
        || matches!(line.trim(), "---" | "***" | "___")
}

struct Doc<'a> {
    mentions: &'a dyn Mentions,
    blocks: Vec<Value>,
    rich: Vec<Value>,
    text: String,
}

impl Doc<'_> {
    fn finish(mut self) -> Rendered {
        self.flush_rich();
        Rendered { blocks: self.blocks, text: self.text.trim().to_owned() }
    }

    fn flush_rich(&mut self) {
        if !self.rich.is_empty() {
            let elements = std::mem::take(&mut self.rich);
            self.blocks.push(json!({"type": "rich_text", "elements": elements}));
        }
    }

    fn header(&mut self, text: &str) {
        self.flush_rich();
        let title: String = text.chars().take(MAX_HEADER).collect();
        self.blocks.push(json!({"type": "header", "text": {"type": "plain_text", "text": title, "emoji": true}}));
        self.text.push_str(&format!("{title}\n"));
    }

    fn divider(&mut self) {
        self.flush_rich();
        self.blocks.push(json!({"type": "divider"}));
    }

    fn preformatted(&mut self, code: &str) {
        self.rich.push(json!({"type": "rich_text_preformatted", "elements": [{"type": "text", "text": code}]}));
        self.text.push_str(&format!("{code}\n"));
    }

    fn quote(&mut self, text: &str) {
        let (elements, plain) = inline(text, self.mentions);
        self.rich.push(json!({"type": "rich_text_quote", "elements": elements}));
        self.text.push_str(&format!("> {plain}\n"));
    }

    fn paragraph_break(&mut self) {
        if let Some(last) = self.rich.last_mut().filter(|e| e["type"] == "rich_text_section") {
            last["elements"].as_array_mut().unwrap().push(json!({"type": "text", "text": "\n"}));
        }
    }

    fn paragraph(&mut self, text: &str) {
        let (elements, plain) = inline(text, self.mentions);
        if let Some(last) = self.rich.last_mut().filter(|e| e["type"] == "rich_text_section") {
            let list = last["elements"].as_array_mut().unwrap();
            list.push(json!({"type": "text", "text": "\n"}));
            list.extend(elements);
        } else {
            self.rich.push(json!({"type": "rich_text_section", "elements": elements}));
        }
        self.text.push_str(&format!("{plain}\n"));
    }

    fn list(&mut self, lines: &[&str]) {
        let mut current: Option<(String, usize, Vec<Value>)> = None;
        for line in lines {
            let caps = LIST_ITEM.captures(line).expect("caller checked");
            let style = if caps.get(2).is_some() { "ordered" } else { "bullet" };
            let indent = caps[1].len() / 2;
            let (elements, plain) = inline(&caps[3], self.mentions);
            self.text.push_str(&format!("{}• {plain}\n", "  ".repeat(indent)));
            match current.as_mut() {
                Some((s, ind, items)) if s == style && *ind == indent => items.push(section(elements)),
                _ => {
                    self.push_list(current.take());
                    current = Some((style.to_owned(), indent, vec![section(elements)]));
                }
            }
        }
        self.push_list(current);
    }

    fn push_list(&mut self, list: Option<(String, usize, Vec<Value>)>) {
        if let Some((style, indent, items)) = list {
            self.rich.push(json!({"type": "rich_text_list", "style": style, "indent": indent, "elements": items}));
        }
    }
}

fn section(elements: Vec<Value>) -> Value {
    json!({"type": "rich_text_section", "elements": elements})
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
}

impl Style {
    fn json(self) -> Option<Value> {
        let mut style = serde_json::Map::new();
        if self.bold {
            style.insert("bold".into(), json!(true));
        }
        if self.italic {
            style.insert("italic".into(), json!(true));
        }
        if self.strike {
            style.insert("strike".into(), json!(true));
        }
        (!style.is_empty()).then_some(Value::Object(style))
    }
}

struct Inline<'a> {
    mentions: &'a dyn Mentions,
    elements: Vec<Value>,
    plain: String,
    buf: String,
    style: Style,
}

pub fn inline(text: &str, mentions: &dyn Mentions) -> (Vec<Value>, String) {
    let mut p = Inline { mentions, elements: vec![], plain: String::new(), buf: String::new(), style: Style::default() };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        let c = chars[i];
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        if c == '\\' && i + 1 < chars.len() {
            p.buf.push(chars[i + 1]);
            i += 2;
        } else if c == '`' {
            let end = chars[i + 1..].iter().position(|&x| x == '`').map(|n| i + 1 + n);
            match end {
                Some(end) => {
                    p.code(&chars[i + 1..end].iter().collect::<String>());
                    i = end + 1;
                }
                None => {
                    p.buf.push(c);
                    i += 1;
                }
            }
        } else if rest.starts_with("**") || rest.starts_with("__") {
            p.toggle(|s| s.bold = !s.bold);
            i += 2;
        } else if rest.starts_with("~~") {
            p.toggle(|s| s.strike = !s.strike);
            i += 2;
        } else if (c == '*' || c == '_') && toggles_italic(&chars, i, p.style.italic) {
            p.toggle(|s| s.italic = !s.italic);
            i += 1;
        } else if let Some(m) = MD_LINK.captures(&rest) {
            p.link(&m[2], &m[1]);
            i += m[0].chars().count();
        } else if let Some(m) = URL.find(&rest) {
            let url = m.as_str().trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
            p.link(url, url);
            i += url.chars().count();
        } else if c == '@' && at_boundary(prev) {
            match HANDLE.find(&rest[1..]).and_then(|m| p.mentions.user(m.as_str()).map(|id| (m.as_str().to_owned(), id))) {
                Some((handle, id)) => {
                    p.user(&id, &handle);
                    i += 1 + handle.chars().count();
                }
                None => {
                    p.buf.push(c);
                    i += 1;
                }
            }
        } else if c == '#' && at_boundary(prev) {
            match CHANNEL.find(&rest[1..]).and_then(|m| p.mentions.channel(m.as_str()).map(|id| (m.as_str().to_owned(), id))) {
                Some((name, id)) => {
                    p.channel(&id, &name);
                    i += 1 + name.chars().count();
                }
                None => {
                    p.buf.push(c);
                    i += 1;
                }
            }
        } else if let Some(m) = EMOJI.captures(&rest).filter(|_| at_boundary(prev)) {
            p.emoji(&m[1]);
            i += m[0].chars().count();
        } else {
            p.buf.push(c);
            i += 1;
        }
    }
    p.flush();
    (p.elements, p.plain)
}

fn at_boundary(prev: Option<char>) -> bool {
    prev.is_none_or(|c| !c.is_alphanumeric())
}

fn toggles_italic(chars: &[char], i: usize, open: bool) -> bool {
    let prev = if i == 0 { None } else { Some(chars[i - 1]) };
    let next = chars.get(i + 1).copied();
    if open {
        prev.is_some_and(|c| !c.is_whitespace()) && next.is_none_or(|c| !c.is_alphanumeric())
    } else {
        at_boundary(prev) && next.is_some_and(|c| !c.is_whitespace())
    }
}

impl Inline<'_> {
    fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.buf);
        self.plain.push_str(&text);
        let mut el = json!({"type": "text", "text": text});
        if let Some(style) = self.style.json() {
            el["style"] = style;
        }
        self.elements.push(el);
    }

    fn toggle(&mut self, change: impl Fn(&mut Style)) {
        self.flush();
        change(&mut self.style);
    }

    fn code(&mut self, code: &str) {
        self.flush();
        self.plain.push_str(code);
        self.elements.push(json!({"type": "text", "text": code, "style": {"code": true}}));
    }

    fn link(&mut self, url: &str, label: &str) {
        self.flush();
        self.plain.push_str(label);
        let mut el = json!({"type": "link", "url": url});
        if label != url {
            el["text"] = json!(label);
        }
        if let Some(style) = self.style.json() {
            el["style"] = style;
        }
        self.elements.push(el);
    }

    fn user(&mut self, id: &str, handle: &str) {
        self.flush();
        self.plain.push_str(&format!("@{handle}"));
        self.elements.push(json!({"type": "user", "user_id": id}));
    }

    fn channel(&mut self, id: &str, name: &str) {
        self.flush();
        self.plain.push_str(&format!("#{name}"));
        self.elements.push(json!({"type": "channel", "channel_id": id}));
    }

    fn emoji(&mut self, name: &str) {
        self.flush();
        self.plain.push_str(&format!(":{name}:"));
        self.elements.push(json!({"type": "emoji", "name": name}));
    }
}

/// Accepts `{"blocks": [...]}` or a bare array, and checks the limits Slack enforces.
pub fn validate_blocks(raw: &str) -> Result<Vec<Value>> {
    let value: Value = serde_json::from_str(raw).map_err(|e| anyhow::anyhow!("blocks are not valid JSON: {e}"))?;
    let blocks = match value {
        Value::Array(a) => a,
        Value::Object(mut o) => match o.remove("blocks") {
            Some(Value::Array(a)) => a,
            _ => bail!("expected a `blocks` array"),
        },
        _ => bail!("expected a `blocks` array"),
    };
    if blocks.is_empty() || blocks.len() > MAX_BLOCKS {
        bail!("Slack accepts 1 to {MAX_BLOCKS} blocks, got {}", blocks.len());
    }
    for (i, block) in blocks.iter().enumerate() {
        let Some(kind) = block["type"].as_str() else { bail!("block {i} has no `type`") };
        if kind == "header" && block["text"]["type"] != "plain_text" {
            bail!("block {i}: header text must be plain_text");
        }
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Team;
    impl Mentions for Team {
        fn user(&self, handle: &str) -> Option<String> {
            (handle == "vivien").then(|| "U1".to_owned())
        }
        fn channel(&self, name: &str) -> Option<String> {
            (name == "general").then(|| "C1".to_owned())
        }
    }

    fn text(t: &str) -> Value {
        json!({"type": "text", "text": t})
    }

    #[test]
    fn plain_paragraph() {
        let r = to_blocks("hello world", &NoMentions);
        assert_eq!(
            r.blocks,
            vec![json!({"type": "rich_text", "elements": [{"type": "rich_text_section", "elements": [text("hello world")]}]})]
        );
        assert_eq!(r.text, "hello world");
    }

    #[test]
    fn inline_styles_and_code() {
        let (els, plain) = inline("a **b** *c* ~~d~~ `e` f", &NoMentions);
        assert_eq!(
            els,
            vec![
                text("a "),
                json!({"type": "text", "text": "b", "style": {"bold": true}}),
                text(" "),
                json!({"type": "text", "text": "c", "style": {"italic": true}}),
                text(" "),
                json!({"type": "text", "text": "d", "style": {"strike": true}}),
                text(" "),
                json!({"type": "text", "text": "e", "style": {"code": true}}),
                text(" f"),
            ]
        );
        assert_eq!(plain, "a b c d e f");
    }

    #[test]
    fn nested_styles_combine() {
        let (els, _) = inline("**bold _both_**", &NoMentions);
        assert_eq!(els[1], json!({"type": "text", "text": "both", "style": {"bold": true, "italic": true}}));
    }

    #[test]
    fn snake_case_and_stars_in_math_survive() {
        let (els, _) = inline("my_var_name and 2*3", &NoMentions);
        assert_eq!(els, vec![text("my_var_name and 2*3")]);
    }

    #[test]
    fn links_bare_and_labelled() {
        let (els, plain) = inline("see [docs](https://a.io) or https://b.io/x.", &NoMentions);
        assert_eq!(
            els,
            vec![
                text("see "),
                json!({"type": "link", "url": "https://a.io", "text": "docs"}),
                text(" or "),
                json!({"type": "link", "url": "https://b.io/x"}),
                text("."),
            ]
        );
        assert_eq!(plain, "see docs or https://b.io/x.");
    }

    #[test]
    fn mentions_resolve_or_stay_text() {
        let (els, plain) = inline("@vivien @nobody #general #random :tada: a@b", &Team);
        assert_eq!(
            els,
            vec![
                json!({"type": "user", "user_id": "U1"}),
                text(" @nobody "),
                json!({"type": "channel", "channel_id": "C1"}),
                text(" #random "),
                json!({"type": "emoji", "name": "tada"}),
                text(" a@b"),
            ]
        );
        assert_eq!(plain, "@vivien @nobody #general #random :tada: a@b");
    }

    #[test]
    fn escapes() {
        let (els, _) = inline(r"\*not bold\*", &NoMentions);
        assert_eq!(els, vec![text("*not bold*")]);
    }

    #[test]
    fn heading_divider_and_lists() {
        let r = to_blocks("# Title\n\n- one\n- two\n  - nested\n1. first\n2. second\n---\nbye", &NoMentions);
        assert_eq!(r.blocks[0], json!({"type": "header", "text": {"type": "plain_text", "text": "Title", "emoji": true}}));
        let lists = &r.blocks[1]["elements"];
        assert_eq!(lists[0]["style"], "bullet");
        assert_eq!(lists[0]["indent"], 0);
        assert_eq!(lists[0]["elements"].as_array().unwrap().len(), 2);
        assert_eq!(lists[1]["indent"], 1);
        assert_eq!(lists[2]["style"], "ordered");
        assert_eq!(r.blocks[2], json!({"type": "divider"}));
        assert_eq!(r.blocks[3]["elements"][0]["elements"][0], text("bye"));
        assert_eq!(r.text, "Title\n• one\n• two\n  • nested\n• first\n• second\nbye");
    }

    #[test]
    fn code_fence_and_quote() {
        let r = to_blocks("```\nls -la\necho hi\n```\n> wise\n> words", &NoMentions);
        let els = &r.blocks[0]["elements"];
        assert_eq!(els[0], json!({"type": "rich_text_preformatted", "elements": [text("ls -la\necho hi")]}));
        assert_eq!(els[1], json!({"type": "rich_text_quote", "elements": [text("wise\nwords")]}));
    }

    #[test]
    fn paragraphs_keep_their_gap() {
        let r = to_blocks("one\ntwo\n\nthree", &NoMentions);
        let els = &r.blocks[0]["elements"];
        assert_eq!(els.as_array().unwrap().len(), 1);
        assert_eq!(els[0]["elements"], json!([text("one\ntwo"), text("\n"), text("\n"), text("three")]));
        assert_eq!(r.text, "one\ntwo\nthree");
    }

    #[test]
    fn header_is_truncated() {
        let long = "x".repeat(200);
        let r = to_blocks(&format!("# {long}"), &NoMentions);
        assert_eq!(r.blocks[0]["text"]["text"].as_str().unwrap().len(), 150);
    }

    #[test]
    fn validate_accepts_both_shapes() {
        assert_eq!(validate_blocks(r#"[{"type":"divider"}]"#).unwrap().len(), 1);
        assert_eq!(validate_blocks(r#"{"blocks":[{"type":"divider"}]}"#).unwrap().len(), 1);
    }

    #[test]
    fn validate_rejects_bad_payloads() {
        assert!(validate_blocks("[]").unwrap_err().to_string().contains("1 to 50"));
        assert!(validate_blocks("{nope").unwrap_err().to_string().contains("valid JSON"));
        assert!(validate_blocks(r#"[{"text":"x"}]"#).unwrap_err().to_string().contains("type"));
        assert!(
            validate_blocks(r#"[{"type":"header","text":{"type":"mrkdwn","text":"x"}}]"#).unwrap_err().to_string().contains("plain_text")
        );
    }
}
