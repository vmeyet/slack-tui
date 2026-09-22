//! `slack login` and `slack logout`: browser sign-in and keychain storage.
use crate::api::Slack;
use crate::auth::login::{self, LoginOptions};
use crate::auth::{self, Credentials, SecretStore, SecurityCli};
use crate::cache::Cache;
use crate::cli::LoginArgs;
use crate::config::{Config, Workspace};
use crate::render::Theme;
use anyhow::{Result, bail};
use std::time::Duration;

/// Signs in through the browser (or a pasted cookie) and stores the session in the keychain.
pub async fn run(args: LoginArgs, json: bool) -> Result<()> {
    let theme = Theme::detect();
    let workspace = args.workspace.as_deref().map(normalize_domain).transpose()?;
    let cookie = args.cookie.as_deref().map(super::read_input).transpose()?.map(|c| c.trim().to_owned());
    let options = LoginOptions {
        workspace,
        browser: args.browser,
        profile: args.profile,
        headless: args.headless,
        cookie,
        timeout: Duration::from_secs(args.timeout),
    };
    if options.cookie.is_none() {
        eprintln!("{} opening your browser, log in to Slack there. Waiting up to {}s…", theme.accent("→"), args.timeout);
    }
    let session = login::capture(&options).await?;
    let credentials = Credentials::new(&session.token, Some(&session.cookie));
    let slack = Slack::new(&Slack::api_url_from_env(), credentials.clone())?;
    let me = slack.auth_test().await?;
    let domain = session.domain;
    SecurityCli::new(auth::SERVICE).set(&domain, &credentials.to_json())?;
    let workspace =
        Workspace { team_id: me.team_id.clone(), team_name: me.team.clone(), user_id: me.user_id.clone(), user_name: me.user.clone() };
    Config::load()?.with_workspace(&domain, workspace).save()?;
    Cache::for_workspace(&domain).clear()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"workspace": domain, "user": me.user, "team": me.team}))?);
        return Ok(());
    }
    println!(
        "{} logged in as {} @ {} {}",
        theme.ok("✓"),
        theme.bold(&me.user),
        theme.accent(&me.team),
        theme.dim("· stored in your keychain")
    );
    Ok(())
}

/// Forgets a workspace: keychain entry, config and cache.
pub fn logout(workspace: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let Some(domain) = workspace.or_else(|| config.default.clone()) else { bail!("nothing to log out from") };
    SecurityCli::new(auth::SERVICE).delete(&domain)?;
    Cache::for_workspace(&domain).clear()?;
    config.without_workspace(&domain).save()?;
    println!("{} forgot {domain}", Theme::detect().ok("✓"));
    Ok(())
}

fn normalize_domain(input: &str) -> Result<String> {
    let s = input.trim().trim_start_matches("https://").trim_start_matches("http://");
    let domain = s.split('/').next().unwrap_or("").trim_end_matches(".slack.com").to_lowercase();
    if domain.is_empty() || !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("`{input}` does not look like a workspace domain (expected e.g. `acme` or `acme.slack.com`)");
    }
    Ok(domain)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn domains_normalize() {
        assert_eq!(normalize_domain("Acme").unwrap(), "acme");
        assert_eq!(normalize_domain("acme.slack.com").unwrap(), "acme");
        assert_eq!(normalize_domain("https://acme.slack.com/archives/C1").unwrap(), "acme");
        assert!(normalize_domain("").is_err());
        assert!(normalize_domain("bad domain").is_err());
    }
}
