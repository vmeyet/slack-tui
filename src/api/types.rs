use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub team_id: String,
    pub team: String,
    pub user_id: String,
    pub user: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub is_im: bool,
    #[serde(default)]
    pub is_mpim: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub is_member: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default)]
    pub num_members: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<Topic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<Topic>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    #[serde(default)]
    pub value: String,
}

impl Channel {
    pub fn kind(&self) -> ChannelKind {
        if self.is_im {
            ChannelKind::Dm
        } else if self.is_mpim {
            ChannelKind::GroupDm
        } else if self.is_private {
            ChannelKind::Private
        } else {
            ChannelKind::Public
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    Public,
    Private,
    Dm,
    GroupDm,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub real_name: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub profile: Profile,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub real_name: String,
    #[serde(default)]
    pub title: String,
}

impl User {
    pub fn handle(&self) -> &str {
        if !self.profile.display_name.is_empty() {
            &self.profile.display_name
        } else if !self.name.is_empty() {
            &self.name
        } else {
            &self.real_name
        }
    }
}

/// A usergroup, mentioned as `@handle` and carried in messages as its `S…` id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    #[serde(default)]
    pub handle: String,
    /// 0 while the group is live, the disband time once it is gone.
    #[serde(default)]
    pub date_delete: u64,
    #[serde(default)]
    pub users: Vec<String>,
}

impl Group {
    pub fn is_live(&self) -> bool {
        self.date_delete == 0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub ts: String,
    #[serde(default)]
    pub text: String,
    /// The rich rendering of `text`, kept so a formatted message reads the same here as in Slack.
    #[serde(default, deserialize_with = "crate::blocks::readable", skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<crate::blocks::Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    #[serde(default)]
    pub reply_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_reply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<Edited>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<Reaction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<File>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
}

impl Message {
    pub fn is_thread_root(&self) -> bool {
        self.reply_count > 0 && self.thread_ts.as_deref().is_none_or(|t| t == self.ts)
    }

    pub fn is_reply(&self) -> bool {
        self.thread_ts.as_deref().is_some_and(|t| t != self.ts)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Edited {
    #[serde(default)]
    pub ts: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reaction {
    pub name: String,
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct File {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub permalink: String,
    #[serde(default)]
    pub mimetype: String,
    #[serde(default)]
    pub thumb_360: String,
    #[serde(default)]
    pub thumb_360_w: u32,
    #[serde(default)]
    pub thumb_360_h: u32,
    #[serde(default)]
    pub thumb_720: String,
    #[serde(default)]
    pub thumb_720_w: u32,
    #[serde(default)]
    pub thumb_720_h: u32,
}

/// A thumbnail Slack already rendered for an image file.
#[derive(Clone, Debug, PartialEq)]
pub struct Thumb<'a> {
    pub url: &'a str,
    pub width: u32,
    pub height: u32,
}

impl File {
    pub fn label(&self) -> &str {
        if self.title.is_empty() { &self.name } else { &self.title }
    }

    /// The sharpest thumbnail of an image file, none for other files or when Slack made none.
    pub fn thumb(&self) -> Option<Thumb<'_>> {
        if !self.mimetype.starts_with("image/") {
            return None;
        }
        let candidates = [(&self.thumb_720, self.thumb_720_w, self.thumb_720_h), (&self.thumb_360, self.thumb_360_w, self.thumb_360_h)];
        candidates.into_iter().find(|(url, w, h)| !url.is_empty() && *w > 0 && *h > 0).map(|(url, width, height)| Thumb {
            url,
            width,
            height,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub fallback: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub ts: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub permalink: String,
    #[serde(default)]
    pub channel: SearchChannel,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchChannel {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub total: u64,
    pub matches: Vec<SearchMatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    #[serde(default)]
    pub channels: Vec<ReadState>,
    #[serde(default)]
    pub ims: Vec<ReadState>,
    #[serde(default)]
    pub mpims: Vec<ReadState>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReadState {
    pub id: String,
    #[serde(default)]
    pub last_read: String,
    #[serde(default)]
    pub latest: String,
    #[serde(default)]
    pub mention_count: u64,
    #[serde(default)]
    pub has_unreads: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub channel_ids_page: SectionPage,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SectionPage {
    #[serde(default)]
    pub channel_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThreadView {
    pub root_msg: ThreadRoot,
    #[serde(default)]
    pub latest_replies: Vec<Message>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThreadRoot {
    #[serde(default)]
    pub channel: String,
    pub ts: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default)]
    pub last_read: String,
    #[serde(default)]
    pub latest_reply: String,
    #[serde(default)]
    pub reply_count: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Posted {
    pub channel: String,
    pub ts: String,
    #[serde(default)]
    pub permalink: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn message_thread_detection() {
        let root = Message { ts: "1.0".into(), reply_count: 2, thread_ts: Some("1.0".into()), ..Default::default() };
        assert!(root.is_thread_root());
        assert!(!root.is_reply());
        let reply = Message { ts: "2.0".into(), thread_ts: Some("1.0".into()), ..Default::default() };
        assert!(reply.is_reply());
        assert!(!reply.is_thread_root());
        assert!(!Message { ts: "3.0".into(), ..Default::default() }.is_thread_root());
    }

    #[test]
    fn user_handle_prefers_display_name() {
        let mut u = User { name: "vmeyet".into(), real_name: "Vivien Meyet".into(), ..Default::default() };
        assert_eq!(u.handle(), "vmeyet");
        u.profile.display_name = "vivien".into();
        assert_eq!(u.handle(), "vivien");
    }

    #[test]
    fn channel_kind() {
        assert_eq!(Channel { is_im: true, ..Default::default() }.kind(), ChannelKind::Dm);
        assert_eq!(Channel { is_mpim: true, ..Default::default() }.kind(), ChannelKind::GroupDm);
        assert_eq!(Channel { is_private: true, ..Default::default() }.kind(), ChannelKind::Private);
        assert_eq!(Channel::default().kind(), ChannelKind::Public);
    }

    #[test]
    fn tolerates_sparse_payloads() {
        let m: Message = serde_json::from_str(r#"{"ts":"1.2","text":"hi","user":"U1"}"#).unwrap();
        assert_eq!(m.user.as_deref(), Some("U1"));
        let c: Channel = serde_json::from_str(r#"{"id":"C1"}"#).unwrap();
        assert_eq!(c.name, "");
    }
}
