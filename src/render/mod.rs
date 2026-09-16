pub mod text;
pub mod theme;
pub mod time;

pub use text::Styled;
pub use theme::Theme;

use crate::api::{Channel, ChannelKind, Identity, Message, Posted, SearchMatch, User};
use crate::mrkdwn;
use crate::resolve::NameBook;
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;

const TIME_COL: usize = 5;
const NAME_COL: usize = 12;
const GAP: usize = 2;
const TEXT_START: usize = TIME_COL + GAP + NAME_COL + GAP;
const TS_WIDTH: usize = 17;

pub fn whoami(t: &Theme, me: &Identity, workspace: Option<&str>) -> String {
    let host = workspace.map(|w| format!("{w}.slack.com")).unwrap_or_else(|| me.url.clone());
    format!("{} {} @ {} {}\n", t.ok("✓"), t.bold(&me.user), t.accent(&me.team), t.dim(&format!("({host} · {})", me.user_id)))
}

pub fn posted(t: &Theme, label: &str, posted: &Posted, in_thread: bool) -> String {
    let what = if in_thread { "replied in" } else { "sent to" };
    format!("{} {what} {} {}\n", t.ok("✓"), t.accent(label), t.link(&posted.permalink))
}

pub fn title_bar(t: &Theme, title: &str, right: &str) -> String {
    let used = title.width() + right.width() + 6;
    let fill = t.width.saturating_sub(used).max(3);
    format!("{} {} {} {}\n", t.dim("──"), t.title(title), t.dim(&"─".repeat(fill)), t.dim(right))
}

pub fn messages(t: &Theme, names: &NameBook, title: &str, messages: &[Message], replies: &HashMap<String, Vec<Message>>) -> String {
    let mut out = title_bar(t, title, &format!("{} messages", messages.len()));
    if messages.is_empty() {
        out.push_str(&format!("{}\n", t.dim("  nothing here yet")));
        return out;
    }
    let mut last_day = String::new();
    for m in messages {
        let day = time::day_label(&m.ts);
        if day != last_day {
            out.push_str(&format!("{}{}\n", " ".repeat(TEXT_START), t.dim(&format!("── {day}"))));
            last_day = day;
        }
        out.push_str(&message(t, names, m, 0));
        if let Some(thread) = replies.get(&m.ts) {
            for r in thread.iter().filter(|r| r.ts != m.ts) {
                out.push_str(&message(t, names, r, 4));
            }
        }
    }
    out
}

pub fn thread(t: &Theme, names: &NameBook, channel: &str, messages: &[Message]) -> String {
    let Some(root) = messages.first() else {
        return title_bar(t, channel, "empty thread");
    };
    let mut out = title_bar(t, &format!("{channel} thread"), &plural(messages.len().saturating_sub(1) as u64, "reply", "replies"));
    out.push_str(&message(t, names, root, 0));
    for r in &messages[1..] {
        out.push_str(&message(t, names, r, 4));
    }
    out
}

pub fn message(t: &Theme, names: &NameBook, m: &Message, indent: usize) -> String {
    let author = author(names, m);
    let name = fit(&author, NAME_COL);
    let head = format!("{}{}{}{}{}", " ".repeat(indent), t.time(&time::hhmm(&m.ts)), " ".repeat(GAP), t.user(&name), " ".repeat(GAP));
    let show_ts = t.width >= 80;
    let text_width = t.width.saturating_sub(indent + TEXT_START + if show_ts { TS_WIDTH + 2 } else { 0 }).max(20);
    let mut lines = body(t, names, m).wrap_with(text_width, t);
    if lines.is_empty() {
        lines.push(String::new());
    }
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            out.push_str(&head);
            out.push_str(line);
            if show_ts {
                let pad = text_width.saturating_sub(text::visible_width(line)) + 2;
                out.push_str(&format!("{}{}", " ".repeat(pad), t.dim(&m.ts)));
            }
        } else {
            out.push_str(&" ".repeat(indent + TEXT_START));
            out.push_str(line);
        }
        out.push('\n');
    }
    let pad = " ".repeat(indent + TEXT_START);
    for f in &m.files {
        let label = if f.title.is_empty() { &f.name } else { &f.title };
        out.push_str(&format!("{pad}{} {}\n", t.dim("📎"), t.link_labelled(label, &f.permalink)));
    }
    if !m.reactions.is_empty() {
        let r: Vec<String> = m.reactions.iter().map(|r| format!("{} {}", crate::emoji::render(&r.name), r.count)).collect();
        out.push_str(&format!("{pad}{}\n", t.dim(&r.join("  "))));
    }
    if m.is_thread_root() && indent == 0 {
        let when = m.latest_reply.as_deref().map(time::relative).map(|s| format!(" · last {s}")).unwrap_or_default();
        out.push_str(&format!("{pad}{}\n", t.accent(&format!("↳ {}{when}", plural(m.reply_count, "reply", "replies")))));
    }
    out
}

