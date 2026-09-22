//! Slack mrkdwn as received from the API, decoded into styled segments for display.
use crate::pattern::regex;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Bold(String),
    Italic(String),
    Strike(String),
    Code(String),
    Pre(String),
    Link { label: String, url: String },
    Mention(String),
    Channel(String),
    Emoji(String),
}

pub trait Names {
    fn user(&self, id: &str) -> Option<String>;
    fn channel(&self, id: &str) -> Option<String>;
    fn group(&self, id: &str) -> Option<String>;
}

pub struct NoNames;

impl Names for NoNames {
    fn user(&self, _: &str) -> Option<String> {
        None
    }
    fn channel(&self, _: &str) -> Option<String> {
        None
    }
    fn group(&self, _: &str) -> Option<String> {
        None
    }
}

static ANGLE: LazyLock<Regex> = LazyLock::new(|| regex(r"<([^<>]+)>"));
static EMOJI: LazyLock<Regex> = LazyLock::new(|| regex(r"^:([a-z0-9_+-]+(?:::skin-tone-\d)?):"));

pub fn parse(text: &str, names: &dyn Names) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut last = 0;
    for m in ANGLE.find_iter(text) {
        push_formatted(&mut out, &text[last..m.start()]);
        out.push(angle_segment(&m.as_str()[1..m.len() - 1], names));
        last = m.end();
    }
    push_formatted(&mut out, &text[last..]);
    out
}

pub fn plain(text: &str, names: &dyn Names) -> String {
    parse(text, names).iter().map(segment_text).collect()
}

fn segment_text(s: &Segment) -> String {
    match s {
        Segment::Text(t) | Segment::Bold(t) | Segment::Italic(t) | Segment::Strike(t) | Segment::Code(t) | Segment::Pre(t) => t.clone(),
        Segment::Link { label, url } if label == url => url.clone(),
        Segment::Link { label, url } => format!("{label} ({url})"),
        Segment::Mention(n) => format!("@{n}"),
        Segment::Channel(n) => format!("#{n}"),
        Segment::Emoji(n) => crate::emoji::render(n),
    }
}

fn angle_segment(inner: &str, names: &dyn Names) -> Segment {
    let (target, label) = match inner.split_once('|') {
        Some((t, l)) => (t, Some(l)),
        None => (inner, None),
    };
    if let Some(id) = target.strip_prefix('@') {
        return Segment::Mention(label.map(str::to_owned).or_else(|| names.user(id)).unwrap_or_else(|| id.to_owned()));
    }
    if let Some(id) = target.strip_prefix('#') {
        return Segment::Channel(label.map(str::to_owned).or_else(|| names.channel(id)).unwrap_or_else(|| id.to_owned()));
    }
    if let Some(special) = target.strip_prefix('!') {
        return Segment::Mention(special_name(special, label, names));
    }
    Segment::Link { label: unescape(label.unwrap_or(target)), url: target.to_owned() }
}

