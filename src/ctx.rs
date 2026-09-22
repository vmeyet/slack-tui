//! Everything a command needs once logged in: API client, directory, config and cache.
use crate::api::Slack;
use crate::auth::{self, Env, SecretStore, SecurityCli};
use crate::cache::Cache;
use crate::config::Config;
use crate::render::Theme;
use crate::resolve::Directory;
use anyhow::Result;
use serde::Serialize;

/// What every command works with once logged in.
pub struct Ctx {
    pub(crate) slack: Slack,
    pub(crate) dir: Directory,
    pub(crate) json: bool,
    pub(crate) theme: Theme,
    pub(crate) workspace: Option<String>,
    pub(crate) config: Config,
    pub(crate) cache: Cache,
}

impl Ctx {
    /// Loads config and credentials for the workspace and connects the API client.
    pub async fn open(workspace: Option<&str>, json: bool) -> Result<Self> {
        let config = Config::load()?;
        let store = SecurityCli::new(auth::SERVICE);
        Self::build(&Env::from_process(), &store, &config, workspace, json).await
    }

    pub(crate) async fn build(env: &Env, store: &dyn SecretStore, config: &Config, workspace: Option<&str>, json: bool) -> Result<Self> {
        let resolved = auth::resolve(env, store, config, workspace)?;
        let slack = Slack::new(&Slack::api_url_from_env(), resolved.credentials)?;
        let cache = Cache::for_workspace(resolved.workspace.as_deref().unwrap_or("env"));
        Ok(Self {
            dir: Directory::new(slack.clone(), cache.clone()).await,
            slack,
            json,
            theme: Theme::detect().with_show_urls(config.links.show_url),
            workspace: resolved.workspace,
            config: config.clone(),
            cache,
        })
    }

    #[allow(clippy::unused_self)]
    pub(crate) fn emit<T: Serialize>(&self, value: &T) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }
}
