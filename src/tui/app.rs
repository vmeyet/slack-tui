use crate::api::{ChannelKind, Message, SearchMatch};
use crate::resolve::NameBook;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Reply { channel: String, thread_ts: Option<String>, label: String },
    React { channel: String, ts: String },
    Filter,
    Search,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub channel: String,
    pub root_ts: String,
    pub messages: Vec<Message>,
    pub selected: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    LoadChannels,
    LoadHistory(String),
    LoadReplies { channel: String, ts: String },
    Send { channel: String, thread_ts: Option<String>, text: String },
    React { channel: String, ts: String, name: String },
    Search(String),
    Open { channel: String, ts: String },
    Yank { channel: String, ts: String },
}

#[derive(Clone, Debug)]
pub enum Incoming {
    Channels(Vec<ChannelRow>, NameBook),
    History { channel: String, messages: Vec<Message>, names: NameBook },
    Replies { channel: String, ts: String, messages: Vec<Message>, names: NameBook },
    Sent { channel: String, thread_ts: Option<String> },
    SearchResults(Vec<SearchMatch>),
    Status(String),
    Error(String),
}

#[derive(Debug, Default)]
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
        self.loading = false;
        match incoming {
            Incoming::Channels(rows, names) => {
                self.channels = rows;
                self.names = names;
                self.status = format!("{} conversations · ? for help", self.channels.len());
            }
            Incoming::History { channel, messages, names } => {
                if self.current_channel.as_deref() != Some(&channel) {
                    return vec![];
                }
                self.names = names;
                self.message_selected = messages.len().saturating_sub(1);
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
        self.handle_browse_key(key)
    }

    fn handle_browse_key(&mut self, key: KeyEvent) -> Vec<Action> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => self.focus = self.next_focus(),
            KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => self.focus = self.prev_focus(),
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
            KeyCode::Char('y') => {
                if let Some((channel, ts)) = self.selected_ref() {
                    return vec![Action::Yank { channel, ts }];
                }
            }
            KeyCode::Char('R') => return self.refresh(),
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
        app.apply(Incoming::Channels(vec![row("C1", "#general"), row("C2", "#random")], NameBook::default()));
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

    #[test]
    fn errors_land_in_status() {
        let mut app = loaded();
        app.apply(Incoming::Error("boom".into()));
        assert_eq!(app.status, "✗ boom");
    }
}
