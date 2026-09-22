mod commands;
mod feedback;
mod incoming;
mod keys;
mod live;
mod sidebar;
mod state;
#[cfg(test)]
mod tests;

pub use feedback::{Toast, Typing};
pub use sidebar::{Badge, ChannelRow, Kind, SidebarRow, arrange, sidebar_rows};
pub use state::{App, Settings};

use super::jump::Candidate;
use super::palette;
use crate::api::rtm;
use crate::api::{Message, SearchMatch};
use crate::firehose::{Line as LiveLine, Tag};
use crate::inbox::{Item, State, Verdicts};
use crate::resolve::NameBook;
use crate::typesafe::Unavailable;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Channels,
    Messages,
    Thread,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Reply { channel: String, thread_ts: Option<String>, label: String },
    React { channel: String, ts: String },
    Edit { channel: String, ts: String },
    Filter,
    Search,
    InboxReply { item: Item },
}

/// The selected message, once it is the signed-in user's own: all `:edit` and `:delete` may touch.
#[derive(Clone, Debug, PartialEq)]
pub struct MyMessage {
    pub channel: String,
    pub ts: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub channel: String,
    pub root_ts: String,
    pub messages: Vec<Message>,
    pub selected: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    LoadChannels,
    CheckUpdate,
    LoadHistory(String),
    LoadReplies {
        channel: String,
        ts: String,
    },
    Send {
        channel: String,
        thread_ts: Option<String>,
        text: String,
    },
    /// Run by the event loop itself, never a background task: the editor takes the terminal.
    Compose {
        channel: String,
        thread_ts: Option<String>,
        draft: String,
    },
    React {
        channel: String,
        ts: String,
        name: String,
    },
    Edit {
        channel: String,
        ts: String,
        text: String,
    },
    Delete {
        channel: String,
        ts: String,
    },
    Search(String),
    Open {
        channel: String,
        ts: String,
    },
    OpenUrl(String),
    Yank {
        channel: String,
        ts: String,
    },
    LoadInbox,
    /// Ask Jev how pressing each inbox item is.
    Prioritize(Vec<Item>),
    /// Ask Jev what kind of live message this is.
    Classify(LiveLine),
    /// Ask Jev which of your recent messages promised a follow-up you have not closed.
    LoadPromises,
    LoadThreads,
    Join(String),
    Leave(String),
    SendTo {
        target: String,
        text: String,
    },
    MarkChannelRead {
        channel: String,
        ts: String,
    },
    /// Like `MarkChannelRead`, without a toast: the user only opened or watched the channel.
    SyncRead {
        channel: String,
        ts: String,
    },
    Export {
        path: PathBuf,
        label: String,
        messages: Vec<Message>,
        format: palette::Format,
    },
    LearnUsers(Vec<String>),
    OpenDm(String),
    MarkRead(Item),
    SaveInbox {
        workspace: String,
        state: State,
    },
    /// A `[tui]` key already validated by the app, written to the config file.
    SaveSetting {
        key: String,
        value: String,
    },
    LoadImage {
        id: String,
        url: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Live {
    #[default]
    Connecting,
    Connected,
    Polling(String),
}

#[derive(Clone, Debug)]
pub enum Incoming {
    Live(Box<rtm::Event>),
    Tick,
    Inbox {
        items: Vec<Item>,
        names: NameBook,
    },
    Priorities(Verdicts),
    Tagged {
        channel: String,
        ts: String,
        tag: Tag,
    },
    /// Jev could not answer; triage stays off for the rest of the session.
    TriageUnavailable(Unavailable),
    Threads(Vec<Candidate>),
    DmOpened(String),
    Names(NameBook),
    Joined(String),
    Left(String),
    Channels {
        rows: Vec<ChannelRow>,
        people: Vec<(String, String)>,
        names: NameBook,
        badges: HashMap<String, Badge>,
        me: String,
    },
    History {
        channel: String,
        messages: Vec<Message>,
        names: NameBook,
    },
    Replies {
        channel: String,
        ts: String,
        messages: Vec<Message>,
        names: NameBook,
    },
    Sent {
        channel: String,
        thread_ts: Option<String>,
    },
    /// What came back from the editor, already worth sending.
    Composed {
        channel: String,
        thread_ts: Option<String>,
        text: String,
    },
    SearchResults(Vec<SearchMatch>),
    Toast(String),
    /// The daily update check answered, `None` when it could not.
    Latest(Option<String>),
    /// A thumbnail came back, decoded, or could not be.
    Thumb {
        id: String,
        image: Option<image::DynamicImage>,
    },
    Error(String),
}
