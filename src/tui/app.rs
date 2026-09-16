use super::inbox::Inbox;
use super::jump::{Candidate, Jump, Target};
use crate::api::rtm;
use crate::api::{ChannelKind, Message, Reaction, SearchMatch};
use crate::inbox::{Item, Snooze, State};
use crate::resolve::NameBook;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;

pub const DEFAULT_HIGHLIGHT: Color = Color::Indexed(236);
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Public,
    Private,
    Dm,
    GroupDm,
}

impl From<ChannelKind> for Kind {
    fn from(k: ChannelKind) -> Self {
        match k {
            ChannelKind::Public => Kind::Public,
            ChannelKind::Private => Kind::Private,
            ChannelKind::Dm => Kind::Dm,
            ChannelKind::GroupDm => Kind::GroupDm,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelRow {
    pub id: String,
    pub label: String,
    pub kind: Kind,
}

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
    LoadReplies { channel: String, ts: String },
    Send { channel: String, thread_ts: Option<String>, text: String },
    React { channel: String, ts: String, name: String },
    Search(String),
    Open { channel: String, ts: String },
    OpenUrl(String),
    Yank { channel: String, ts: String },
    LoadInbox,
    LoadThreads,
    OpenDm(String),
    MarkRead(Item),
    SaveInbox { workspace: String, state: State },
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
    Live(rtm::Event),
    Tick,
    Inbox { items: Vec<Item>, names: NameBook },
    Threads(Vec<Candidate>),
    DmOpened(String),
    Channels { rows: Vec<ChannelRow>, people: Vec<(String, String)>, names: NameBook },
    History { channel: String, messages: Vec<Message>, names: NameBook },
    Replies { channel: String, ts: String, messages: Vec<Message>, names: NameBook },
    Sent { channel: String, thread_ts: Option<String> },
    SearchResults(Vec<SearchMatch>),
    Status(String),
    Error(String),
}

#[derive(Debug)]
pub struct App {
    pub channels: Vec<ChannelRow>,
    pub filter: String,
    pub channel_selected: usize,
    pub current_channel: Option<String>,
    pub messages: Vec<Message>,
    pub message_selected: usize,
    pub thread: Option<Thread>,
    pub search: Option<Vec<SearchMatch>>,
    pub focus: Focus,
    pub input: Option<Input>,
    pub buffer: String,
    pub status: String,
    pub loading: bool,
    pub names: NameBook,
    pub help: bool,
    pub should_quit: bool,
    pub live: Live,
    pub unread: HashSet<String>,
    pub highlight: Color,
    pub inbox: Option<Inbox>,
    pub workspace: String,
    pub jump: Option<Jump>,
    pub people: Vec<(String, String)>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            channels: vec![],
            filter: String::new(),
            channel_selected: 0,
            current_channel: None,
            messages: vec![],
            message_selected: 0,
            thread: None,
            search: None,
            focus: Focus::default(),
            input: None,
            buffer: String::new(),
            status: String::new(),
            loading: false,
            names: NameBook::default(),
            help: false,
            should_quit: false,
            live: Live::default(),
            unread: HashSet::new(),
            highlight: DEFAULT_HIGHLIGHT,
            inbox: None,
            workspace: "env".into(),
            jump: None,
            people: vec![],
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self { status: "loading channels…".into(), loading: true, ..Default::default() }
    }

    pub fn visible_channels(&self) -> Vec<&ChannelRow> {
        let f = self.filter.to_lowercase();
        self.channels.iter().filter(|c| c.label.to_lowercase().contains(&f)).collect()
    }

    pub fn current_label(&self) -> String {
        self.current_channel.as_deref().map(|id| self.names.channel_label(id)).unwrap_or_default()
    }

    pub fn selected_message(&self) -> Option<&Message> {
        match self.focus {
            Focus::Thread => self.thread.as_ref().and_then(|t| t.messages.get(t.selected)),
            _ => self.messages.get(self.message_selected),
        }
    }

    pub fn apply(&mut self, incoming: Incoming) -> Vec<Action> {
        match incoming {
            Incoming::Live(event) => return self.apply_live(event),
            Incoming::Tick => return self.poll(),
            Incoming::Inbox { items, names } => {
                self.names = names;
                if let Some(inbox) = &mut self.inbox {
                    inbox.set_items(items);
                }
                return vec![];
            }
            Incoming::Status(s) if s.is_empty() => return vec![],
            Incoming::Threads(candidates) => {
                if let Some(jump) = &mut self.jump {
                    jump.threads = candidates;
                }
                return vec![];
            }
            Incoming::DmOpened(channel) => return self.open_channel(channel),
            _ => {}
        }
        self.loading = false;
        match incoming {
            Incoming::Live(_) | Incoming::Tick | Incoming::Inbox { .. } | Incoming::Threads(_) | Incoming::DmOpened(_) => unreachable!(),
            Incoming::Channels { rows, people, names } => {
                self.channels = rows;
                self.people = people;
                self.names = names;
                self.status = format!("{} conversations · ? for help", self.channels.len());
            }
            Incoming::History { channel, messages, names } => {
                if self.current_channel.as_deref() != Some(&channel) {
                    return vec![];
                }
                self.names = names;
                let at_bottom = self.message_selected + 1 >= self.messages.len();
                let selected_ts = self.messages.get(self.message_selected).map(|m| m.ts.clone());
                let kept = selected_ts.filter(|_| !at_bottom).and_then(|ts| messages.iter().position(|m| m.ts == ts));
                self.message_selected = kept.unwrap_or(messages.len().saturating_sub(1));
                self.messages = messages;
                self.status = self.current_label();
            }
            Incoming::Replies { channel, ts, messages, names } => {
                self.names = names;
                let selected = messages.len().saturating_sub(1);
                self.thread = Some(Thread { channel, root_ts: ts, messages, selected });
            }
            Incoming::Sent { channel, thread_ts } => {
                self.status = "sent ✓".into();
                if self.current_channel.as_deref() != Some(&channel) {
                    if let Some(inbox) = &mut self.inbox {
                        inbox.flash = "sent ✓".into();
                    }
                    return vec![];
                }
                self.loading = true;
                return match thread_ts {
                    Some(ts) => vec![Action::LoadReplies { channel: channel.clone(), ts }, Action::LoadHistory(channel)],
                    None => vec![Action::LoadHistory(channel)],
                };
            }
            Incoming::SearchResults(matches) => {
                self.status = format!("{} results · enter to jump · esc to close", matches.len());
                self.message_selected = 0;
                self.search = Some(matches);
                self.focus = Focus::Messages;
            }
            Incoming::Status(s) => self.status = s,
            Incoming::Error(e) => self.status = format!("✗ {e}"),
        }
        vec![]
    }

    fn apply_live(&mut self, event: rtm::Event) -> Vec<Action> {
        match event {
            rtm::Event::Connected => self.live = Live::Live,
            rtm::Event::Disconnected(reason) => {
                if reason.contains("giving up") {
                    self.live = Live::Polling(reason);
                } else {
                    self.live = Live::Connecting;
                }
            }
            rtm::Event::Message { channel, message } => self.live_message(channel, message),
            rtm::Event::Changed { channel, message } => {
                if self.current_channel.as_deref() == Some(&channel) {
                    for m in self.all_messages_mut().into_iter().filter(|m| m.ts == message.ts) {
                        m.text = message.text.clone();
                        m.edited = message.edited.clone();
                    }
                }
            }
            rtm::Event::Deleted { channel, ts } => {
                if self.current_channel.as_deref() == Some(&channel) {
                    self.messages.retain(|m| m.ts != ts);
                    if let Some(t) = &mut self.thread {
                        t.messages.retain(|m| m.ts != ts);
                    }
                    self.clamp_selections();
                }
            }
            rtm::Event::Reaction { channel, ts, name, added } => {
                if self.current_channel.as_deref() == Some(&channel) {
                    for m in self.all_messages_mut().into_iter().filter(|m| m.ts == ts) {
                        adjust_reaction(&mut m.reactions, &name, added);
                    }
                }
            }
        }
        vec![]
    }

    fn live_message(&mut self, channel: String, message: Message) {
        if self.current_channel.as_deref() != Some(&channel) {
            self.unread.insert(channel);
            return;
        }
        if let Some(root) = message.thread_ts.clone().filter(|t| t != &message.ts) {
            if let Some(m) = self.messages.iter_mut().find(|m| m.ts == root) {
                m.reply_count += 1;
                m.latest_reply = Some(message.ts.clone());
            }
            if let Some(t) = self.thread.as_mut().filter(|t| t.root_ts == root && !t.messages.iter().any(|m| m.ts == message.ts)) {
                let follow = t.selected + 1 >= t.messages.len();
                t.messages.push(message);
                if follow {
                    t.selected = t.messages.len() - 1;
                }
            }
            return;
        }
        if self.messages.iter().any(|m| m.ts == message.ts) {
            return;
        }
        let follow = self.message_selected + 1 >= self.messages.len();
        self.messages.push(message);
        if follow && self.search.is_none() {
            self.message_selected = self.messages.len() - 1;
        }
    }

    fn all_messages_mut(&mut self) -> Vec<&mut Message> {
        let thread = self.thread.as_mut().map(|t| t.messages.iter_mut()).into_iter().flatten();
        self.messages.iter_mut().chain(thread).collect()
    }

    fn clamp_selections(&mut self) {
        self.message_selected = self.message_selected.min(self.messages.len().saturating_sub(1));
        if let Some(t) = &mut self.thread {
            t.selected = t.selected.min(t.messages.len().saturating_sub(1));
        }
    }

    /// Without a live feed, the open conversation is refreshed on every tick.
    fn poll(&mut self) -> Vec<Action> {
        if self.live == Live::Live || self.loading || self.input.is_some() {
            return vec![];
        }
        match &self.current_channel {
            Some(c) => vec![Action::LoadHistory(c.clone())],
            None => vec![],
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return vec![];
        }
        if self.help {
            self.help = false;
            return vec![];
        }
        if self.input.is_some() {
            return self.handle_input_key(key);
        }
        if self.jump.is_some() {
            return self.handle_jump_key(key);
        }
        if key.code == KeyCode::Char('k') && key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) {
            return self.open_jump();
        }
        if self.inbox.is_some() {
            return self.handle_inbox_key(key);
        }
        self.handle_browse_key(key)
    }

