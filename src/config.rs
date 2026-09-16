use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspaces: BTreeMap<String, Workspace>,
    #[serde(default, skip_serializing_if = "Tui::is_default")]
    pub tui: Tui,
    #[serde(default, skip_serializing_if = "Links::is_default")]
    pub links: Links,
    #[serde(default, skip_serializing_if = "Firehose::is_default")]
    pub firehose: Firehose,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Firehose {
    /// Case-insensitive regexes that light up a line in the firehose.
    #[serde(default)]
    pub highlight: Vec<String>,
}

impl Firehose {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Links {
    /// Print `label (url)` instead of a clickable label.
    #[serde(default)]
    pub show_url: bool,
}

impl Links {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tui {
    /// Background of the selected row: a name (`darkgray`), `#rrggbb`, or a 0-255 index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
}

impl Tui {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub team_id: String,
    pub team_name: String,
    pub user_id: String,
    pub user_name: String,
}

impl Config {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) => toml::from_str(&raw).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?).with_context(|| format!("writing {}", path.display()))
    }

    pub fn with_workspace(mut self, domain: &str, workspace: Workspace) -> Self {
        self.workspaces.insert(domain.to_owned(), workspace);
        self.default = Some(domain.to_owned());
        self
    }

    pub fn without_workspace(mut self, domain: &str) -> Self {
        self.workspaces.remove(domain);
        if self.default.as_deref() == Some(domain) {
            self.default = self.workspaces.keys().next().cloned();
        }
        self
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SLACK_CLI_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("slack-cli")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acme() -> Workspace {
        Workspace { team_id: "T1".into(), team_name: "Acme".into(), user_id: "U1".into(), user_name: "vivien".into() }
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load_from(&dir.path().join("nope.toml")).unwrap(), Config::default());
    }

    #[test]
    fn round_trips_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("config.toml");
        let config = Config::default().with_workspace("acme", acme());
        config.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap(), config);
    }

    #[test]
    fn adding_a_workspace_makes_it_default() {
        let config = Config::default().with_workspace("acme", acme()).with_workspace("beta", acme());
        assert_eq!(config.default.as_deref(), Some("beta"));
        assert_eq!(config.workspaces.len(), 2);
    }

    #[test]
    fn tui_section_is_optional() {
        let config: Config = toml::from_str("default = \"acme\"\n\n[tui]\nhighlight = \"#2a2a2a\"\n").unwrap();
        assert_eq!(config.tui.highlight.as_deref(), Some("#2a2a2a"));
        assert!(!config.links.show_url);
        let config: Config = toml::from_str("[links]\nshow_url = true\n\n[firehose]\nhighlight = [\"prod\", \"error|failed\"]\n").unwrap();
        assert!(config.links.show_url);
        assert_eq!(config.firehose.highlight, ["prod", "error|failed"]);
        assert!(!toml::to_string(&Config::default()).unwrap().contains("tui"));
    }

    #[test]
    fn removing_default_picks_another() {
        let config = Config::default().with_workspace("acme", acme()).with_workspace("beta", acme());
        let config = config.without_workspace("beta");
        assert_eq!(config.default.as_deref(), Some("acme"));
        assert_eq!(config.without_workspace("acme").default, None);
    }
}
