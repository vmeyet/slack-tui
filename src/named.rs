//! What `--json` prints: Slack's fields with their ids, and the names `NameBook` knows beside them.
use crate::api::Message;
use crate::firehose::Line;
use crate::inbox::Item;
use crate::mrkdwn::label_mentions;
use crate::resolve::NameBook;
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub struct NamedMessage {
    #[serde(flatten)]
    message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_name: Option<String>,
}

#[derive(Serialize)]
pub struct NamedLine {
    #[serde(flatten)]
    line: Line,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_name: Option<String>,
}

pub fn messages(messages: &[Message], names: &NameBook) -> Vec<NamedMessage> {
    messages.iter().map(|m| message(m, names)).collect()
}

fn message(m: &Message, names: &NameBook) -> NamedMessage {
    NamedMessage {
        user_name: names.author_name(m.user.as_deref(), m.username.as_deref()),
        message: Message { text: label_mentions(&m.text, names), ..m.clone() },
    }
}

pub fn line(line: &Line, names: &NameBook) -> NamedLine {
    NamedLine {
        user_name: names.author_name(line.user.as_deref(), line.username.as_deref()),
        channel_name: names.channel_name(&line.channel),
        line: Line { text: label_mentions(&line.text, names), ..line.clone() },
    }
}

pub fn items(items: &[Item], names: &NameBook) -> Result<Vec<Value>> {
    items.iter().map(|i| item(i, names)).collect()
}

fn item(item: &Item, names: &NameBook) -> Result<Value> {
    let unread = serde_json::to_value(messages(&item.unread, names))?;
    let mut json = serde_json::to_value(item)?;
    json["unread"] = unread;
    Ok(json)
}
