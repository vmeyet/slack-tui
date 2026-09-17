mod commands;
mod incoming;
mod keys;
mod live;
mod sidebar;
mod state;
#[cfg(test)]
mod tests;

pub use sidebar::{Badge, ChannelRow, Kind, SidebarRow, arrange, sidebar_rows};
pub use state::{App, Settings};

use super::jump::Candidate;
use super::palette;
use crate::api::rtm;
use crate::api::{Message, SearchMatch};
use crate::inbox::{Item, State};
use crate::resolve::NameBook;
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
    Filter,
    Search,
    InboxReply { item: Item },
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
    React {
        channel: String,
        ts: String,
        name: String,
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
    Live,
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
    SearchResults(Vec<SearchMatch>),
    Status(String),
    /// A thumbnail came back, decoded, or could not be.
    Thumb {
        id: String,
        image: Option<image::DynamicImage>,
    },
    Error(String),
}
