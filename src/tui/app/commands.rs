use super::{Action, App, Focus, Input, MyMessage};
use crate::inbox::Snooze;
use crate::tui::images::Thumbs;
use crate::tui::palette::{self, Command, Format};
use crate::tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::LazyLock;

/// A command either yields actions or a reason it could not run, shown in the status line.
type Outcome = Result<Vec<Action>, String>;

impl App {
    pub(super) fn handle_palette_key(&mut self, key: KeyEvent) -> Vec<Action> {
        let palette = self.palette.as_mut().expect("palette open");
        match key.code {
            KeyCode::Esc => self.palette = None,
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => palette.type_char(c),
            KeyCode::Backspace => {
                if palette.input.is_empty() {
                    self.palette = None;
                } else {
                    palette.backspace();
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let candidates = self.completions_for(&self.palette.as_ref().expect("open").input);
                self.palette.as_mut().expect("open").complete(&candidates, key.code == KeyCode::BackTab);
            }
            KeyCode::Up => palette.history_up(),
            KeyCode::Down => palette.history_down(),
            KeyCode::Right | KeyCode::End => {
                let candidates = self.completions_for(&self.palette.as_ref().expect("open").input);
                self.palette.as_mut().expect("open").accept(&candidates);
            }
            KeyCode::Enter => {
                let line = palette.submit();
                self.palette_history = palette.history.clone();
                self.palette = None;
                return match palette::parse(&line) {
                    Ok(command) => self.run_command(command),
                    Err(e) => {
                        self.fail(e);
                        vec![]
                    }
                };
            }
            _ => {}
        }
        vec![]
    }

    pub fn palette_ghost(&self) -> Option<String> {
        let palette = self.palette.as_ref()?;
        palette.ghost(&self.completions_for(&palette.input))
    }

    /// The emoji list is thousands of names long, so it is borrowed rather than copied per frame.
    fn completions_for(&self, input: &str) -> Cow<'static, [String]> {
        let channels = || self.channels.iter().map(|c| c.label.clone());
        let people = || self.people.iter().map(|(_, h)| format!("@{h}"));
        match palette::slot(input) {
            palette::Slot::Verb => Cow::Owned(palette::VERBS.iter().map(|(v, _)| (*v).to_owned()).collect()),
            palette::Slot::Channel => Cow::Owned(channels().collect()),
            palette::Slot::Person => Cow::Owned(people().collect()),
            palette::Slot::Conversation => Cow::Owned(channels().chain(people()).collect()),
            palette::Slot::Emoji => Cow::Borrowed(emoji_names()),
            palette::Slot::Literal(options) => Cow::Owned(options.iter().map(|o| (*o).to_owned()).collect()),
            palette::Slot::Free => Cow::Owned(vec![]),
        }
    }

    fn run_command(&mut self, command: Command) -> Vec<Action> {
        let outcome = match command {
            Command::Join(name) => Ok(self.join(name)),
            Command::Leave(name) => self.leave(name),
            Command::Go(target) => self.go(&target),
            Command::Msg { target, text } => Ok(self.message(target, text)),
            Command::Compose => Ok(self.compose()),
            Command::React(name) => Ok(self.react(name)),
            Command::Edit => self.edit_selected(),
            Command::Delete => self.ask_delete(),
            Command::Thread => Ok(self.open_thread()),
            Command::Search(query) => Ok(self.search_for(query)),
            Command::Open => Ok(self.open_selected()),
            Command::Copy => Ok(self.copy_selected()),
            Command::Export(format) => self.export(format),
            Command::Read => Ok(self.mark_read()),
            Command::Snooze(preset) => self.snooze(preset),
            Command::Set { key, value } => self.set(key, value),
            Command::Help => Ok(self.show_help()),
            Command::Quit => Ok(self.quit()),
        };
        outcome.unwrap_or_else(|reason| {
            self.fail(reason);
            vec![]
        })
    }

    fn join(&mut self, name: String) -> Vec<Action> {
        self.toast(format!("joining {name}…"));
        vec![Action::Join(name)]
    }

    fn leave(&mut self, name: Option<String>) -> Outcome {
        let target = name.or_else(|| self.current_channel.clone()).ok_or("no conversation to leave")?;
        let id = self.channel_named(&target).map_or(target.clone(), |c| c.id.clone());
        Ok(vec![Action::Leave(id)])
    }

    fn go(&mut self, target: &str) -> Outcome {
        if let Some(id) = self.channel_named(target).map(|c| c.id.clone()) {
            return Ok(self.open_channel(id));
        }
        let Some(handle) = target.strip_prefix('@') else { return Err(format!("no conversation called {target}")) };
        let handle = handle.to_lowercase();
        let (id, _) = self.people.iter().find(|(_, h)| h.to_lowercase() == handle).ok_or_else(|| format!("nobody called {target}"))?;
        Ok(vec![Action::OpenDm(id.clone())])
    }

