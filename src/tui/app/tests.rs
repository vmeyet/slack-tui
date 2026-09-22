use super::*;
use crate::api::{File, Message};
use crate::inbox::Item;
use crate::tui::field::Field;
use crate::tui::images::Thumbs;
use crate::tui::jump::Target;
use crate::tui::motion::{FRAME, SPINNER_FRAME};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn code(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

fn row(id: &str, label: &str) -> ChannelRow {
    ChannelRow::new(id, label, Kind::Public)
}

fn msg(ts: &str, text: &str) -> Message {
    Message { ts: ts.into(), text: text.into(), user: Some("U1".into()), ..Default::default() }
}

fn from_other(ts: &str, text: &str) -> Message {
    Message { user: Some("U2".into()), ..msg(ts, text) }
}

fn history(messages: Vec<Message>) -> Incoming {
    Incoming::History { channel: "C1".into(), messages, names: NameBook::default() }
}

fn loaded() -> App {
    let mut app = App::new();
    app.apply(Incoming::Channels {
        rows: vec![row("C1", "#general"), row("C2", "#random")],
        people: vec![("U1".into(), "vivien".into())],
        names: NameBook::default(),
        badges: HashMap::new(),
        me: "U1".into(),
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
fn filter_follows_an_edit_made_mid_text() {
    let mut app = loaded();
    app.handle_key(key('/'));
    for c in "ran".chars() {
        app.handle_key(key(c));
    }
    app.handle_key(code(KeyCode::Left));
    app.handle_key(key('e'));
    assert_eq!(app.filter, "raen");
    assert!(app.visible_channels().is_empty());
    app.handle_key(code(KeyCode::Backspace));
    assert_eq!(app.filter, "ran");
    assert_eq!(app.visible_channels().len(), 1);
}

#[test]
fn the_input_row_edits_where_the_cursor_sits() {
    let mut app = reading();
    app.handle_key(key('r'));
    for c in "helo".chars() {
        app.handle_key(key(c));
    }
    app.handle_key(code(KeyCode::Left));
    app.handle_key(key('l'));
    assert_eq!(app.buffer.text(), "hello");
    app.handle_key(code(KeyCode::Home));
    app.handle_key(code(KeyCode::Delete));
    app.handle_key(ctrl('e'));
    app.handle_key(key('!'));
    let actions = app.handle_key(code(KeyCode::Enter));
    assert_eq!(actions, vec![Action::Send { channel: "C1".into(), thread_ts: None, text: "ello!".into() }]);
}

#[test]
fn ctrl_w_eats_the_word_before_the_cursor_and_ctrl_a_goes_back_to_the_start() {
    let mut app = reading();
    app.handle_key(key('r'));
    for c in "ship it now".chars() {
        app.handle_key(key(c));
    }
    app.handle_key(ctrl('w'));
    assert_eq!(app.buffer.text(), "ship it ");
    app.handle_key(ctrl('a'));
    app.handle_key(key('>'));
    assert_eq!(app.buffer.text(), ">ship it ");
    app.handle_key(ctrl('z'));
    assert_eq!(app.buffer.text(), ">ship it ", "an unbound control key types nothing");
}

#[test]
fn an_edit_prefill_starts_with_the_cursor_after_the_last_character() {
    let mut app = reading();
    app.apply(history(vec![msg("1", "café")]));
    palette_run(&mut app, "edit");
    app.handle_key(key('!'));
    assert_eq!(app.buffer.text(), "café!");
    app.handle_key(code(KeyCode::Home));
    app.handle_key(key('¡'));
    assert_eq!(app.buffer.text(), "¡café!");
}

/// The react row on a half-typed name, one key away from `🚀 rocket`.
fn reacting(typed: &str) -> App {
    let mut app = reading();
    app.handle_key(key('e'));
    for c in typed.chars() {
        app.handle_key(key(c));
    }
    app
}

#[test]
fn tab_cycles_emoji_names_and_reacts_with_the_bare_name() {
    let mut app = reacting("rocke");
    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.buffer.text(), "rocket");
    let hint = app.input_hint().expect("cycling");
    assert!(hint.starts_with("[🚀 rocket]"), "the glyph shows next to the name: {hint}");
    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.buffer.text(), "arrows_clockwise");
    app.handle_key(code(KeyCode::BackTab));
    assert_eq!(app.buffer.text(), "rocket");
    let actions = app.handle_key(code(KeyCode::Enter));
    assert_eq!(actions, vec![Action::React { channel: "C1".into(), ts: "1".into(), name: "rocket".into() }]);
}

fn reacted_name(typed: &str) -> String {
    let actions = reacting(typed).handle_key(code(KeyCode::Enter));
    match actions.as_slice() {
        [Action::React { name, .. }] => name.clone(),
        other => panic!("{typed} reacted with {other:?}"),
    }
}

#[test]
fn a_pasted_glyph_reacts_with_the_name_slack_wants() {
    assert_eq!(reacted_name("🚀"), "rocket");
    assert_eq!(reacted_name("👍🏽"), "+1", "a skin tone reacts with the base name");
    assert_eq!(reacted_name("rocket"), "rocket");
    assert_eq!(reacted_name(":rocket:"), "rocket");
    assert_eq!(reacted_name("partyparrot"), "partyparrot", "a custom emoji is a name of its own");
}

#[test]
fn the_react_command_takes_a_glyph_too() {
    let mut app = reacting("");
    app.handle_key(code(KeyCode::Esc));
    match palette_run(&mut app, "react 🚀").as_slice() {
        [Action::React { name, .. }] => assert_eq!(name, "rocket"),
        other => panic!(":react 🚀 gave {other:?}"),
    }
}

#[test]
fn tab_on_a_pasted_glyph_completes_to_its_name() {
    let mut app = reacting("🚀");
    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.buffer.text(), "rocket");
    assert!(app.input_hint().expect("cycling").starts_with("[🚀 rocket]"));
}

#[test]
fn an_edit_drops_the_options_being_cycled_and_so_does_leaving_the_row() {
    let mut app = reacting("rocke");
    app.handle_key(code(KeyCode::Tab));
    assert!(app.input_hint().is_some());
    app.handle_key(code(KeyCode::Backspace));
    assert_eq!(app.input_hint(), None);
    assert_eq!(app.buffer.text(), "rocke");
    app.handle_key(code(KeyCode::Tab));
    app.handle_key(code(KeyCode::Esc));
    assert_eq!(app.input_hint(), None, "nothing to cycle once the row is closed");
}

#[test]
fn right_takes_the_suggested_end_of_the_name_and_leaves_the_cursor_after_it() {
    let mut app = reacting("rocke");
    assert_eq!(app.input_ghost().as_deref(), Some("t"));
    app.handle_key(code(KeyCode::Right));
    assert_eq!(app.buffer.text(), "rocket");
    assert_eq!(app.input_ghost(), None, "a complete name suggests nothing");
    app.handle_key(key('!'));
    assert_eq!(app.buffer.text(), "rocket!", "the cursor stayed at the end");
}

#[test]
fn the_react_row_suggests_nothing_with_the_cursor_mid_text() {
    let mut app = reacting("rocke");
    app.handle_key(code(KeyCode::Left));
    assert_eq!(app.input_ghost(), None);
    app.handle_key(code(KeyCode::Right));
    assert_eq!(app.input_ghost().as_deref(), Some("t"), "back at the end, the suggestion is back");
}

#[test]
fn a_reply_row_completes_nothing_and_keeps_its_arrows() {
    let mut app = reading();
    app.handle_key(key('r'));
    for c in "rocke".chars() {
        app.handle_key(key(c));
    }
    assert_eq!(app.input_ghost(), None);
    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.buffer.text(), "rocke", "tab completes nothing in a reply");
    app.handle_key(code(KeyCode::Left));
    app.handle_key(code(KeyCode::Right));
    app.handle_key(key('t'));
    assert_eq!(app.buffer.text(), "rocket", "→ moved the cursor back to the end");
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
fn compose_seeds_the_editor_with_the_input_row_and_clears_it_once_sent() {
    let mut app = loaded();
    app.handle_key(code(KeyCode::Enter));
    app.buffer = Field::new("half written");
    let actions = app.handle_key(key('E'));
    assert_eq!(actions, vec![Action::Compose { channel: "C1".into(), thread_ts: None, draft: "half written".into() }]);
    let sent = app.apply(Incoming::Composed { channel: "C1".into(), thread_ts: None, text: "two\nlines".into() });
    assert_eq!(sent, vec![Action::Send { channel: "C1".into(), thread_ts: None, text: "two\nlines".into() }]);
    assert_eq!(app.input, None);
    assert_eq!(app.buffer.text(), "");
}

#[test]
fn compose_writes_in_the_open_thread_and_needs_a_conversation() {
    let mut app = loaded();
    assert_eq!(app.handle_key(key('E')), vec![]);
    assert_eq!(app.status_line(), "pick a conversation first");
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::Replies { channel: "C1".into(), ts: "1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
    let actions = app.handle_key(key('E'));
    assert_eq!(actions, vec![Action::Compose { channel: "C1".into(), thread_ts: Some("1".into()), draft: String::new() }]);
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
    assert_eq!(app.buffer.text(), "");
}

#[test]
fn history_with_pictures_asks_for_each_thumbnail_once() {
    let mut app = loaded();
    app.thumbs = Thumbs::with(ratatui_image::picker::Picker::halfblocks());
    app.handle_key(code(KeyCode::Enter));
    let picture = File {
        id: "F1".into(),
        name: "shot.png".into(),
        mimetype: "image/png".into(),
        thumb_360: "https://files.slack.com/shot.png".into(),
        thumb_360_w: 360,
        thumb_360_h: 200,
        ..Default::default()
    };
    let with_picture = Message { files: vec![picture], ..msg("1", "look") };
    let history = |m: Message| Incoming::History { channel: "C1".into(), messages: vec![m], names: NameBook::default() };
    let actions = app.apply(history(with_picture.clone()));
    assert_eq!(actions, vec![Action::LoadImage { id: "F1".into(), url: "https://files.slack.com/shot.png".into() }]);
    assert_eq!(app.apply(history(with_picture)), vec![]);
    app.apply(Incoming::Thumb { id: "F1".into(), image: Some(image::DynamicImage::new_rgb8(4, 4)) });
    assert!(matches!(app.thumbs.get("F1"), Some(super::super::images::Thumb::Ready(_))));
    app.apply(history(msg("2", "gone")));
    assert!(app.thumbs.get("F1").is_none(), "forgotten once off screen");
}

#[test]
fn animates_only_while_an_empty_state_is_visible() {
    let mut app = loaded();
    assert!(app.animating(), "no conversation picked yet");
    app.handle_key(code(KeyCode::Enter));
    assert!(!app.animating(), "history is loading");
    app.apply(Incoming::History { channel: "C1".into(), messages: vec![], names: NameBook::default() });
    assert!(app.animating(), "empty channel");
    app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "hi")], names: NameBook::default() });
    assert!(!app.animating());
    app.apply(Incoming::SearchResults(vec![]));
    assert!(app.animating(), "search without results");
    app.help = true;
    assert!(!app.animating(), "a modal covers it");
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
    assert_eq!(app.status_line(), "no link in this message");
    app.handle_key(key('k'));
    assert_eq!(app.handle_key(key('u')), vec![Action::OpenUrl("https://a.io".into())]);
}