fn author(names: &NameBook, m: &Message) -> String {
    if let Some(u) = &m.user {
        return names.user_label(u);
    }
    if let Some(u) = &m.username {
        return u.clone();
    }
    m.bot_id.clone().map(|_| "bot".to_owned()).unwrap_or_else(|| "?".to_owned())
}

fn body(t: &Theme, names: &NameBook, m: &Message) -> Styled {
    let mut text = m.text.clone();
    if text.is_empty() {
        text = m
            .attachments
            .iter()
            .map(|a| if a.fallback.is_empty() { format!("{}\n{}", a.title, a.text) } else { a.fallback.clone() })
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut styled = match m.subtype.as_deref() {
        Some("channel_join") => Styled::dim("joined the channel"),
        Some("channel_leave") => Styled::dim("left the channel"),
        _ => text::from_segments(&mrkdwn::parse(&text, names), t.show_urls),
    };
    if m.edited.is_some() {
        styled.push_dim(" (edited)");
    }
    styled
}

pub fn search(t: &Theme, result_total: u64, matches: &[SearchMatch]) -> String {
    let mut out = title_bar(t, "search", &format!("{} of {result_total}", matches.len()));
    let mut current = String::new();
    for m in matches {
        let channel = if m.channel.name.is_empty() { m.channel.id.clone() } else { format!("#{}", m.channel.name) };
        if channel != current {
            out.push_str(&format!("{}\n", t.accent(&channel)));
            current = channel;
        }
        let when = format!("{} {}", time::day_label(&m.ts), time::hhmm(&m.ts));
        let head = format!("  {}  {}  ", t.dim(&when), t.user(&fit(&m.username, NAME_COL)));
        let indent = 2 + when.width() + 2 + NAME_COL + 2;
        let lines = text::from_segments(&mrkdwn::parse(&m.text, &mrkdwn::NoNames), t.show_urls)
            .wrap_with(t.width.saturating_sub(indent).max(20), t);
        for (i, line) in lines.iter().enumerate() {
            out.push_str(if i == 0 { &head } else { "" });
            if i > 0 {
                out.push_str(&" ".repeat(indent));
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&format!("{}{}\n", " ".repeat(indent), t.link(&m.permalink)));
    }
    out
}

pub fn channels(t: &Theme, channels: &[(&Channel, String)]) -> String {
    let mut out = String::new();
    let name_width = channels.iter().map(|(_, n)| n.width()).max().unwrap_or(10).min(40);
    for (c, label) in channels {
        let kind = match c.kind() {
            ChannelKind::Public => "",
            ChannelKind::Private => "private",
            ChannelKind::Dm => "dm",
            ChannelKind::GroupDm => "group",
        };
        let topic = c
            .topic
            .as_ref()
            .map(|x| x.value.as_str())
            .filter(|v| !v.is_empty())
            .or_else(|| c.purpose.as_ref().map(|p| p.value.as_str()))
            .unwrap_or("");
        let members = if c.num_members > 0 { format!("{:>5}", c.num_members) } else { "     ".into() };
        let topic = fit(&topic.replace('\n', " "), t.width.saturating_sub(name_width + 20).max(10));
        let line = format!(
            "{}  {} {}  {}",
            t.accent(&pad(label, name_width)),
            t.dim(&members),
            t.dim(&format!("{kind:<7}")),
            t.dim(topic.trim_end())
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

pub fn users(t: &Theme, users: &[User]) -> String {
    let mut out = String::new();
    let handle_width = users.iter().map(|u| u.handle().width() + 1).max().unwrap_or(10).min(30);
    for u in users {
        let real = if u.profile.real_name.is_empty() { &u.real_name } else { &u.profile.real_name };
        let title = if u.profile.title.is_empty() { String::new() } else { format!(" · {}", u.profile.title) };
        let bot = if u.is_bot { t.dim(" (bot)") } else { String::new() };
        out.push_str(&format!(
            "{}  {}{}{}  {}\n",
            t.user(&pad(&format!("@{}", u.handle()), handle_width)),
            real,
            bot,
            t.dim(&title),
            t.dim(&u.id)
        ));
    }
    out
}

pub fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn fit(s: &str, width: usize) -> String {
    if s.width() <= width {
        return pad(s, width);
    }
    let mut out = String::new();
    for c in s.chars() {
        if out.width() + 1 >= width {
            break;
        }
        out.push(c);
    }
    format!("{out}…")
}

fn pad(s: &str, width: usize) -> String {
    format!("{s}{}", " ".repeat(width.saturating_sub(s.width())))
}