    pub fn open_jump(&mut self) -> Vec<Action> {
        let channels = self.channels.iter().map(|c| Candidate { label: c.label.clone(), target: Target::Channel(c.id.clone()) }).collect();
        let people =
            self.people.iter().map(|(id, handle)| Candidate { label: format!("@{handle}"), target: Target::Person(id.clone()) }).collect();
        self.jump = Some(Jump { channels, people, ..Default::default() });
        vec![Action::LoadThreads]
    }

    fn handle_jump_key(&mut self, key: KeyEvent) -> Vec<Action> {
        let jump = self.jump.as_mut().expect("jump open");
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.jump = None,
            KeyCode::Down | KeyCode::Tab => jump.move_by(1),
            KeyCode::Up | KeyCode::BackTab => jump.move_by(-1),
            KeyCode::Char('n') if ctrl => jump.move_by(1),
            KeyCode::Char('p') if ctrl => jump.move_by(-1),
            KeyCode::Backspace => jump.backspace(),
            KeyCode::Char(c) if !ctrl => jump.type_char(c),
            KeyCode::Enter => {
                if jump.is_search() {
                    let query = jump.query[1..].trim().to_owned();
                    self.jump = None;
                    if query.is_empty() {
                        return vec![];
                    }
                    self.loading = true;
                    self.status = "searching…".into();
                    return vec![Action::Search(query)];
                }
                let Some(candidate) = jump.selected_candidate() else { return vec![] };
                self.jump = None;
                self.inbox = None;
                return match candidate.target {
                    Target::Channel(id) => self.open_channel(id),
                    Target::Person(user) => {
                        self.status = "opening conversation…".into();
                        vec![Action::OpenDm(user)]
                    }
                    Target::Thread { channel, ts } => {
                        let mut actions = self.open_channel(channel.clone());
                        actions.push(Action::LoadReplies { channel, ts });
                        actions
                    }
                };
            }
            _ => {}
        }
        vec![]
    }

    pub fn open_inbox(&mut self) -> Vec<Action> {
        self.inbox = Some(Inbox::new(State::load(&self.workspace)));
        vec![Action::LoadInbox]
    }

    fn handle_inbox_key(&mut self, key: KeyEvent) -> Vec<Action> {
        let inbox = self.inbox.as_mut().expect("inbox open");
        inbox.flash.clear();
        if inbox.picking_snooze {
            return match key.code {
                KeyCode::Char(c @ '1'..='4') => {
                    let preset = Snooze::ALL[c as usize - '1' as usize];
                    inbox.snooze_selected(preset);
                    self.persist_inbox()
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    inbox.picking_snooze = false;
                    vec![]
                }
                _ => vec![],
            };
        }
        match key.code {
            KeyCode::Esc => self.inbox = None,
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => inbox.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => inbox.move_by(-1),
            KeyCode::Char('g') | KeyCode::Home => inbox.move_by(i64::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => inbox.move_by(i64::MAX / 2),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('d') => {
                if let Some(item) = inbox.read_selected() {
                    let mut actions = vec![Action::MarkRead(item)];
                    actions.extend(self.persist_inbox());
                    return actions;
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('s') => {
                if inbox.selected_item().is_some() {
                    inbox.picking_snooze = true;
                }
            }
            KeyCode::Char('a') => {
                let mut actions: Vec<Action> = inbox.read_all().into_iter().map(Action::MarkRead).collect();
                actions.extend(self.persist_inbox());
                return actions;
            }
            KeyCode::Char('r') => {
                if let Some(item) = inbox.selected_item().cloned() {
                    self.start_input(Input::InboxReply { item }, String::new());
                }
            }
            KeyCode::Char('o') => {
                if let Some(item) = inbox.selected_item() {
                    return vec![Action::Open { channel: item.channel.clone(), ts: item.ts.clone() }];
                }
            }
            KeyCode::Char('R') => {
                inbox.loading = true;
                return vec![Action::LoadInbox];
            }
            KeyCode::Enter => {
                if let Some(item) = inbox.selected_item().cloned() {
                    self.inbox = None;
                    let mut actions = self.open_channel(item.channel.clone());
                    if let Some(root) = item.thread_ts.clone().or_else(|| (item.kind != crate::inbox::Kind::Dm).then(|| item.ts.clone())) {
                        actions.push(Action::LoadReplies { channel: item.channel, ts: root });
                    }
                    return actions;
                }
            }
            KeyCode::Char('?') => self.help = true,
            _ => {}
        }
        vec![]
    }

    fn persist_inbox(&self) -> Vec<Action> {
        match &self.inbox {
            Some(inbox) => vec![Action::SaveInbox { workspace: self.workspace.clone(), state: inbox.state.clone() }],
            None => vec![],
        }
    }

    fn handle_browse_key(&mut self, key: KeyEvent) -> Vec<Action> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => self.focus = self.next_focus(),
            KeyCode::BackTab => self.focus = self.prev_focus(),
            KeyCode::Char('l') | KeyCode::Right => return self.go_right(),
            KeyCode::Char('h') | KeyCode::Left => self.go_left(),
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') | KeyCode::Home => self.move_selection(i64::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => self.move_selection(i64::MAX / 2),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Enter => return self.activate(),
            KeyCode::Esc => self.escape(),
            KeyCode::Char('/') => self.start_input(Input::Filter, self.filter.clone()),
            KeyCode::Char('s') => self.start_input(Input::Search, String::new()),
            KeyCode::Char('r') => return self.start_reply(self.focus == Focus::Thread),
            KeyCode::Char('t') => return self.start_reply(true),
            KeyCode::Char('e') => {
                if let Some((channel, ts)) = self.selected_ref() {
                    self.start_input(Input::React { channel, ts }, String::new());
                }
            }
            KeyCode::Char('o') => {
                if let Some((channel, ts)) = self.selected_ref() {
                    return vec![Action::Open { channel, ts }];
                }
            }
            KeyCode::Char('u') => match self.selected_message().and_then(first_link) {
                Some(url) => return vec![Action::OpenUrl(url)],
                None => self.status = "no link in this message".into(),
            },
            KeyCode::Char('y') => {
                if let Some((channel, ts)) = self.selected_ref() {
                    return vec![Action::Yank { channel, ts }];
                }
            }
            KeyCode::Char('R') => return self.refresh(),
            KeyCode::Char('i') => return self.open_inbox(),
            _ => {}
        }
        vec![]
    }

    fn handle_input_key(&mut self, key: KeyEvent) -> Vec<Action> {
        match key.code {
            KeyCode::Esc => {
                if self.input == Some(Input::Filter) {
                    self.filter.clear();
                }
                self.input = None;
                self.buffer.clear();
            }
            KeyCode::Enter => return self.submit_input(),
            KeyCode::Backspace => {
                self.buffer.pop();
                self.sync_filter();
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                self.sync_filter();
            }
            _ => {}
        }
        vec![]
    }

    fn sync_filter(&mut self) {
        if self.input == Some(Input::Filter) {
            self.filter = self.buffer.clone();
            self.channel_selected = 0;
        }
    }

    fn start_input(&mut self, input: Input, initial: String) {
        self.buffer = initial;
        self.input = Some(input);
    }

    fn submit_input(&mut self) -> Vec<Action> {
        let Some(input) = self.input.take() else { return vec![] };
        let text = std::mem::take(&mut self.buffer);
        match input {
            Input::Filter => {
                self.focus = Focus::Channels;
                vec![]
            }
            Input::Search if text.trim().is_empty() => vec![],
            Input::Search => {
                self.loading = true;
                self.status = "searching…".into();
                vec![Action::Search(text)]
            }
            Input::Reply { .. } if text.trim().is_empty() => vec![],
            Input::Reply { channel, thread_ts, .. } => {
                self.loading = true;
                self.status = "sending…".into();
                vec![Action::Send { channel, thread_ts, text }]
            }
            Input::React { channel, ts } => {
                let name = text.trim().trim_matches(':').to_owned();
                if name.is_empty() { vec![] } else { vec![Action::React { channel, ts, name }] }
            }
            Input::InboxReply { .. } if text.trim().is_empty() => vec![],
            Input::InboxReply { item } => {
                let mut actions = vec![Action::Send { channel: item.channel.clone(), thread_ts: item.reply_thread(), text }];
                if let Some(inbox) = &mut self.inbox
                    && let Some(read) = inbox.items.iter().position(|i| i.key == item.key).and_then(|i| {
                        inbox.selected = i;
                        inbox.read_selected()
                    })
                {
                    actions.push(Action::MarkRead(read));
                    actions.extend(self.persist_inbox());
                }
                actions
            }
        }
    }

    fn start_reply(&mut self, in_thread: bool) -> Vec<Action> {
        let Some(channel) = self.current_channel.clone() else {
            self.status = "pick a conversation first".into();
            return vec![];
        };
        let label = self.current_label();
        if !in_thread {
            self.start_input(Input::Reply { channel, thread_ts: None, label }, String::new());
            return vec![];
        }
        let root = match self.focus {
            Focus::Thread => self.thread.as_ref().map(|t| t.root_ts.clone()),
            _ => self.messages.get(self.message_selected).map(|m| m.thread_ts.clone().unwrap_or_else(|| m.ts.clone())),
        };
        let Some(root) = root else { return vec![] };
        self.start_input(Input::Reply { channel, thread_ts: Some(root), label: format!("{label} thread") }, String::new());
        vec![]
    }

    fn selected_ref(&self) -> Option<(String, String)> {
        if let Some(results) = &self.search
            && self.focus == Focus::Messages
        {
            return results.get(self.message_selected).map(|m| (m.channel.id.clone(), m.ts.clone()));
        }
        let channel = match self.focus {
            Focus::Thread => self.thread.as_ref()?.channel.clone(),
            _ => self.current_channel.clone()?,
        };
        Some((channel, self.selected_message()?.ts.clone()))
    }

    /// Right dives in: channel → its messages, message with replies → its thread.
    fn go_right(&mut self) -> Vec<Action> {
        match self.focus {
            Focus::Channels => self.activate(),
            Focus::Messages if self.search.is_none() => match self.messages.get(self.message_selected) {
                Some(m) if m.is_thread_root() || m.is_reply() => self.activate(),
                _ => vec![],
            },
            Focus::Messages => self.activate(),
            Focus::Thread => vec![],
        }
    }

    /// Left backs out: thread → messages (closing it), messages → channels.
    fn go_left(&mut self) {
        match self.focus {
            Focus::Thread => {
                self.thread = None;
                self.focus = Focus::Messages;
            }
            Focus::Messages => self.focus = Focus::Channels,
            Focus::Channels => {}
        }
    }

    fn next_focus(&self) -> Focus {
        match (self.focus, self.thread.is_some()) {
            (Focus::Channels, _) => Focus::Messages,
            (Focus::Messages, true) => Focus::Thread,
            _ => Focus::Channels,
        }
    }

    fn prev_focus(&self) -> Focus {
        match (self.focus, self.thread.is_some()) {
            (Focus::Channels, true) => Focus::Thread,
            (Focus::Channels, false) => Focus::Messages,
            (Focus::Messages, _) => Focus::Channels,
            (Focus::Thread, _) => Focus::Messages,
        }
    }

    fn move_selection(&mut self, delta: i64) {
        let visible = self.visible_channels().len();
        let (selected, len) = match self.focus {
            Focus::Channels => (&mut self.channel_selected, visible),
            Focus::Messages => (&mut self.message_selected, self.search.as_ref().map_or(self.messages.len(), Vec::len)),
            Focus::Thread => match self.thread.as_mut() {
                Some(t) => (&mut t.selected, t.messages.len()),
                None => return,
            },
        };
        if len == 0 {
            *selected = 0;
            return;
        }
        *selected = (*selected as i64).saturating_add(delta).clamp(0, len as i64 - 1) as usize;
    }

    fn activate(&mut self) -> Vec<Action> {
        match self.focus {
            Focus::Channels => {
                let Some(row) = self.visible_channels().get(self.channel_selected).cloned().cloned() else { return vec![] };
                self.open_channel(row.id)
            }
            Focus::Messages if self.search.is_some() => {
                let Some(m) = self.search.as_ref().and_then(|r| r.get(self.message_selected)).cloned() else { return vec![] };
                self.search = None;
                let mut actions = self.open_channel(m.channel.id.clone());
                actions.push(Action::LoadReplies { channel: m.channel.id, ts: m.ts });
                actions
            }
            Focus::Messages => {
                let Some(m) = self.messages.get(self.message_selected) else { return vec![] };
                let Some(channel) = self.current_channel.clone() else { return vec![] };
                let ts = m.thread_ts.clone().unwrap_or_else(|| m.ts.clone());
                self.focus = Focus::Thread;
                self.loading = true;
                vec![Action::LoadReplies { channel, ts }]
            }
            Focus::Thread => vec![],
        }
    }

    fn open_channel(&mut self, id: String) -> Vec<Action> {
        self.unread.remove(&id);
        self.current_channel = Some(id.clone());
        self.messages.clear();
        self.thread = None;
        self.focus = Focus::Messages;
        self.loading = true;
        self.status = format!("loading {}…", self.names.channel_label(&id));
        vec![Action::LoadHistory(id)]
    }

    fn refresh(&mut self) -> Vec<Action> {
        let mut actions = vec![Action::LoadChannels];
        if let Some(c) = &self.current_channel {
            actions.push(Action::LoadHistory(c.clone()));
        }
        if let Some(t) = &self.thread {
            actions.push(Action::LoadReplies { channel: t.channel.clone(), ts: t.root_ts.clone() });
        }
        self.loading = true;
        actions
    }

    fn escape(&mut self) {
        if self.search.is_some() {
            self.search = None;
            self.message_selected = self.messages.len().saturating_sub(1);
        } else if self.thread.is_some() {
            self.thread = None;
            self.focus = Focus::Messages;
        } else if !self.filter.is_empty() {
            self.filter.clear();
        }
    }
}

