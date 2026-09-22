use super::{Action, App, Badge, ChannelRow, Focus, Incoming, Live, Thread};
use crate::api::{Message, SearchMatch};
use crate::inbox::Item;
use crate::resolve::NameBook;
use crate::tui::firehose;
use ratatui::widgets::ListState;
use std::collections::HashMap;

impl App {
    pub fn apply(&mut self, incoming: Incoming) -> Vec<Action> {
        let actions = self.route(incoming);
        self.mark_seen();
        actions
    }

    fn route(&mut self, incoming: Incoming) -> Vec<Action> {
        match incoming {
            Incoming::Live(event) => return self.apply_live(*event),
            Incoming::Tick => return self.poll(),
            Incoming::DmOpened(channel) => return self.open_channel(channel),
            Incoming::Joined(channel) => return self.joined(channel),
            Incoming::Left(channel) => return self.left(&channel),
            Incoming::History { channel, messages, names } => return self.history_loaded(&channel, messages, names),
            Incoming::Replies { channel, ts, messages, names } => return self.replies_loaded(channel, ts, messages, names),
            Incoming::Sent { channel, thread_ts } => return self.sent(channel, thread_ts),
            Incoming::Composed { channel, thread_ts, text } => return self.composed(channel, thread_ts, text),
            Incoming::Channels { rows, people, names, badges, me } => self.channels_loaded(rows, people, names, badges, me),
            Incoming::SearchResults(matches) => self.search_loaded(matches),
            Incoming::Inbox { items, names } => return self.inbox_loaded(items, names),
            Incoming::Priorities(verdicts) => {
                if let Some(inbox) = &mut self.inbox {
                    inbox.rank(&verdicts);
                }
            }
            Incoming::Tagged { channel, ts, tag } => firehose::tag(&mut self.wall, &channel, &ts, tag),
            Incoming::TriageUnavailable(unavailable) => {
                if std::mem::take(&mut self.triage) {
                    self.toast(unavailable.notice());
                }
            }
            Incoming::Threads(candidates) => {
                if let Some(jump) = &mut self.jump {
                    jump.threads = candidates;
                }
            }
            Incoming::Names(names) => self.names = names,
            Incoming::Latest(commit) => self.latest = commit,
            Incoming::Thumb { id, image } => self.thumbs.arrived(&id, image),
            Incoming::Toast(text) if text.is_empty() => {}
            Incoming::Toast(text) => {
                self.loading = false;
                self.toast(text);
            }
            Incoming::Error(e) => {
                self.loading = false;
                self.fail(e);
            }
        }
        vec![]
    }

    fn inbox_loaded(&mut self, items: Vec<Item>, names: NameBook) -> Vec<Action> {
        self.names = names;
        let Some(inbox) = &mut self.inbox else { return vec![] };
        inbox.set_items(items);
        if !self.triage || inbox.items.is_empty() {
            return vec![];
        }
        vec![Action::Prioritize(inbox.items.clone())]
    }

    fn joined(&mut self, channel: String) -> Vec<Action> {
        let mut actions = vec![Action::LoadChannels];
        actions.extend(self.open_channel(channel));
        actions
    }

    fn left(&mut self, channel: &str) -> Vec<Action> {
        if self.current_channel.as_deref() == Some(channel) {
            self.current_channel = None;
            self.messages.clear();
            self.typing.clear();
            self.thread = None;
            self.focus = Focus::Channels;
        }
        self.toast("left");
        vec![Action::LoadChannels]
    }

    fn channels_loaded(
        &mut self,
        rows: Vec<ChannelRow>,
        people: Vec<(String, String)>,
        names: NameBook,
        badges: HashMap<String, Badge>,
        me: String,
    ) {
        self.loading = false;
        self.channels = rows;
        self.people = people;
        self.names = names;
        self.me = me;
        self.badges = badges;
        self.unread = self.badges.iter().filter(|(_, b)| b.unread).map(|(id, _)| id.clone()).collect();
    }

    fn history_loaded(&mut self, channel: &str, messages: Vec<Message>, names: NameBook) -> Vec<Action> {
        self.loading = false;
        if self.current_channel.as_deref() != Some(channel) {
            return vec![];
        }
        self.names = names;
        let at_bottom = self.message_selected + 1 >= self.messages.len();
        let selected_ts = self.messages.get(self.message_selected).map(|m| m.ts.clone());
        let kept = selected_ts.filter(|_| !at_bottom).and_then(|ts| messages.iter().position(|m| m.ts == ts));
        self.message_selected = kept.unwrap_or(messages.len().saturating_sub(1));
        self.messages = messages;
        let mut actions = self.refresh_thumbs();
        actions.extend(self.sync_read());
        actions
    }

    fn replies_loaded(&mut self, channel: String, ts: String, messages: Vec<Message>, names: NameBook) -> Vec<Action> {
        self.loading = false;
        self.names = names;
        let selected = messages.len().saturating_sub(1);
        if self.thread.as_ref().is_none_or(|t| t.root_ts != ts) {
            self.thread_view = ListState::default();
        }
        self.thread = Some(Thread { channel, root_ts: ts, messages, selected });
        self.refresh_thumbs()
    }

    /// The editor replaces the input row: what it wrote is sent, the row goes back to empty.
    fn composed(&mut self, channel: String, thread_ts: Option<String>, text: String) -> Vec<Action> {
        self.input = None;
        self.buffer.clear();
        self.send(channel, thread_ts, text)
    }

    fn sent(&mut self, channel: String, thread_ts: Option<String>) -> Vec<Action> {
        self.toast("sent ✓");
        if self.current_channel.as_deref() != Some(&channel) {
            self.loading = false;
            if let Some(inbox) = &mut self.inbox {
                inbox.flash = "sent ✓".into();
            }
            return vec![];
        }
        self.loading = true;
        match thread_ts {
            Some(ts) => vec![Action::LoadReplies { channel: channel.clone(), ts }, Action::LoadHistory(channel)],
            None => vec![Action::LoadHistory(channel)],
        }
    }

    fn search_loaded(&mut self, matches: Vec<SearchMatch>) {
        self.loading = false;
        self.message_selected = 0;
        self.search = Some(matches);
        self.focus = Focus::Messages;
    }

    /// Without a live feed, the open conversation is refreshed on every tick.
    /// Live messages in the open channel are marked read here, so a burst costs one call per tick.
    fn poll(&mut self) -> Vec<Action> {
        let synced = self.sync_read();
        let reload = self.reload_when_offline();
        synced.into_iter().chain(reload).collect()
    }

    fn reload_when_offline(&self) -> Option<Action> {
        if self.live == Live::Connected || self.loading || self.input.is_some() {
            return None;
        }
        self.current_channel.clone().map(Action::LoadHistory)
    }
}
