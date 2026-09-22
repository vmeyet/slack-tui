use super::{Action, App, Live};
use crate::api::rtm;
use crate::api::{Message, Reaction};
use crate::firehose::Line as LiveLine;
use crate::tui::firehose;

impl App {
    pub(super) fn apply_live(&mut self, event: rtm::Event) -> Vec<Action> {
        match event {
            rtm::Event::Connected => self.live = Live::Live,
            rtm::Event::Disconnected(_) => self.live = Live::Connecting,
            rtm::Event::GaveUp(reason) => self.live = Live::Polling(reason),
            rtm::Event::Message { channel, message } => {
                let line = LiveLine::from_message(&channel, &message);
                let classify = (self.triage && self.firehose.is_some()).then(|| Action::Classify(line.clone()));
                firehose::push(&mut self.wall, line);
                let unknown = message.user.clone().filter(|u| self.names.user_label(u) == *u);
                self.live_message(channel, message);
                let mut actions = self.refresh_thumbs();
                actions.extend(classify);
                if let Some(id) = unknown {
                    actions.push(Action::LearnUsers(vec![id]));
                }
                return actions;
            }
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
            rtm::Event::Typing { channel, user } => {
                if self.current_channel.as_deref() == Some(&channel) && user != self.me {
                    self.mark_typing(&user);
                }
            }
            rtm::Event::Reaction { channel, ts, name, user, added } => {
                if self.current_channel.as_deref() == Some(&channel) {
                    for m in self.all_messages_mut().into_iter().filter(|m| m.ts == ts) {
                        adjust_reaction(&mut m.reactions, &name, &user, added);
                    }
                }
            }
        }
        vec![]
    }

    fn live_message(&mut self, channel: String, message: Message) {
        if self.current_channel.as_deref() != Some(&channel) {
            let badge = self.badges.entry(channel.clone()).or_default();
            badge.unread = true;
            if !self.me.is_empty() && crate::inbox::mentions_me(&message.text, &self.me, None) {
                badge.mentions += 1;
            }
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
}

fn adjust_reaction(reactions: &mut Vec<Reaction>, name: &str, user: &str, added: bool) {
    let Some(i) = reactions.iter().position(|r| r.name == name) else {
        if added {
            reactions.push(Reaction { name: name.to_owned(), count: 1, users: vec![user.to_owned()] });
        }
        return;
    };
    let reaction = &mut reactions[i];
    let mine = reaction.users.iter().any(|u| u == user);
    match (added, mine) {
        (true, false) => {
            reaction.users.push(user.to_owned());
            reaction.count += 1;
        }
        (false, true) => {
            reaction.users.retain(|u| u != user);
            reaction.count = reaction.count.saturating_sub(1);
        }
        _ => {}
    }
    if reaction.count == 0 {
        reactions.remove(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_reaction_events_are_idempotent() {
        let mut reactions = vec![];
        adjust_reaction(&mut reactions, "tada", "U1", true);
        adjust_reaction(&mut reactions, "tada", "U1", true);
        assert_eq!(reactions, vec![Reaction { name: "tada".into(), count: 1, users: vec!["U1".into()] }]);
        adjust_reaction(&mut reactions, "tada", "U9", false);
        assert_eq!(reactions[0].count, 1);
        adjust_reaction(&mut reactions, "tada", "U1", false);
        adjust_reaction(&mut reactions, "tada", "U1", false);
        assert!(reactions.is_empty());
    }
}