fn first_link(m: &Message) -> Option<String> {
    let in_text = crate::mrkdwn::parse(&m.text, &crate::mrkdwn::NoNames).into_iter().find_map(|s| match s {
        crate::mrkdwn::Segment::Link { url, .. } => Some(url),
        _ => None,
    });
    in_text.or_else(|| m.files.iter().map(|f| f.permalink.clone()).find(|p| !p.is_empty()))
}

fn adjust_reaction(reactions: &mut Vec<Reaction>, name: &str, added: bool) {
    match reactions.iter_mut().position(|r| r.name == name) {
        Some(i) if added => reactions[i].count += 1,
        Some(i) => {
            reactions[i].count = reactions[i].count.saturating_sub(1);
            if reactions[i].count == 0 {
                reactions.remove(i);
            }
        }
        None if added => reactions.push(Reaction { name: name.to_owned(), count: 1, users: vec![] }),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn row(id: &str, label: &str) -> ChannelRow {
        ChannelRow { id: id.into(), label: label.into(), kind: Kind::Public }
    }

    fn msg(ts: &str, text: &str) -> Message {
        Message { ts: ts.into(), text: text.into(), user: Some("U1".into()), ..Default::default() }
    }

    fn loaded() -> App {
        let mut app = App::new();
        app.apply(Incoming::Channels {
            rows: vec![row("C1", "#general"), row("C2", "#random")],
            people: vec![("U1".into(), "vivien".into())],
            names: NameBook::default(),
        });
        app
    }

    #[test]
    fn enter_on_channel_loads_history_and_focuses_messages() {
        let mut app = loaded();
        app.handle_key(key('j'));
        let actions = app.handle_key(code(KeyCode::Enter));
        assert_eq!(actions, vec![Action::LoadHistory("C2".into())]);
        assert_eq!(app.focus, Focus::Messages);
        assert!(app.loading);
        app.apply(Incoming::History { channel: "C2".into(), messages: vec![msg("1", "a"), msg("2", "b")], names: NameBook::default() });
        assert_eq!(app.message_selected, 1);
        assert!(!app.loading);
    }

    #[test]
    fn stale_history_is_ignored() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History { channel: "C9".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
        assert!(app.messages.is_empty());
    }

    #[test]
    fn filter_narrows_channels_live_and_escape_clears() {
        let mut app = loaded();
        app.handle_key(key('/'));
        for c in "ran".chars() {
            app.handle_key(key(c));
        }
        assert_eq!(app.visible_channels().len(), 1);
        app.handle_key(code(KeyCode::Enter));
        assert_eq!(app.input, None);
        assert_eq!(app.filter, "ran");
        app.handle_key(code(KeyCode::Esc));
        assert_eq!(app.filter, "");
    }

    #[test]
    fn reply_in_channel_sends_and_reloads() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
        app.handle_key(key('r'));
        for c in "hi".chars() {
            app.handle_key(key(c));
        }
        let actions = app.handle_key(code(KeyCode::Enter));
        assert_eq!(actions, vec![Action::Send { channel: "C1".into(), thread_ts: None, text: "hi".into() }]);
        assert_eq!(app.apply(Incoming::Sent { channel: "C1".into(), thread_ts: None }), vec![Action::LoadHistory("C1".into())]);
    }

    #[test]
    fn thread_reply_targets_the_root() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        let reply = Message { thread_ts: Some("1".into()), ..msg("2", "b") };
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a"), reply], names: NameBook::default() });
        assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![Action::LoadReplies { channel: "C1".into(), ts: "1".into() }]);
        assert_eq!(app.focus, Focus::Thread);
        app.apply(Incoming::Replies {
            channel: "C1".into(),
            ts: "1".into(),
            messages: vec![msg("1", "a"), msg("2", "b")],
            names: NameBook::default(),
        });
        app.handle_key(key('r'));
        app.handle_key(key('x'));
        let actions = app.handle_key(code(KeyCode::Enter));
        assert_eq!(actions, vec![Action::Send { channel: "C1".into(), thread_ts: Some("1".into()), text: "x".into() }]);
    }

    #[test]
    fn empty_reply_is_dropped_and_escape_cancels() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.handle_key(key('r'));
        assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![]);
        app.handle_key(key('r'));
        app.handle_key(key('z'));
        app.handle_key(code(KeyCode::Esc));
        assert_eq!(app.input, None);
        assert_eq!(app.buffer, "");
    }

    #[test]
    fn react_open_and_yank_use_selected_message() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
        assert_eq!(app.handle_key(key('o')), vec![Action::Open { channel: "C1".into(), ts: "1".into() }]);
        assert_eq!(app.handle_key(key('y')), vec![Action::Yank { channel: "C1".into(), ts: "1".into() }]);
        app.handle_key(key('e'));
        for c in ":tada:".chars() {
            app.handle_key(key(c));
        }
        assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![Action::React { channel: "C1".into(), ts: "1".into(), name: "tada".into() }]);
    }

    #[test]
    fn u_opens_the_first_link_of_the_selected_message() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        let linked = msg("1", "see <https://a.io|docs> and <https://b.io>");
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![linked, msg("2", "nothing")], names: NameBook::default() });
        assert_eq!(app.handle_key(key('u')), vec![]);
        assert_eq!(app.status, "no link in this message");
        app.handle_key(key('k'));
        assert_eq!(app.handle_key(key('u')), vec![Action::OpenUrl("https://a.io".into())]);
    }

    #[test]
    fn search_results_jump_to_channel_and_thread() {
        let mut app = loaded();
        app.handle_key(key('s'));
        app.handle_key(key('x'));
        assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![Action::Search("x".into())]);
        let hit = SearchMatch {
            ts: "5".into(),
            channel: crate::api::SearchChannel { id: "C2".into(), name: "random".into() },
            ..Default::default()
        };
        app.apply(Incoming::SearchResults(vec![hit]));
        assert_eq!(app.focus, Focus::Messages);
        let actions = app.handle_key(code(KeyCode::Enter));
        assert_eq!(actions, vec![Action::LoadHistory("C2".into()), Action::LoadReplies { channel: "C2".into(), ts: "5".into() }]);
        assert_eq!(app.search, None);
    }

    #[test]
    fn selection_is_clamped() {
        let mut app = loaded();
        app.handle_key(key('k'));
        assert_eq!(app.channel_selected, 0);
        app.handle_key(key('G'));
        assert_eq!(app.channel_selected, 1);
        app.handle_key(key('j'));
        assert_eq!(app.channel_selected, 1);
        app.handle_key(key('g'));
        assert_eq!(app.channel_selected, 0);
    }

    #[test]
    fn focus_cycles_through_open_panes() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Messages);
        app.handle_key(code(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Channels);
        app.thread = Some(Thread { channel: "C1".into(), root_ts: "1".into(), messages: vec![], selected: 0 });
        app.handle_key(code(KeyCode::Tab));
        app.handle_key(code(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Thread);
        app.handle_key(code(KeyCode::Esc));
        assert_eq!(app.thread, None);
        assert_eq!(app.focus, Focus::Messages);
    }

    #[test]
    fn right_opens_the_selected_thread_and_left_closes_it() {
        let mut app = loaded();
        assert_eq!(app.handle_key(code(KeyCode::Right)), vec![Action::LoadHistory("C1".into())]);
        let mut root = msg("1", "root");
        root.reply_count = 2;
        root.thread_ts = Some("1".into());
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![root, msg("2", "plain")], names: NameBook::default() });
        app.thread = Some(Thread { channel: "C1".into(), root_ts: "9".into(), messages: vec![], selected: 0 });
        assert_eq!(app.handle_key(code(KeyCode::Right)), vec![]);
        assert_eq!(app.focus, Focus::Messages);
        app.handle_key(key('k'));
        assert_eq!(app.handle_key(code(KeyCode::Right)), vec![Action::LoadReplies { channel: "C1".into(), ts: "1".into() }]);
        assert_eq!(app.focus, Focus::Thread);
        app.handle_key(code(KeyCode::Left));
        assert_eq!(app.thread, None);
        assert_eq!(app.focus, Focus::Messages);
        app.handle_key(code(KeyCode::Left));
        assert_eq!(app.focus, Focus::Channels);
    }

    #[test]
    fn quit_and_help() {
        let mut app = loaded();
        app.handle_key(key('?'));
        assert!(app.help);
        app.handle_key(key('j'));
        assert!(!app.help);
        assert_eq!(app.channel_selected, 0);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    fn live(app: &mut App, event: rtm::Event) -> Vec<Action> {
        app.apply(Incoming::Live(event))
    }

    #[test]
    fn live_message_appends_and_follows_bottom() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
        live(&mut app, rtm::Event::Connected);
        assert_eq!(app.live, Live::Live);
        live(&mut app, rtm::Event::Message { channel: "C1".into(), message: msg("2", "b") });
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.message_selected, 1);
        live(&mut app, rtm::Event::Message { channel: "C1".into(), message: msg("2", "b") });
        assert_eq!(app.messages.len(), 2);
        live(&mut app, rtm::Event::Message { channel: "C2".into(), message: msg("3", "elsewhere") });
        assert!(app.unread.contains("C2"));
        assert_eq!(app.messages.len(), 2);
    }

    #[test]
    fn live_reply_updates_root_and_open_thread() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "root")], names: NameBook::default() });
        app.thread = Some(Thread { channel: "C1".into(), root_ts: "1".into(), messages: vec![msg("1", "root")], selected: 0 });
        let reply = Message { thread_ts: Some("1".into()), ..msg("2", "reply") };
        live(&mut app, rtm::Event::Message { channel: "C1".into(), message: reply });
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].reply_count, 1);
        let t = app.thread.as_ref().unwrap();
        assert_eq!(t.messages.len(), 2);
        assert_eq!(t.selected, 1);
    }

    #[test]
    fn live_edit_delete_and_reactions() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a"), msg("2", "b")], names: NameBook::default() });
        live(&mut app, rtm::Event::Changed { channel: "C1".into(), message: msg("1", "edited") });
        assert_eq!(app.messages[0].text, "edited");
        live(&mut app, rtm::Event::Reaction { channel: "C1".into(), ts: "1".into(), name: "tada".into(), added: true });
        live(&mut app, rtm::Event::Reaction { channel: "C1".into(), ts: "1".into(), name: "tada".into(), added: true });
        assert_eq!(app.messages[0].reactions[0].count, 2);
        live(&mut app, rtm::Event::Reaction { channel: "C1".into(), ts: "1".into(), name: "tada".into(), added: false });
        live(&mut app, rtm::Event::Reaction { channel: "C1".into(), ts: "1".into(), name: "tada".into(), added: false });
        assert!(app.messages[0].reactions.is_empty());
        live(&mut app, rtm::Event::Deleted { channel: "C1".into(), ts: "2".into() });
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.message_selected, 0);
    }

    #[test]
    fn polling_only_when_feed_is_down() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![], names: NameBook::default() });
        assert_eq!(app.apply(Incoming::Tick), vec![Action::LoadHistory("C1".into())]);
        live(&mut app, rtm::Event::Connected);
        assert_eq!(app.apply(Incoming::Tick), vec![]);
        live(&mut app, rtm::Event::Disconnected("boom (giving up)".into()));
        assert!(matches!(app.live, Live::Polling(_)));
        assert_eq!(app.apply(Incoming::Tick), vec![Action::LoadHistory("C1".into())]);
    }

    #[test]
    fn refresh_keeps_selection_when_not_at_bottom() {
        let mut app = loaded();
        app.handle_key(code(KeyCode::Enter));
        app.apply(Incoming::History {
            channel: "C1".into(),
            messages: vec![msg("1", "a"), msg("2", "b"), msg("3", "c")],
            names: NameBook::default(),
        });
        app.handle_key(key('g'));
        app.apply(Incoming::History {
            channel: "C1".into(),
            messages: vec![msg("0", "z"), msg("1", "a"), msg("2", "b"), msg("3", "c")],
            names: NameBook::default(),
        });
        assert_eq!(app.message_selected, 1);
    }

    fn inbox_item(key: &str) -> Item {
        Item {
            key: key.into(),
            kind: crate::inbox::Kind::Mention,
            channel: "C2".into(),
            label: "#random".into(),
            thread_ts: Some("9".into()),
            ts: "10".into(),
            unread: vec![msg("10", "ping")],
        }
    }

    #[test]
    fn inbox_read_snooze_reply_and_open() {
        let mut app = loaded();
        assert_eq!(app.handle_key(key('i')), vec![Action::LoadInbox]);
        app.apply(Incoming::Inbox { items: vec![inbox_item("a"), inbox_item("b")], names: NameBook::default() });
        assert_eq!(app.inbox.as_ref().unwrap().items.len(), 2);
        let actions = app.handle_key(code(KeyCode::Right));
        assert!(matches!(&actions[0], Action::MarkRead(i) if i.key == "a"));
        assert!(matches!(&actions[1], Action::SaveInbox { .. }));
        app.handle_key(code(KeyCode::Left));
        assert!(app.inbox.as_ref().unwrap().picking_snooze);
        let actions = app.handle_key(key('3'));
        assert!(matches!(&actions[0], Action::SaveInbox { state, .. } if state.snoozed.contains_key("b")));
        assert!(app.inbox.as_ref().unwrap().items.is_empty());
        app.apply(Incoming::Inbox { items: vec![inbox_item("c")], names: NameBook::default() });
        app.handle_key(key('r'));
        for c in "ok".chars() {
            app.handle_key(key(c));
        }
        let actions = app.handle_key(code(KeyCode::Enter));
        assert_eq!(actions[0], Action::Send { channel: "C2".into(), thread_ts: Some("9".into()), text: "ok".into() });
        assert!(matches!(&actions[1], Action::MarkRead(i) if i.key == "c"));
        app.apply(Incoming::Inbox { items: vec![inbox_item("d")], names: NameBook::default() });
        let actions = app.handle_key(code(KeyCode::Enter));
        assert_eq!(actions, vec![Action::LoadHistory("C2".into()), Action::LoadReplies { channel: "C2".into(), ts: "9".into() }]);
        assert!(app.inbox.is_none());
        assert_eq!(app.current_channel.as_deref(), Some("C2"));
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn jump_opens_channels_people_threads_and_search() {
        let mut app = loaded();
        assert_eq!(app.handle_key(ctrl('k')), vec![Action::LoadThreads]);
        for c in "rnd".chars() {
            app.handle_key(key(c));
        }
        assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![Action::LoadHistory("C2".into())]);
        assert!(app.jump.is_none());

        app.handle_key(ctrl('k'));
        for c in "@viv".chars() {
            app.handle_key(key(c));
        }
        assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![Action::OpenDm("U1".into())]);
        assert_eq!(app.apply(Incoming::DmOpened("D1".into())), vec![Action::LoadHistory("D1".into())]);
        assert_eq!(app.current_channel.as_deref(), Some("D1"));

        app.handle_key(ctrl('k'));
        app.apply(Incoming::Threads(vec![Candidate {
            label: "#general · plan".into(),
            target: Target::Thread { channel: "C1".into(), ts: "9".into() },
        }]));
        for c in "plan".chars() {
            app.handle_key(key(c));
        }
        assert_eq!(
            app.handle_key(code(KeyCode::Enter)),
            vec![Action::LoadHistory("C1".into()), Action::LoadReplies { channel: "C1".into(), ts: "9".into() }]
        );

        app.handle_key(ctrl('k'));
        for c in ">deploy failed".chars() {
            app.handle_key(key(c));
        }
        assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![Action::Search("deploy failed".into())]);
        app.handle_key(ctrl('k'));
        app.handle_key(code(KeyCode::Esc));
        assert!(app.jump.is_none());
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::SUPER));
        assert!(app.jump.is_some());
    }

    #[test]
    fn inbox_escape_closes_and_q_quits() {
        let mut app = loaded();
        app.handle_key(key('i'));
        app.handle_key(code(KeyCode::Esc));
        assert!(app.inbox.is_none());
        app.handle_key(key('i'));
        app.handle_key(key('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn errors_land_in_status() {
        let mut app = loaded();
        app.apply(Incoming::Error("boom".into()));
        assert_eq!(app.status, "✗ boom");
    }
}
