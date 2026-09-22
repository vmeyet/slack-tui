pub mod cdp;
pub mod credentials;
pub mod login;
pub mod store;

pub use credentials::Credentials;
#[cfg(test)]
pub use store::MemoryStore;
pub use store::{SecretStore, SecurityCli};

use crate::config::Config;
use anyhow::{Result, bail};

pub const SERVICE: &str = "slack-cli";

pub struct Env {
    pub token: Option<String>,
    pub cookie: Option<String>,
}

impl Env {
    pub fn from_process() -> Self {
        Self {
            token: std::env::var("SLACK_TOKEN").ok().filter(|s| !s.is_empty()),
            cookie: std::env::var("SLACK_COOKIE").ok().filter(|s| !s.is_empty()),
        }
    }
}

#[derive(Debug)]
pub struct Resolved {
    pub credentials: Credentials,
    pub workspace: Option<String>,
}

pub fn resolve(env: &Env, store: &dyn SecretStore, config: &Config, workspace: Option<&str>) -> Result<Resolved> {
    if let Some(token) = &env.token {
        return Ok(Resolved { credentials: Credentials::new(token, env.cookie.as_deref()), workspace: None });
    }
    let Some(domain) = workspace.map(str::to_owned).or_else(|| config.default.clone()) else {
        bail!("not logged in. Run `slack login <workspace>` first");
    };
    let Some(secret) = store.get(&domain)? else {
        bail!("no credentials for workspace `{domain}`. Run `slack login {domain}`");
    };
    Ok(Resolved { credentials: Credentials::from_json(&secret)?, workspace: Some(domain) })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn no_env() -> Env {
        Env { token: None, cookie: None }
    }

    #[test]
    fn env_token_wins_over_everything() {
        let env = Env { token: Some("xoxp-env".into()), cookie: None };
        let r = resolve(&env, &MemoryStore::default(), &Config::default(), Some("acme")).unwrap();
        assert_eq!(r.credentials.token, "xoxp-env");
        assert_eq!(r.workspace, None);
    }

    #[test]
    fn explicit_workspace_reads_store() {
        let store = MemoryStore::default();
        store.set("acme", &Credentials::new("xoxc-1", Some("xoxd-1")).to_json()).unwrap();
        let r = resolve(&no_env(), &store, &Config::default(), Some("acme")).unwrap();
        assert_eq!(r.credentials.cookie.as_deref(), Some("xoxd-1"));
        assert_eq!(r.workspace.as_deref(), Some("acme"));
    }

    #[test]
    fn falls_back_to_config_default() {
        let store = MemoryStore::default();
        store.set("acme", &Credentials::new("xoxc-1", None).to_json()).unwrap();
        let config = Config { default: Some("acme".into()), ..Default::default() };
        let r = resolve(&no_env(), &store, &config, None).unwrap();
        assert_eq!(r.credentials.token, "xoxc-1");
    }

    #[test]
    fn missing_everything_asks_to_login() {
        let err = resolve(&no_env(), &MemoryStore::default(), &Config::default(), None).unwrap_err();
        assert!(err.to_string().contains("slack login"));
    }

    #[test]
    fn missing_secret_names_workspace() {
        let err = resolve(&no_env(), &MemoryStore::default(), &Config::default(), Some("acme")).unwrap_err();
        assert!(err.to_string().contains("slack login acme"));
    }
}
