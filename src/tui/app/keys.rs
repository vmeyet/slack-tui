use super::commands::emoji_names;
use super::{Action, App, Focus, Input};
use crate::api::Message;
use crate::inbox::Snooze;
use crate::tui::complete::{self, Cycle};
use crate::tui::field::Field;
use crate::tui::firehose::Firehose;
use crate::tui::jump::{Candidate, Jump, Target};
use crate::tui::palette::Palette;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Action> {
        self.dismiss_error();
        let actions = self.route_key(key);
        self.mark_seen();
        actions
    }

    fn route_key(&mut self, key: KeyEvent) -> Vec<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return vec![];
        }
        if self.help {
            self.help = false;
            return vec![];
        }
        if self.pending_delete.is_some() {
            return self.handle_confirm_key(key);
        }
        if self.input.is_some() {
            return self.handle_input_key(key);
        }
        if self.palette.is_some() {
            return self.handle_palette_key(key);
        }
        if key.code == KeyCode::Char(':') && self.jump.is_none() {
            self.palette = Some(Palette::with_history(self.palette_history.clone()));
            return vec![];
        }
        if self.jump.is_some() {
            return self.handle_jump_key(key);
        }
        if key.code == KeyCode::Char('k') && key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) {
            return self.open_jump();
        }
        if self.firehose.is_some() {
            return self.handle_firehose_key(key);
        }
        if self.inbox.is_some() {
            return self.handle_inbox_key(key);
        }
        let actions = self.handle_browse_key(key);
        if self.zen && self.focus == Focus::Channels {
            self.focus = Focus::Messages;
        }
        actions
    }

    fn toggle_reading(&mut self) {
        self.zen = !self.zen;
        if self.zen && self.current_channel.is_none() {
            self.zen = false;
            self.toast("open a conversation first");
        }
    }

    #[allow(clippy::expect_used)]
    fn handle_firehose_key(&mut self, key: KeyEvent) -> Vec<Action> {
        let view = self.firehose.as_mut().expect("firehose open");
        let lines = view.visible(&self.wall);
        let len = lines.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('f') => self.firehose = None,
            KeyCode::Char('n') => {
                view.show_noise = !view.show_noise;
                view.follow();
            }
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => view.move_by(1, len),
            KeyCode::Char('k') | KeyCode::Up => view.move_by(-1, len),
            KeyCode::PageDown => view.move_by(10, len),
            KeyCode::PageUp => view.move_by(-10, len),
            KeyCode::Char('g') | KeyCode::Home => view.move_by(i64::MIN / 2, len),
            KeyCode::Char('G') | KeyCode::End => view.follow(),
            KeyCode::Enter => {
                let Some(line) = view.selected.or_else(|| len.checked_sub(1)).and_then(|i| lines.get(i)).map(|l| (*l).clone()) else {
                    return vec![];
                };
                self.firehose = None;
                let mut actions = self.open_channel(line.channel.clone());
                if let Some(root) = line.thread_ts {
                    actions.push(Action::LoadReplies { channel: line.channel, ts: root });
                }
                return actions;
            }
            KeyCode::Char('?') => self.help = true,
            _ => {}
        }
        vec![]
    }

    fn load_promises(&mut self) -> Vec<Action> {
        if !self.triage {
            self.toast("promises need Jev: set `[typesafe] enabled = true`");
            return vec![];
        }
        self.loading = true;
        self.toast("looking for your open promises…");
        vec![Action::LoadPromises]
    }

    fn open_jump(&mut self) -> Vec<Action> {
        let channels = self.channels.iter().map(|c| Candidate { label: c.label.clone(), target: Target::Channel(c.id.clone()) }).collect();
        let people =
            self.people.iter().map(|(id, handle)| Candidate { label: format!("@{handle}"), target: Target::Person(id.clone()) }).collect();
        self.jump = Some(Jump { channels, people, ..Default::default() });
        vec![Action::LoadThreads]
    }

    #[allow(clippy::expect_used)]
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
                    return if query.is_empty() { vec![] } else { self.search_for(query) };
                }
                let Some(candidate) = jump.selected_candidate() else { return vec![] };
                self.jump = None;
                self.inbox = None;
                return match candidate.target {
                    Target::Channel(id) => self.open_channel(id),
                    Target::Person(user) => {
                        self.toast("opening conversation…");
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

    #[allow(clippy::expect_used)]
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
            KeyCode::Right | KeyCode::Char('l' | 'd') => {
                if let Some(item) = inbox.read_selected() {
                    let mut actions = vec![Action::MarkRead(item)];
                    actions.extend(self.persist_inbox());
                    return actions;
                }
            }
            KeyCode::Left | KeyCode::Char('h' | 's') => {
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
            KeyCode::Char('e') => return self.compose(),
            KeyCode::Char('+') => {
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
                None => self.toast("no link in this message"),
            },
            KeyCode::Char('y') => {
                if let Some((channel, ts)) = self.selected_ref() {
                    return vec![Action::Yank { channel, ts }];
                }
            }
            KeyCode::Char('R') => return self.refresh(),
            KeyCode::Char('i') => return self.open_inbox(),
            KeyCode::Char('p') => return self.load_promises(),
            KeyCode::Char('f') => self.firehose = Some(Firehose::default()),
            KeyCode::Char('z') => self.toggle_reading(),
            _ => {}
        }
        vec![]
    }

    fn handle_input_key(&mut self, key: KeyEvent) -> Vec<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                if self.input == Some(Input::Filter) {
                    self.filter.clear();
                }
                self.close_input();
            }
            KeyCode::Enter => return self.submit_input(),
            KeyCode::Tab | KeyCode::BackTab => self.cycle_emoji(key.code == KeyCode::BackTab),
            KeyCode::Left => self.buffer.left(),
            KeyCode::Right => {
                if !self.accept_emoji() {
                    self.buffer.right();
                }
            }
            KeyCode::Home => self.buffer.start(),
            KeyCode::End => {
                if !self.accept_emoji() {
                    self.buffer.end();
                }
            }
            KeyCode::Char('a') if ctrl => self.buffer.start(),
            KeyCode::Char('e') if ctrl => self.buffer.end(),
            KeyCode::Char('w') if ctrl => {
                self.buffer.delete_word();
                self.edited();
            }
            KeyCode::Backspace => {
                self.buffer.backspace();
                self.edited();
            }
            KeyCode::Delete => {
                self.buffer.delete();
                self.edited();
            }
            KeyCode::Char(c) if !ctrl => {
                self.buffer.insert(c);
                self.edited();
            }
            _ => {}
        }
        vec![]
    }

    fn close_input(&mut self) {
        self.input = None;
        self.buffer.clear();
        self.react_cycle = None;
    }

    /// After a change to the text: the filter follows it, and any completion in hand is stale.
    fn edited(&mut self) {
        self.react_cycle = None;
        if self.input == Some(Input::Filter) {
            self.filter = self.buffer.text().to_owned();
            self.channel_selected = 0;
        }
    }

    /// Replaces the half-typed emoji name with the next one that matches it. The react row is the
    /// only one with something to complete.
    fn cycle_emoji(&mut self, backwards: bool) {
        let candidates = self.emoji_candidates();
        match &mut self.react_cycle {
            Some(cycle) => cycle.advance(backwards),
            None => self.react_cycle = Cycle::new(self.emoji_token(), candidates),
        }
        let Some(name) = self.react_cycle.as_ref().map(|c| c.current().to_owned()) else { return };
        self.buffer = Field::new(name);
    }

    /// Takes the suggested end of the emoji name, as `→` does in a shell, and says whether it did.
    fn accept_emoji(&mut self) -> bool {
        let Some(rest) = self.input_ghost() else { return false };
        for c in rest.chars() {
            self.buffer.insert(c);
        }
        true
    }

    /// The end of the emoji name being typed, shown in grey after the cursor. Only the react row
    /// suggests anything, only with the cursor at the end, and never while tab is cycling.
    pub(in crate::tui) fn input_ghost(&self) -> Option<String> {
        if self.react_cycle.is_some() || !self.buffer.at_end() {
            return None;
        }
        complete::ghost(self.emoji_token(), self.emoji_candidates()).filter(|rest| !rest.is_empty())
    }

    /// The options tab is cycling through, each behind its glyph.
    pub(in crate::tui) fn input_hint(&self) -> Option<String> {
        Some(self.react_cycle.as_ref()?.hint(|name| format!("{} {name}", crate::emoji::render(name))))
    }

    fn emoji_candidates(&self) -> &'static [String] {
        match self.input {
            Some(Input::React { .. }) => emoji_names(),
            _ => &[],
        }
    }

    /// What the row is completing, which the candidates are names of.
    fn emoji_token(&self) -> &str {
        shortcode(self.buffer.text())
    }

    /// A delete cannot be undone, so only `y` goes through and every other key keeps the message.
    fn handle_confirm_key(&mut self, key: KeyEvent) -> Vec<Action> {
        let Some(pending) = self.pending_delete.take() else { return vec![] };
        if key.code != KeyCode::Char('y') {
            self.toast("kept");
            return vec![];
        }
        self.toast("deleting…");
        vec![Action::Delete { channel: pending.channel, ts: pending.ts }]
    }

    pub(super) fn start_input(&mut self, input: Input, initial: String) {
        self.buffer = Field::new(initial);
        self.input = Some(input);
        self.react_cycle = None;
    }

    fn submit_input(&mut self) -> Vec<Action> {
        let Some(input) = self.input.take() else { return vec![] };
        let text = self.buffer.take();
        self.react_cycle = None;
        match input {
            Input::Filter => {
                self.focus = Focus::Channels;
                vec![]
            }
            Input::Search | Input::Reply { .. } | Input::InboxReply { .. } if text.trim().is_empty() => vec![],
            Input::Search => self.search_for(text),
            Input::Reply { channel, thread_ts, .. } => self.send(channel, thread_ts, text),
            Input::React { channel, ts } => {
                let name = shortcode(text.trim().trim_matches(':')).to_owned();
                if name.is_empty() { vec![] } else { vec![Action::React { channel, ts, name }] }
            }
            Input::Edit { .. } if text.trim().is_empty() => {
                self.fail("an empty edit would not delete it — use :delete");
                vec![]
            }
            Input::Edit { channel, ts } => {
                self.toast("saving…");
                vec![Action::Edit { channel, ts, text }]
            }
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
        if self.current_channel.is_none() {
            self.toast("pick a conversation first");
            return vec![];
        }
        let Some((channel, thread_ts)) = self.reply_target(in_thread) else { return vec![] };
        let label = self.current_label();
        let label = if thread_ts.is_some() { format!("{label} thread") } else { label };
        self.start_input(Input::Reply { channel, thread_ts, label }, String::new());
        vec![]
    }

    /// Hands the input row's text to the editor and, once it comes back, sends it where a reply would go.
    pub(super) fn compose(&mut self) -> Vec<Action> {
        let Some((channel, thread_ts)) = self.reply_target(self.focus == Focus::Thread) else {
            self.toast("pick a conversation first");
            return vec![];
        };
        vec![Action::Compose { channel, thread_ts, draft: self.buffer.text().to_owned() }]
    }

    /// The conversation a reply goes to, and the thread it belongs to when it belongs to one.
    fn reply_target(&self, in_thread: bool) -> Option<(String, Option<String>)> {
        let channel = self.current_channel.clone()?;
        if !in_thread {
            return Some((channel, None));
        }
        let root = match self.focus {
            Focus::Thread => self.thread.as_ref().map(|t| t.root_ts.clone()),
            _ => self.messages.get(self.message_selected).map(|m| m.thread_ts.clone().unwrap_or_else(|| m.ts.clone())),
        };
        Some((channel, Some(root?)))
    }

    /// Right dives in: channel → its messages, message with replies → its thread.
    fn go_right(&mut self) -> Vec<Action> {
        match self.focus {
            Focus::Messages if self.search.is_none() => match self.messages.get(self.message_selected) {
                Some(m) if m.is_thread_root() || m.is_reply() => self.activate(),
                _ => vec![],
            },
            Focus::Channels | Focus::Messages => self.activate(),
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
            (Focus::Channels, false) | (Focus::Thread, _) => Focus::Messages,
            (Focus::Messages, _) => Focus::Channels,
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

    pub(super) fn activate(&mut self) -> Vec<Action> {
        match self.focus {
            Focus::Channels => {
                let Some(row) = self.visible_channels().get(self.channel_selected).copied().cloned() else { return vec![] };
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
}

/// Reactions travel by name, so a pasted 🚀 becomes `rocket`; anything else stands as typed.
pub(super) fn shortcode(text: &str) -> &str {
    crate::emoji::name_for(text).unwrap_or(text)
}

fn first_link(m: &Message) -> Option<String> {
    let in_text = crate::mrkdwn::parse(&m.text, &crate::mrkdwn::NoNames).into_iter().find_map(|s| match s {
        crate::mrkdwn::Segment::Link { url, .. } => Some(url),
        _ => None,
    });
    in_text.or_else(|| m.files.iter().map(|f| f.permalink.clone()).find(|p| !p.is_empty()))
}