/// `<!here>` and friends name themselves; `<!subteam^S1>` only carries an id to look up.
fn special_name(special: &str, label: Option<&str>, names: &dyn Names) -> String {
    let name = match special.strip_prefix("subteam^") {
        Some(id) => label.map(str::to_owned).or_else(|| names.group(id)).unwrap_or_else(|| id.to_owned()),
        None => label.unwrap_or(special).to_owned(),
    };
    name.trim_start_matches('@').to_owned()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn push_formatted(out: &mut Vec<Segment>, raw: &str) {
    let text = unescape(raw);
    let chars: Vec<char> = text.chars().collect();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&['`', '`', '`'])
            && let Some(end) = find_seq(&chars, i + 3, &['`', '`', '`'])
        {
            flush(out, &mut buf);
            out.push(Segment::Pre(chars[i + 3..end].iter().collect::<String>().trim_matches('\n').to_owned()));
            i = end + 3;
            continue;
        }
        let c = chars[i];
        if c == ':'
            && let Some(m) = EMOJI.captures(&chars[i..].iter().collect::<String>())
        {
            flush(out, &mut buf);
            out.push(Segment::Emoji(m[1].to_owned()));
            i += m[0].chars().count();
            continue;
        }
        if matches!(c, '*' | '_' | '~' | '`')
            && opens_at(&chars, i)
            && let Some(end) = close_of(&chars, i, c)
        {
            flush(out, &mut buf);
            let inner: String = chars[i + 1..end].iter().collect();
            out.push(match c {
                '*' => Segment::Bold(inner),
                '_' => Segment::Italic(inner),
                '~' => Segment::Strike(inner),
                _ => Segment::Code(inner),
            });
            i = end + 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush(out, &mut buf);
}

fn flush(out: &mut Vec<Segment>, buf: &mut String) {
    if !buf.is_empty() {
        out.push(Segment::Text(std::mem::take(buf)));
    }
}

fn find_seq(chars: &[char], from: usize, seq: &[char]) -> Option<usize> {
    (from..chars.len().saturating_sub(seq.len() - 1)).find(|&i| chars[i..].starts_with(seq))
}

fn opens_at(chars: &[char], i: usize) -> bool {
    let prev_ok = i == 0 || !chars[i - 1].is_alphanumeric();
    let next_ok = chars.get(i + 1).is_some_and(|c| !c.is_whitespace() && *c != chars[i]);
    prev_ok && next_ok
}

fn close_of(chars: &[char], open: usize, marker: char) -> Option<usize> {
    let mut j = open + 1;
    while j < chars.len() {
        if chars[j] == '\n' && marker != '`' {
            return None;
        }
        if chars[j] == marker && !chars[j - 1].is_whitespace() && chars.get(j + 1).is_none_or(|c| !c.is_alphanumeric()) {
            return Some(j);
        }
        j += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use Segment::*;

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

    #[test]
    fn usergroups_resolve_to_their_handle() {
        assert_eq!(parse("ping <!subteam^S1>", &Fake), vec![Text("ping ".into()), Mention("team-x".into())]);
        assert_eq!(parse("ping <!subteam^S9>", &Fake), vec![Text("ping ".into()), Mention("S9".into())]);
        assert_eq!(parse("ping <!subteam^S1>", &NoNames), vec![Text("ping ".into()), Mention("S1".into())]);
    }

    #[test]
    fn mentions_channels_and_specials() {
        assert_eq!(
            parse("hey <@U1> see <#C1|general> and <#C2> <!here> <!subteam^S1|@team>", &Fake),
            vec![
                Text("hey ".into()),
                Mention("vivien".into()),
                Text(" see ".into()),
                Channel("general".into()),
                Text(" and ".into()),
                Channel("C2".into()),
                Text(" ".into()),
                Mention("here".into()),
                Text(" ".into()),
                Mention("team".into()),
            ]
        );
    }

    #[test]
    fn links_with_and_without_label() {
        assert_eq!(
            parse("<https://a.io|Docs> <https://b.io>", &NoNames),
            vec![
                Link { label: "Docs".into(), url: "https://a.io".into() },
                Text(" ".into()),
                Link { label: "https://b.io".into(), url: "https://b.io".into() },
            ]
        );
    }

    #[test]
    fn inline_styles() {
        assert_eq!(
            parse("*bold* _it_ ~gone~ `x = 1`", &NoNames),
            vec![
                Bold("bold".into()),
                Text(" ".into()),
                Italic("it".into()),
                Text(" ".into()),
                Strike("gone".into()),
                Text(" ".into()),
                Code("x = 1".into()),
            ]
        );
    }

    #[test]
    fn snake_case_and_math_are_left_alone() {
        assert_eq!(parse("my_var_name 2*3*4", &NoNames), vec![Text("my_var_name 2*3*4".into())]);
    }

    #[test]
    fn code_fence_becomes_pre() {
        assert_eq!(parse("run\n```\nls -la\n```", &NoNames), vec![Text("run\n".into()), Pre("ls -la".into())]);
    }

    #[test]
    fn emoji_shortcodes_become_segments() {
        assert_eq!(
            parse("ship it :rocket: 12:30 :partyparrot:", &NoNames),
            vec![Text("ship it ".into()), Emoji("rocket".into()), Text(" 12:30 ".into()), Emoji("partyparrot".into())]
        );
        assert_eq!(plain("go :tada: :custom_one:", &NoNames), "go 🎉 :custom_one:");
    }

    #[test]
    fn entities_are_unescaped() {
        assert_eq!(plain("a &amp; b &lt;c&gt;", &NoNames), "a & b <c>");
    }

    #[test]
    fn plain_flattens_everything() {
        assert_eq!(plain("hi <@U1>, *see* <https://a.io|docs>", &Fake), "hi @vivien, see docs (https://a.io)");
    }
}
