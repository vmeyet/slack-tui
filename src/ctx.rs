use crate::api::Slack;
use crate::auth::{self, Env, SecretStore, SecurityCli};
use crate::cache::Cache;
use crate::config::Config;
use crate::render::Theme;
use crate::resolve::Directory;
use anyhow::Result;
use serde::Serialize;

pub struct Ctx {
    pub slack: Slack,
    pub dir: Directory,
    pub json: bool,
    pub theme: Theme,
    pub workspace: Option<String>,
    pub config: Config,
    pub cache: Cache,
}

impl Ctx {
    pub fn open(workspace: Option<&str>, json: bool) -> Result<Self> {
        let config = Config::load()?;
        let store = SecurityCli::new(auth::SERVICE);
        Self::build(&Env::from_process(), &store, &config, workspace, json)
    }

    pub fn build(env: &Env, store: &dyn SecretStore, config: &Config, workspace: Option<&str>, json: bool) -> Result<Self> {
        let resolved = auth::resolve(env, store, config, workspace)?;
        let slack = Slack::new(&Slack::api_url_from_env(), resolved.credentials)?;
        let cache = Cache::for_workspace(resolved.workspace.as_deref().unwrap_or("env"));
        Ok(Self {
            dir: Directory::new(slack.clone(), cache.clone()),
            slack,
            json,
            theme: Theme::detect().with_show_urls(config.links.show_url),
            workspace: resolved.workspace,
            config: config.clone(),
            cache,
        })
    }

    pub fn emit<T: Serialize>(&self, value: &T) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }
}