#[test]
fn search_results_jump_to_channel_and_thread() {
    let mut app = loaded();
    app.handle_key(key('s'));
    app.handle_key(key('x'));
    assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![Action::Search("x".into())]);
    let hit =
        SearchMatch { ts: "5".into(), channel: crate::api::SearchChannel { id: "C2".into(), name: "random".into() }, ..Default::default() };
    app.apply(Incoming::SearchResults(vec![hit]));
    assert_eq!(app.focus, Focus::Messages);
    let actions = app.handle_key(code(KeyCode::Enter));
    assert_eq!(actions, vec![Action::LoadHistory("C2".into()), Action::LoadReplies { channel: "C2".into(), ts: "5".into() }]);
    assert_eq!(app.search, None);
}

#[test]
fn status_follows_where_you_are() {
    let mut app = App::new();
    assert_eq!(app.status_line(), "loading channels…");
    app = loaded();
    assert_eq!(app.status_line(), "2 conversations · ? for help");

    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.status_line(), "C1");

    app.handle_key(key('s'));
    app.handle_key(key('x'));
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::SearchResults(vec![SearchMatch::default()]));
    app.now += Duration::from_secs(2);
    assert_eq!(app.status_line(), "1 results · enter to jump · esc to close");

    app.handle_key(code(KeyCode::Esc));
    assert_eq!(app.status_line(), "C1", "escaping search leaves the hint behind");
}

