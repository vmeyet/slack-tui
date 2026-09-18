//! Block Kit blocks as received from the API, decoded into the same styled segments as mrkdwn.
use crate::mrkdwn::{self, Names, Segment};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Slack keeps inventing block shapes, and one we cannot read must never cost the whole message.
pub fn readable<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Block>, D::Error> {
    let raw = Vec::<Value>::deserialize(d)?;
    Ok(raw.into_iter().filter_map(|b| serde_json::from_value(b).ok()).collect())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Block {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    elements: Vec<Part>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<Content>,
}

/// One part of a rich text block: a paragraph, a list, a quote or a code block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Part {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    style: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    indent: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    elements: Vec<Element>,
}

/// A leaf of a rich text part, or a list item holding leaves of its own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Element {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    user_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    channel_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    usergroup_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    range: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    style: Option<Marks>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    elements: Vec<Element>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Marks {
    #[serde(default, skip_serializing_if = "is_unset")]
    bold: bool,
    #[serde(default, skip_serializing_if = "is_unset")]
    italic: bool,
    #[serde(default, skip_serializing_if = "is_unset")]
    strike: bool,
    #[serde(default, skip_serializing_if = "is_unset")]
    code: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Content {
    #[serde(default)]
    text: String,
}

fn is_unset(b: &bool) -> bool {
    !b
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Element {
    fn label(&self) -> &str {
        if self.text.is_empty() { &self.url } else { &self.text }
    }
}

/// The rich blocks Slack sent, or the mrkdwn `text` when they carry nothing to show.
pub fn segments(blocks: &[Block], text: &str, names: &dyn Names) -> Vec<Segment> {
    let rich = trim_end(join(blocks.iter().map(|b| block(b, names))));
    if rich.iter().all(is_blank) { mrkdwn::parse(text, names) } else { rich }
}

fn block(b: &Block, names: &dyn Names) -> Vec<Segment> {
    match &b.text {
        Some(content) => mrkdwn::parse(&content.text, names),
        None => join(b.elements.iter().map(|p| part(p, names))),
    }
}

fn part(p: &Part, names: &dyn Names) -> Vec<Segment> {
    match p.kind.as_str() {
        "rich_text_preformatted" => vec![Segment::Pre(code(&p.elements))],
        "rich_text_quote" => quoted(inline(&p.elements, names)),
        "rich_text_list" => list(p, names),
        _ if !p.text.is_empty() => mrkdwn::parse(&p.text, names),
        _ => inline(&p.elements, names),
    }
}

fn list(p: &Part, names: &dyn Names) -> Vec<Segment> {
    let indent = "  ".repeat(p.indent);
    let items = p.elements.iter().enumerate().map(|(i, item)| {
        let marker = match p.style.as_str() {
            "ordered" => format!("{indent}{}. ", i + 1),
            _ => format!("{indent}• "),
        };
        [vec![Segment::Text(marker)], inline(&item.elements, names)].concat()
    });
    join(items)
}

/// A quote reads as Slack writes it in mrkdwn: every line behind a `>` marker.
fn quoted(segments: Vec<Segment>) -> Vec<Segment> {
    let behind_marker = segments.into_iter().map(|s| match s {
        Segment::Text(t) => Segment::Text(t.replace('\n', "\n> ")),
        other => other,
    });
    std::iter::once(Segment::Text("> ".to_owned())).chain(behind_marker).collect()
}

fn code(elements: &[Element]) -> String {
    elements.iter().map(Element::label).collect()
}

fn inline(elements: &[Element], names: &dyn Names) -> Vec<Segment> {
    elements.iter().map(|e| element(e, names)).collect()
}

fn element(e: &Element, names: &dyn Names) -> Segment {
    match e.kind.as_str() {
        "link" => Segment::Link { label: e.label().to_owned(), url: e.url.clone() },
        "user" => Segment::Mention(names.user(&e.user_id).unwrap_or_else(|| e.user_id.clone())),
        "channel" => Segment::Channel(names.channel(&e.channel_id).unwrap_or_else(|| e.channel_id.clone())),
        "usergroup" => Segment::Mention(group_name(e, names)),
        "broadcast" => Segment::Mention(e.range.clone()),
        "emoji" => Segment::Emoji(e.name.clone()),
        _ => marked(e),
    }
}

/// A usergroup element usually carries its id alone, so the handle has to be looked up.
fn group_name(e: &Element, names: &dyn Names) -> String {
    match e.name.is_empty() {
        true => names.group(&e.usergroup_id).unwrap_or_else(|| e.usergroup_id.clone()),
        false => e.name.clone(),
    }
}

fn marked(e: &Element) -> Segment {
    let text = e.text.clone();
    match e.style.unwrap_or_default() {
        Marks { code: true, .. } => Segment::Code(text),
        Marks { bold: true, .. } => Segment::Bold(text),
        Marks { italic: true, .. } => Segment::Italic(text),
        Marks { strike: true, .. } => Segment::Strike(text),
        _ => Segment::Text(text),
    }
}

/// Stacks groups of segments into one body, a newline between them.
fn join(lines: impl IntoIterator<Item = Vec<Segment>>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for line in lines.into_iter().filter(|l| !l.is_empty()) {
        if !out.is_empty() {
            out.push(Segment::Text("\n".to_owned()));
        }
        out.extend(line);
    }
    out
}

fn trim_end(mut segments: Vec<Segment>) -> Vec<Segment> {
    while let Some(Segment::Text(t)) = segments.last_mut() {
        *t = t.trim_end().to_owned();
        if !t.is_empty() {
            break;
        }
        segments.pop();
    }
    segments
}

fn is_blank(s: &Segment) -> bool {
    matches!(s, Segment::Text(t) if t.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{self, NoMentions};
    use crate::mrkdwn::NoNames;
    use Segment::*;
    use serde_json::json;

    struct Fake;
    impl Names for Fake {
        fn user(&self, id: &str) -> Option<String> {
            (id == "U1").then(|| "vivien".to_owned())
        }
        fn channel(&self, id: &str) -> Option<String> {
            (id == "C1").then(|| "general".to_owned())
        }
        fn group(&self, id: &str) -> Option<String> {
            (id == "S1").then(|| "team-x".to_owned())
        }
    }

    fn blocks(value: Value) -> Vec<Block> {
        serde_json::from_value(value).expect("blocks")
    }

    fn rich(elements: Value) -> Vec<Block> {
        blocks(json!([{"type": "rich_text", "elements": elements}]))
    }

    fn section(elements: Value) -> Vec<Block> {
        rich(json!([{"type": "rich_text_section", "elements": elements}]))
    }

    fn read(blocks: &[Block]) -> Vec<Segment> {
        segments(blocks, "fallback", &Fake)
    }

    /// The lines a reader ends up seeing, so two producers can be compared past their span splits.
    fn screen(segments: &[Segment]) -> Vec<String> {
        crate::render::text::from_segments(segments, false).wrap(60)
    }

    #[test]
    fn text_styles_become_their_segments() {
        let b = section(json!([
            {"type": "text", "text": "b", "style": {"bold": true}},
            {"type": "text", "text": "i", "style": {"italic": true}},
            {"type": "text", "text": "s", "style": {"strike": true}},
            {"type": "text", "text": "c", "style": {"code": true}},
            {"type": "text", "text": " plain"},
        ]));
        assert_eq!(read(&b), vec![Bold("b".into()), Italic("i".into()), Strike("s".into()), Code("c".into()), Text(" plain".into())]);
    }

    #[test]
    fn mentions_links_and_emoji_resolve_like_mrkdwn() {
        let b = section(json!([
            {"type": "user", "user_id": "U1"},
            {"type": "text", "text": " "},
            {"type": "user", "user_id": "U9"},
            {"type": "text", "text": " "},
            {"type": "channel", "channel_id": "C1"},
            {"type": "text", "text": " "},
            {"type": "link", "url": "https://a.io", "text": "docs"},
            {"type": "text", "text": " "},
            {"type": "link", "url": "https://b.io"},
            {"type": "text", "text": " "},
            {"type": "emoji", "name": "rocket"},
            {"type": "text", "text": " "},
            {"type": "broadcast", "range": "here"},
        ]));
        assert_eq!(
            read(&b),
            vec![
                Mention("vivien".into()),
                Text(" ".into()),
                Mention("U9".into()),
                Text(" ".into()),
                Channel("general".into()),
                Text(" ".into()),
                Link { label: "docs".into(), url: "https://a.io".into() },
                Text(" ".into()),
                Link { label: "https://b.io".into(), url: "https://b.io".into() },
                Text(" ".into()),
                Emoji("rocket".into()),
                Text(" ".into()),
                Mention("here".into()),
            ]
        );
    }

    #[test]
    fn usergroups_resolve_to_their_handle() {
        let b = section(json!([
            {"type": "usergroup", "usergroup_id": "S1"},
            {"type": "text", "text": " "},
            {"type": "usergroup", "usergroup_id": "S9"},
            {"type": "text", "text": " "},
            {"type": "usergroup", "usergroup_id": "S9", "name": "team-y"},
        ]));
        assert_eq!(
            read(&b),
            vec![Mention("team-x".into()), Text(" ".into()), Mention("S9".into()), Text(" ".into()), Mention("team-y".into())]
        );
    }

    #[test]
    fn preformatted_and_quote_keep_their_shape() {
        let b = rich(json!([
            {"type": "rich_text_preformatted", "elements": [{"type": "text", "text": "ls -la\necho hi"}]},
            {"type": "rich_text_quote", "elements": [{"type": "text", "text": "wise\nwords"}]},
        ]));
        assert_eq!(read(&b), vec![Pre("ls -la\necho hi".into()), Text("\n".into()), Text("> ".into()), Text("wise\n> words".into())]);
    }

    #[test]
    fn both_list_kinds_get_their_markers() {
        let item = |t: &str| json!({"type": "rich_text_section", "elements": [{"type": "text", "text": t}]});
        let b = rich(json!([
            {"type": "rich_text_list", "style": "bullet", "indent": 0, "elements": [item("one"), item("two")]},
            {"type": "rich_text_list", "style": "bullet", "indent": 1, "elements": [item("deep")]},
            {"type": "rich_text_list", "style": "ordered", "indent": 0, "elements": [item("first"), item("second")]},
        ]));
        assert_eq!(screen(&read(&b)), vec!["• one", "• two", "  • deep", "1. first", "2. second"]);
    }

    #[test]
    fn a_block_without_rich_elements_falls_back_to_its_own_text() {
        let b = blocks(json!([
            {"type": "header", "text": {"type": "plain_text", "text": "Title"}},
            {"type": "divider"},
            {"type": "section", "text": {"type": "mrkdwn", "text": "*loud* <@U1>"}},
        ]));
        assert_eq!(
            read(&b),
            vec![Text("Title".into()), Text("\n".into()), Bold("loud".into()), Text(" ".into()), Mention("vivien".into())]
        );
    }

    #[test]
    fn unknown_kinds_degrade_to_their_text() {
        let b = blocks(json!([
            {"type": "video", "title": "clip"},
            {"type": "rich_text", "elements": [
                {"type": "rich_text_unheard_of", "elements": [{"type": "text", "text": "kept"}]},
                {"type": "rich_text_section", "elements": [{"type": "text", "text": " and "}, {"type": "sticker", "text": "this"}]},
            ]},
        ]));
        assert_eq!(read(&b), vec![Text("kept".into()), Text("\n".into()), Text(" and ".into()), Text("this".into())]);
    }

    #[test]
    fn nothing_to_show_falls_back_to_the_message_text() {
        let fallback = vec![Text("fallback".into())];
        assert_eq!(read(&[]), fallback);
        assert_eq!(read(&blocks(json!([{"type": "divider"}]))), fallback);
        assert_eq!(read(&section(json!([{"type": "text", "text": "  "}]))), fallback);
        assert_eq!(read(&blocks(json!([{"type": "rich_text"}]))), fallback);
        assert_eq!(segments(&blocks(json!([{"type": "divider"}])), "", &Fake), vec![]);
    }

    #[test]
    fn what_we_send_comes_back_styled_the_way_slack_writes_it() {
        let sent = markdown::to_blocks("**bold** and `code` and _soft_\n> quoted\n```\nls -la\n```", &NoMentions);
        let mine = screen(&segments(&blocks(json!(sent.blocks)), &sent.text, &NoNames));
        let theirs = screen(&mrkdwn::parse("*bold* and `code` and _soft_\n&gt; quoted\n```\nls -la\n```", &NoNames));
        assert_eq!(mine, theirs);
        assert_ne!(mine, screen(&mrkdwn::parse(&sent.text, &NoNames)), "the flat fallback would have lost the styling");
    }

    #[test]
    fn what_we_send_keeps_its_lists_and_mentions() {
        struct Team;
        impl markdown::Mentions for Team {
            fn user(&self, handle: &str) -> Option<String> {
                (handle == "vivien").then(|| "U1".to_owned())
            }
            fn channel(&self, name: &str) -> Option<String> {
                (name == "general").then(|| "C1".to_owned())
            }
        }
        let sent = markdown::to_blocks("hi @vivien in #general\n- one\n- two", &Team);
        let mine = segments(&blocks(json!(sent.blocks)), &sent.text, &Fake);
        assert_eq!(screen(&mine), screen(&mrkdwn::parse("hi <@U1> in <#C1>\n• one\n• two", &Fake)));
        assert_eq!(mine.iter().filter(|s| matches!(s, Mention(_) | Channel(_))).count(), 2);
    }

    #[test]
    fn a_block_we_cannot_read_costs_only_itself() {
        let raw = json!({"ts": "1.0", "text": "press it", "blocks": [
            {"type": "actions", "elements": [{"type": "button", "text": {"type": "plain_text", "text": "go"}}]},
            {"type": "rich_text", "elements": [{"type": "rich_text_section", "elements": [{"type": "text", "text": "press it"}]}]},
        ]});
        let m: crate::api::Message = serde_json::from_value(raw).expect("a readable message");
        assert_eq!(m.blocks.len(), 1);
        assert_eq!(segments(&m.blocks, &m.text, &NoNames), vec![Text("press it".into())]);
    }

    #[test]
    fn blocks_round_trip_through_serde_without_growing() {
        let raw = json!([{"type": "rich_text", "elements": [{"type": "rich_text_section", "elements": [
            {"type": "text", "text": "b", "style": {"bold": true}},
            {"type": "user", "user_id": "U1"},
        ]}]}]);
        assert_eq!(serde_json::to_value(blocks(raw.clone())).unwrap(), raw);
    }
}