    fn message(&mut self, target: String, text: String) -> Vec<Action> {
        self.toast(format!("sending to {target}…"));
        vec![Action::SendTo { target, text }]
    }

    fn react(&self, name: String) -> Vec<Action> {
        let name = super::keys::shortcode(name.trim().trim_matches(':')).to_owned();
        self.selected_ref().map(|(channel, ts)| vec![Action::React { channel, ts, name }]).unwrap_or_default()
    }

    fn edit_selected(&mut self) -> Outcome {
        let MyMessage { channel, ts, text } = self.my_message("edit")?;
        self.start_input(Input::Edit { channel, ts }, text);
        Ok(vec![])
    }

    fn ask_delete(&mut self) -> Outcome {
        self.pending_delete = Some(self.my_message("delete")?);
        Ok(vec![])
    }

    fn open_thread(&mut self) -> Vec<Action> {
        self.focus = Focus::Messages;
        self.activate()
    }

    fn open_selected(&self) -> Vec<Action> {
        self.selected_ref().map(|(channel, ts)| vec![Action::Open { channel, ts }]).unwrap_or_default()
    }

    fn copy_selected(&self) -> Vec<Action> {
        self.selected_ref().map(|(channel, ts)| vec![Action::Yank { channel, ts }]).unwrap_or_default()
    }

    fn export(&self, format: Format) -> Outcome {
        let (label, messages) = match (&self.thread, self.focus) {
            (Some(t), Focus::Thread) => (format!("{} thread", self.current_label()), t.messages.clone()),
            _ => (self.current_label(), self.messages.clone()),
        };
        if messages.is_empty() {
            return Err("nothing to export".into());
        }
        Ok(vec![Action::Export { path: export_path(&label, format), label, messages, format }])
    }

    fn mark_read(&mut self) -> Vec<Action> {
        let (Some(channel), Some(last)) = (self.current_channel.clone(), self.messages.last()) else { return vec![] };
        self.unread.remove(&channel);
        vec![Action::MarkChannelRead { channel, ts: last.ts.clone() }]
    }

    fn snooze(&mut self, preset: Snooze) -> Outcome {
        let inbox = self.inbox.as_mut().filter(|i| i.selected_item().is_some()).ok_or("open the inbox (i) and pick an item first")?;
        inbox.snooze_selected(preset);
        Ok(self.persist_inbox())
    }

    fn set(&mut self, key: String, value: String) -> Outcome {
        let saved = match key.as_str() {
            "highlight" => self.set_highlight(&value)?,
            "theme" => self.set_theme(&value)?,
            "images" => self.set_images(&value)?,
            other => return Err(format!("unknown setting `{other}` (try theme, highlight, images)")),
        };
        let note = if key == "images" && saved == "on" && !self.thumbs.enabled() { " (restart to apply)" } else { "" };
        self.toast(format!("{key} = {saved}{note}"));
        Ok(vec![Action::SaveSetting { key, value: saved }])
    }

    fn set_highlight(&mut self, value: &str) -> Result<String, String> {
        if matches!(value, "none" | "off") {
            self.theme.highlight = None;
            return Ok("none".into());
        }
        let color = value.parse().map_err(|_| format!("`{value}` is not a colour (try `#2a2a2a`, `236` or `none`)"))?;
        self.theme.highlight = Some(color);
        Ok(value.to_owned())
    }

    fn set_theme(&mut self, value: &str) -> Result<String, String> {
        let theme = Theme::named(value).ok_or_else(|| format!("unknown theme `{value}` (try {})", Theme::NAMES.join(", ")))?;
        self.theme = theme;
        Ok(theme.name.to_owned())
    }

    fn set_images(&mut self, value: &str) -> Result<String, String> {
        match value {
            "off" => self.thumbs = Thumbs::off(),
            "on" => {}
            _ => return Err(format!("`{value}` is not on/off")),
        }
        Ok(value.to_owned())
    }

    fn show_help(&mut self) -> Vec<Action> {
        self.help = true;
        vec![]
    }

    fn quit(&mut self) -> Vec<Action> {
        self.should_quit = true;
        vec![]
    }
}

/// Every shortcode, in name order, for the `:react` slot and the react row.
pub(super) fn emoji_names() -> &'static [String] {
    static NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
        let mut names: Vec<String> = crate::emoji::names().map(str::to_owned).collect();
        names.sort();
        names
    });
    &NAMES
}

fn export_path(label: &str, format: Format) -> PathBuf {
    let stem: String = label.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let ext = match format {
        Format::Json => "json",
        Format::Markdown => "md",
    };
    let dir = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
    dir.join(format!("slack-{}-{}.{ext}", stem.trim_matches('-'), chrono::Local::now().format("%Y%m%d-%H%M")))
}