#[test]
fn reloading_channels_keeps_the_open_conversation_in_the_status() {
    let mut app = loaded();
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::Channels {
        rows: vec![row("C1", "#general"), row("C2", "#random")],
        people: vec![],
        names: NameBook::default(),
        badges: HashMap::new(),
        me: "U1".into(),
    });
    assert_eq!(app.status_line(), "C1");
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
    app.apply(Incoming::Live(Box::new(event)))
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
    let react = |user: &str, added: bool| rtm::Event::Reaction {
        channel: "C1".into(),
        ts: "1".into(),
        name: "tada".into(),
        user: user.into(),
        added,
    };
    live(&mut app, react("U1", true));
    live(&mut app, react("U2", true));
    assert_eq!(app.messages[0].reactions[0].count, 2);
    assert_eq!(app.messages[0].reactions[0].users, vec!["U1", "U2"]);
    live(&mut app, react("U1", false));
    assert_eq!(app.messages[0].reactions[0].users, vec!["U2"]);
    live(&mut app, react("U2", false));
    assert!(app.messages[0].reactions.is_empty());
    live(&mut app, rtm::Event::Deleted { channel: "C1".into(), ts: "2".into() });
    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.message_selected, 0);
}

fn typing(app: &mut App, user: &str) {
    live(app, rtm::Event::Typing { channel: "C1".into(), user: user.into() });
}

fn watching_c1() -> App {
    let mut app = loaded();
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
    app
}

#[test]
fn a_typist_shows_for_five_seconds_then_goes_quiet() {
    let mut app = watching_c1();
    typing(&mut app, "U2");
    assert_eq!(app.typing_line().as_deref(), Some("U2 is typing···"));
    app.now += Duration::from_secs(4);
    assert_eq!(app.typing_line().as_deref(), Some("U2 is typing···"));
    app.now += Duration::from_secs(1);
    assert_eq!(app.typing_line(), None);
}

#[test]
fn typing_again_extends_only_that_person() {
    let mut app = watching_c1();
    typing(&mut app, "U2");
    app.now += Duration::from_secs(3);
    typing(&mut app, "U3");
    app.now += Duration::from_secs(3);
    assert_eq!(app.typing_line().as_deref(), Some("U3 is typing···"));
    typing(&mut app, "U3");
    app.now += Duration::from_secs(4);
    assert_eq!(app.typing_line().as_deref(), Some("U3 is typing···"));
}

#[test]
fn typing_elsewhere_or_by_me_shows_nothing() {
    let mut app = watching_c1();
    live(&mut app, rtm::Event::Typing { channel: "C2".into(), user: "U2".into() });
    typing(&mut app, "U1");
    assert_eq!(app.typing_line(), None);
}

#[test]
fn several_typists_read_as_one_line_whatever_their_order() {
    let mut app = watching_c1();
    typing(&mut app, "U3");
    typing(&mut app, "U2");
    assert_eq!(app.typing_line().as_deref(), Some("U2 and U3 are typing···"));
    typing(&mut app, "U4");
    assert_eq!(app.typing_line().as_deref(), Some("3 people are typing···"));
}

#[test]
fn opening_another_conversation_forgets_its_typists() {
    let mut app = watching_c1();
    typing(&mut app, "U2");
    app.open_channel("C2".into());
    assert_eq!(app.typing_line(), None);
}

#[test]
fn polling_only_when_feed_is_down() {
    let mut app = loaded();
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::History { channel: "C1".into(), messages: vec![], names: NameBook::default() });
    assert_eq!(app.apply(Incoming::Tick), vec![Action::LoadHistory("C1".into())]);
    live(&mut app, rtm::Event::Connected);
    assert_eq!(app.apply(Incoming::Tick), vec![]);
    live(&mut app, rtm::Event::GaveUp("boom".into()));
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
        priority: None,
    }
}

#[test]
fn triage_ranks_the_visible_inbox_once_it_loads() {
    use crate::inbox::{Priority, Urgency};
    let mut app = loaded();
    app.handle_key(key('i'));
    let items = vec![inbox_item("a"), Item { ts: "11".into(), ..inbox_item("b") }];
    assert!(app.apply(Incoming::Inbox { items: items.clone(), names: NameBook::default() }).is_empty(), "triage off asks nothing");
    app.triage = true;
    let actions = app.apply(Incoming::Inbox { items: items.clone(), names: NameBook::default() });
    assert_eq!(actions, vec![Action::Prioritize(items)]);
    let urgent = Priority { needs_reply: true, urgency: Urgency::High };
    app.apply(Incoming::Priorities([("C2/11".to_owned(), urgent)].into()));
    let inbox = app.inbox.as_ref().unwrap();
    assert_eq!(inbox.items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    assert_eq!(inbox.selected_item().map(|i| i.key.as_str()), Some("a"), "the cursor stays on its item");
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
fn firehose_collects_every_channel_and_jumps() {
    let mut app = loaded();
    live(&mut app, rtm::Event::Message { channel: "C2".into(), message: msg("1", "one") });
    let reply = Message { thread_ts: Some("1".into()), ..msg("2", "two") };
    live(&mut app, rtm::Event::Message { channel: "C9".into(), message: reply });
    assert_eq!(app.wall.len(), 2);
    app.handle_key(key('f'));
    assert!(app.firehose.is_some());
    app.handle_key(key('k'));
    assert_eq!(app.firehose.as_ref().unwrap().selected, Some(0));
    app.handle_key(key('G'));
    assert!(app.firehose.as_ref().unwrap().following());
    let actions = app.handle_key(code(KeyCode::Enter));
    assert_eq!(actions, vec![Action::LoadHistory("C9".into()), Action::LoadReplies { channel: "C9".into(), ts: "1".into() }]);
    assert!(app.firehose.is_none());
    app.handle_key(key('f'));
    app.handle_key(code(KeyCode::Esc));
    assert!(app.firehose.is_none());
}

#[test]
fn triage_tags_live_lines_only_while_the_firehose_is_open() {
    use crate::firehose::Tag;
    let mut app = App { triage: true, ..loaded() };
    let actions = live(&mut app, rtm::Event::Message { channel: "C2".into(), message: msg("1", "one") });
    assert!(!actions.iter().any(|a| matches!(a, Action::Classify(_))), "closed firehose asks nothing");
    app.handle_key(key('f'));
    let actions = live(&mut app, rtm::Event::Message { channel: "C2".into(), message: msg("2", "lunch?") });
    assert!(actions.iter().any(|a| matches!(a, Action::Classify(line) if line.ts == "2")));
    live(&mut app, rtm::Event::Message { channel: "C2".into(), message: msg("3", "prod down") });
    app.apply(Incoming::Tagged { channel: "C2".into(), ts: "2".into(), tag: Tag::Noise });
    app.apply(Incoming::Tagged { channel: "C2".into(), ts: "3".into(), tag: Tag::Incident });
    let view = app.firehose.as_ref().unwrap();
    assert_eq!(view.visible(&app.wall).iter().map(|l| l.ts.as_str()).collect::<Vec<_>>(), ["1", "3"]);
    app.handle_key(key('k'));
    assert_eq!(
        app.handle_key(code(KeyCode::Enter))[0],
        Action::LoadHistory("C2".into()),
        "enter opens the visible line, not the hidden one"
    );
    app.handle_key(key('f'));
    app.handle_key(key('n'));
    assert_eq!(app.firehose.as_ref().unwrap().visible(&app.wall).len(), 3, "n shows the noise again");
}

#[test]
fn triage_failure_turns_it_off_with_one_notice() {
    let mut app = App { triage: true, ..loaded() };
    app.apply(Incoming::TriageUnavailable(crate::typesafe::Unavailable("quota exhausted".into())));
    assert_eq!(app.status_line(), "⚠ typesafe unavailable: quota exhausted");
    app.handle_key(key('j'));
    app.now += Duration::from_secs(3);
    app.apply(Incoming::TriageUnavailable(crate::typesafe::Unavailable("again".into())));
    assert!(!app.status_line().contains("again"));
    app.handle_key(key('f'));
    let actions = live(&mut app, rtm::Event::Message { channel: "C2".into(), message: msg("1", "one") });
    assert!(!actions.iter().any(|a| matches!(a, Action::Classify(_))));
}

#[test]
fn promises_need_triage_and_open_as_results_to_jump_to() {
    let mut app = loaded();
    assert!(app.handle_key(key('p')).is_empty());
    assert_eq!(app.status_line(), "promises need Jev: set `[typesafe] enabled = true`");
    app.triage = true;
    assert_eq!(app.handle_key(key('p')), vec![Action::LoadPromises]);
    assert!(app.loading);
    let promise =
        SearchMatch { ts: "7".into(), channel: crate::api::SearchChannel { id: "C2".into(), ..Default::default() }, ..Default::default() };
    app.apply(Incoming::SearchResults(vec![promise]));
    let actions = app.handle_key(code(KeyCode::Enter));
    assert_eq!(actions, vec![Action::LoadHistory("C2".into()), Action::LoadReplies { channel: "C2".into(), ts: "7".into() }]);
}

#[test]
fn unknown_live_authors_are_learned_once_seen() {
    let mut app = loaded();
    let actions =
        live(&mut app, rtm::Event::Message { channel: "C1".into(), message: Message { user: Some("U77".into()), ..msg("1", "x") } });
    assert_eq!(actions, vec![Action::LearnUsers(vec!["U77".into()])]);
}

#[test]
fn reading_mode_keeps_focus_on_the_conversation() {
    let mut app = loaded();
    app.handle_key(key('z'));
    assert!(!app.zen);
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
    app.handle_key(key('z'));
    assert!(app.zen);
    app.handle_key(code(KeyCode::Left));
    assert_eq!(app.focus, Focus::Messages);
    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Messages);
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::Replies { channel: "C1".into(), ts: "1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
    assert_eq!(app.focus, Focus::Thread);
    app.handle_key(code(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Messages);
    assert!(app.thread.is_none());
    app.handle_key(key('z'));
    assert!(!app.zen);
}

fn palette_run(app: &mut App, line: &str) -> Vec<Action> {
    app.handle_key(key(':'));
    for c in line.chars() {
        app.handle_key(key(c));
    }
    app.handle_key(code(KeyCode::Enter))
}

#[test]
fn palette_runs_verbs() {
    let mut app = loaded();
    assert_eq!(palette_run(&mut app, "go #random"), vec![Action::LoadHistory("C2".into())]);
    assert_eq!(palette_run(&mut app, "go @vivien"), vec![Action::OpenDm("U1".into())]);
    assert_eq!(palette_run(&mut app, "join #ops"), vec![Action::Join("#ops".into())]);
    assert_eq!(
        palette_run(&mut app, "msg @vivien hello there"),
        vec![Action::SendTo { target: "@vivien".into(), text: "hello there".into() }]
    );
    assert_eq!(palette_run(&mut app, "search deploy"), vec![Action::Search("deploy".into())]);
    let saved = |key: &str, value: &str| vec![Action::SaveSetting { key: key.into(), value: value.into() }];
    assert_eq!(palette_run(&mut app, "set highlight=#2a2a2a"), saved("highlight", "#2a2a2a"));
    assert_eq!(app.theme.highlight, Some("#2a2a2a".parse().unwrap()));
    assert_eq!(palette_run(&mut app, "set highlight=nope"), vec![]);
    assert_eq!(palette_run(&mut app, "set highlight=off"), saved("highlight", "none"));
    assert_eq!(app.theme.highlight, None);
    assert_eq!(palette_run(&mut app, "set theme=Tokyo Night"), saved("theme", "tokyonight"));
    assert_eq!(palette_run(&mut app, "set theme=nord"), saved("theme", "nord"));
    assert_eq!(app.theme.name, "nord");
    assert_eq!(app.theme.highlight, None);
    assert_eq!(palette_run(&mut app, "set images=off"), saved("images", "off"));
    assert_eq!(palette_run(&mut app, "set images=maybe"), vec![]);
    palette_run(&mut app, "set theme=solarized");
    assert!(app.status_line().contains("unknown theme") && app.status_line().contains("dracula"));
    assert_eq!(app.theme.name, "nord");
    palette_run(&mut app, "jion #x");
    assert!(app.status_line().contains("did you mean :join"));
    assert_eq!(palette_run(&mut app, "leave"), vec![Action::Leave("C2".into())]);
    app.apply(Incoming::Left("C2".into()));
    assert_eq!(app.current_channel, None);
    palette_run(&mut app, "q");
    assert!(app.should_quit);
}

#[test]
fn palette_completion_and_history() {
    let mut app = loaded();
    app.handle_key(key(':'));
    for c in "go ran".chars() {
        app.handle_key(key(c));
    }
    app.handle_key(code(KeyCode::Tab));
    assert_eq!(app.palette.as_ref().unwrap().input, "go #random ");
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(key(':'));
    for c in "go #gen".chars() {
        app.handle_key(key(c));
    }
    assert_eq!(app.palette_ghost().as_deref(), Some("eral"));
    app.handle_key(code(KeyCode::Right));
    assert_eq!(app.palette.as_ref().unwrap().input, "go #general ");
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(key(':'));
    app.handle_key(code(KeyCode::Up));
    assert_eq!(app.palette.as_ref().unwrap().input, "go #general");
    app.handle_key(code(KeyCode::Up));
    assert_eq!(app.palette.as_ref().unwrap().input, "go #random");
    app.handle_key(code(KeyCode::Esc));
    assert!(app.palette.is_none());
}

#[test]
fn palette_export_and_read_use_the_open_conversation() {
    let mut app = loaded();
    app.handle_key(code(KeyCode::Enter));
    app.apply(Incoming::History { channel: "C1".into(), messages: vec![msg("1", "a")], names: NameBook::default() });
    let actions = palette_run(&mut app, "export md");
    assert!(matches!(&actions[0], Action::Export { format: palette::Format::Markdown, messages, .. } if messages.len() == 1));
    assert_eq!(palette_run(&mut app, "read"), vec![Action::MarkChannelRead { channel: "C1".into(), ts: "1".into() }]);
    assert_eq!(palette_run(&mut app, "react rocket"), vec![Action::React { channel: "C1".into(), ts: "1".into(), name: "rocket".into() }]);
}

#[test]
fn edit_prefills_the_message_and_saves_the_change() {
    let mut app = reading();
    assert_eq!(palette_run(&mut app, "edit"), vec![]);
    assert_eq!(app.input, Some(Input::Edit { channel: "C1".into(), ts: "1".into() }));
    assert_eq!(app.buffer.text(), "a", "prefilled, with the cursor after the last letter");
    app.handle_key(key('!'));
    let actions = app.handle_key(code(KeyCode::Enter));
    assert_eq!(actions, vec![Action::Edit { channel: "C1".into(), ts: "1".into(), text: "a!".into() }]);
    assert_eq!(app.messages[0].text, "a", "the screen waits for the live event");
    live(&mut app, rtm::Event::Changed { channel: "C1".into(), message: msg("1", "a!") });
    assert_eq!(app.messages[0].text, "a!");
}

#[test]
fn an_empty_edit_is_refused_rather_than_deleting() {
    let mut app = reading();
    palette_run(&mut app, "edit");
    app.handle_key(code(KeyCode::Backspace));
    assert_eq!(app.handle_key(code(KeyCode::Enter)), vec![]);
    assert!(app.status_line().contains(":delete"), "{}", app.status_line());
    assert_eq!(app.messages.len(), 1);
}

#[test]
fn delete_asks_first_and_only_y_goes_through() {
    let mut app = reading();
    assert_eq!(palette_run(&mut app, "delete"), vec![]);
    assert_eq!(app.pending_delete, Some(MyMessage { channel: "C1".into(), ts: "1".into(), text: "a".into() }));
    assert_eq!(app.handle_key(key('n')), vec![]);
    assert_eq!(app.pending_delete, None);
    assert_eq!(app.status_line(), "kept");

    palette_run(&mut app, "delete");
    assert_eq!(app.handle_key(code(KeyCode::Esc)), vec![]);
    assert_eq!(app.pending_delete, None, "esc keeps it too");

    palette_run(&mut app, "delete");
    assert_eq!(app.handle_key(key('y')), vec![Action::Delete { channel: "C1".into(), ts: "1".into() }]);
    assert_eq!(app.pending_delete, None);
    assert_eq!(app.messages.len(), 1, "the screen waits for the live event");
    live(&mut app, rtm::Event::Deleted { channel: "C1".into(), ts: "1".into() });
    assert!(app.messages.is_empty());
}

#[test]
fn edit_and_delete_refuse_someone_elses_message() {
    let mut app = reading();
    app.apply(history(vec![from_other("2", "theirs")]));
    for verb in ["edit", "delete"] {
        assert_eq!(palette_run(&mut app, verb), vec![]);
        assert!(app.status_line().contains(&format!("you can only {verb} your own messages")), "{}", app.status_line());
    }
    assert_eq!(app.input, None);
    assert_eq!(app.pending_delete, None);
    assert_eq!(app.messages.len(), 1);
}

#[test]
fn edit_and_delete_follow_the_selection_into_a_thread() {
    let mut app = reading();
    app.thread = Some(Thread {
        channel: "C1".into(),
        root_ts: "1".into(),
        messages: vec![msg("1", "a"), from_other("2", "theirs"), msg("3", "mine")],
        selected: 2,
    });
    app.focus = Focus::Thread;
    palette_run(&mut app, "edit");
    assert_eq!(app.input, Some(Input::Edit { channel: "C1".into(), ts: "3".into() }));
    assert_eq!(app.buffer.text(), "mine");
    app.handle_key(code(KeyCode::Esc));

    palette_run(&mut app, "delete");
    assert_eq!(app.pending_delete.as_ref().map(|p| p.ts.clone()), Some("3".into()));
    app.handle_key(key('y'));

    app.thread.as_mut().unwrap().selected = 1;
    palette_run(&mut app, "delete");
    assert_eq!(app.pending_delete, None);
    assert!(app.status_line().contains("you can only delete your own messages"), "{}", app.status_line());
}

#[test]
fn a_search_hit_has_to_be_opened_before_it_can_be_deleted() {
    let mut app = reading();
    let hit =
        SearchMatch { ts: "9".into(), channel: crate::api::SearchChannel { id: "C2".into(), name: "random".into() }, ..Default::default() };
    app.apply(Incoming::SearchResults(vec![hit]));
    palette_run(&mut app, "delete");
    assert_eq!(app.pending_delete, None);
    assert!(app.status_line().contains("open the message first"), "{}", app.status_line());
}

#[test]
fn badges_come_from_counts_and_live_mentions() {
    let mut app = loaded();
    app.apply(Incoming::Channels {
        rows: vec![row("C1", "#general"), row("C2", "#random")],
        people: vec![],
        names: NameBook::default(),
        badges: HashMap::from([("C2".to_string(), Badge { unread: true, mentions: 2 })]),
        me: "U1".into(),
    });
    assert!(app.unread.contains("C2"));
    live(&mut app, rtm::Event::Message { channel: "C1".into(), message: Message { user: Some("U2".into()), ..msg("1", "<@U1> ping") } });
    assert_eq!(app.badges["C1"], Badge { unread: true, mentions: 1 });
    app.handle_key(code(KeyCode::Enter));
    assert!(!app.badges.contains_key("C1"));
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

/// `loaded()` with #general open on one message, so no empty state is animating.
fn reading() -> App {
    let mut app = loaded();
    app.handle_key(code(KeyCode::Enter));
    app.apply(history(vec![msg("1", "a")]));
    app
}

#[test]
fn toast_covers_the_status_for_two_seconds() {
    let mut app = reading();
    app.apply(Incoming::Toast("permalink copied".into()));
    assert_eq!(app.status_line(), "permalink copied");
    app.now += Duration::from_millis(1999);
    assert_eq!(app.status_line(), "permalink copied");
    app.now += Duration::from_millis(1);
    assert_eq!(app.status_line(), "C1");
}

#[test]
fn error_survives_time_and_clears_on_the_next_key() {
    let mut app = reading();
    app.apply(Incoming::Error("boom".into()));
    app.now += Duration::from_secs(60);
    assert_eq!(app.status_line(), "✗ boom");
    assert!(!app.animating(), "nothing to wake up for");
    app.handle_key(key('j'));
    assert_eq!(app.status_line(), "C1");
}

#[test]
fn key_that_fails_shows_its_own_error() {
    let mut app = reading();
    app.apply(Incoming::Error("boom".into()));
    palette_run(&mut app, "jion #x");
    assert!(app.status_line().contains("did you mean :join"));
}

#[test]
fn timed_toast_keeps_the_loop_awake_until_it_ends() {
    let mut app = reading();
    assert_eq!(app.redraw_in(), None);
    app.handle_key(key('u'));
    assert!(app.animating());
    assert_eq!(app.redraw_in(), Some(FRAME));
    app.now += Duration::from_secs(2);
    assert!(!app.animating());
    assert_eq!(app.redraw_in(), None);
}

#[test]
fn loading_wakes_the_loop_at_spinner_speed() {
    let mut app = reading();
    app.handle_key(key('R'));
    assert_eq!(app.redraw_in(), Some(SPINNER_FRAME));
    app.loading = false;
    app.handle_key(key('i'));
    assert_eq!(app.redraw_in(), Some(SPINNER_FRAME), "inbox is loading");
}

fn scrolled_up() -> App {
    let mut app = reading();
    app.apply(history(vec![msg("1", "a"), from_other("2", "b"), from_other("3", "c")]));
    app.handle_key(key('g'));
    app
}

#[test]
fn messages_arriving_below_the_selection_are_counted() {
    let mut app = scrolled_up();
    assert_eq!(app.new_below(), 0, "scrolling up over read messages counts nothing");
    app.apply(history(vec![msg("1", "a"), from_other("2", "b"), from_other("3", "c"), from_other("4", "d"), msg("5", "mine")]));
    assert_eq!(app.message_selected, 0);
    assert_eq!(app.new_below(), 1, "my own message does not count");
    live(&mut app, rtm::Event::Message { channel: "C1".into(), message: from_other("10", "e") });
    assert_eq!(app.new_below(), 2);
}

#[test]
fn reaching_new_messages_clears_the_count() {
    let mut app = scrolled_up();
    app.apply(history(vec![msg("1", "a"), from_other("2", "b"), from_other("3", "c"), from_other("4", "d"), from_other("5", "e")]));
    assert_eq!(app.new_below(), 2);
    for _ in 0..3 {
        app.handle_key(key('j'));
    }
    assert_eq!(app.new_below(), 1, "reached the first new one");
    app.handle_key(key('k'));
    assert_eq!(app.new_below(), 1, "going back up does not unsee it");
    app.handle_key(key('G'));
    assert_eq!(app.new_below(), 0);
}

#[test]
fn switching_conversation_clears_the_count() {
    let mut app = scrolled_up();
    app.apply(history(vec![msg("1", "a"), from_other("2", "b"), from_other("3", "c"), from_other("4", "d")]));
    assert_eq!(app.new_below(), 1);
    palette_run(&mut app, "go #random");
    assert_eq!(app.new_below(), 0);
    app.apply(Incoming::History { channel: "C2".into(), messages: vec![from_other("1", "x")], names: NameBook::default() });
    assert_eq!(app.new_below(), 0);
}

#[test]
fn refresh_at_the_bottom_counts_nothing() {
    let mut app = reading();
    app.apply(history(vec![msg("1", "a"), from_other("2", "b")]));
    assert_eq!(app.message_selected, 1);
    assert_eq!(app.new_below(), 0);
}
