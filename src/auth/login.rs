use super::cdp::{self, Cdp, Cookie};
use crate::pattern::regex;
use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::process::Command;

pub struct LoginOptions {
    pub workspace: Option<String>,
    pub browser: Option<String>,
    pub profile: Option<PathBuf>,
    pub headless: bool,
    pub cookie: Option<String>,
    pub timeout: Duration,
}

#[derive(Debug, PartialEq)]
pub struct Session {
    pub domain: String,
    pub token: String,
    pub cookie: String,
}

const BROWSERS: [(&str, &str); 4] = [
    ("brave", "Brave Browser.app/Contents/MacOS/Brave Browser"),
    ("chrome", "Google Chrome.app/Contents/MacOS/Google Chrome"),
    ("chromium", "Chromium.app/Contents/MacOS/Chromium"),
    ("edge", "Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
];

pub async fn capture(options: &LoginOptions) -> Result<Session> {
    if let Some(cookie) = &options.cookie {
        let Some(domain) = &options.workspace else { bail!("--cookie needs the workspace: slack login <workspace> --cookie …") };
        let token = fetch_token(&workspace_url(domain), cookie).await?.context("that cookie is not logged into this workspace")?;
        return Ok(Session { domain: domain.clone(), token, cookie: cookie.clone() });
    }
    let binary = find_browser(options.browser.as_deref())?;
    let scratch = tempfile::Builder::new().prefix("slack-login-").tempdir()?;
    let profile = options.profile.clone().unwrap_or_else(|| scratch.path().to_path_buf());
    let start_url = options.workspace.as_deref().map_or_else(|| "https://slack.com/signin".to_owned(), workspace_url);
    let mut child = launch(&binary, &profile, &start_url, options.headless)?;
    let result = tokio::time::timeout(options.timeout, wait_for_session(&profile, options.workspace.as_deref())).await;
    let _ = child.start_kill();
    let _ = child.wait().await;
    match result {
        Ok(session) => session,
        Err(_) => bail!("timed out after {}s waiting for the login", options.timeout.as_secs()),
    }
}

fn debug(msg: &str) {
    if std::env::var_os("SLACK_CLI_DEBUG").is_some() {
        eprintln!("[debug] {msg}");
    }
}

fn workspace_url(domain: &str) -> String {
    format!("https://{domain}.slack.com/")
}

fn find_browser(choice: Option<&str>) -> Result<PathBuf> {
    let roots = [PathBuf::from("/Applications"), dirs::home_dir().unwrap_or_default().join("Applications")];
    let candidates: Vec<(&str, PathBuf)> = roots.iter().flat_map(|r| BROWSERS.iter().map(move |(n, rel)| (*n, r.join(rel)))).collect();
    match choice {
        Some(path) if Path::new(path).is_file() => Ok(PathBuf::from(path)),
        Some(name) => candidates
            .iter()
            .find(|(n, p)| *n == name.to_lowercase() && p.is_file())
            .map(|(_, p)| p.clone())
            .with_context(|| format!("browser `{name}` not found (try brave, chrome, chromium, edge, or a path)")),
        None => candidates
            .iter()
            .find(|(_, p)| p.is_file())
            .map(|(_, p)| p.clone())
            .context("no Chromium-based browser found in /Applications (Brave, Chrome, Chromium or Edge)"),
    }
}

fn launch(binary: &Path, profile: &Path, url: &str, headless: bool) -> Result<tokio::process::Child> {
    let _ = std::fs::remove_file(profile.join("DevToolsActivePort"));
    let mut cmd = Command::new(binary);
    cmd.arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-sync")
        .arg("--window-size=1100,800")
        .arg("--new-window")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if headless {
        cmd.arg("--headless=new");
    }
    cmd.spawn().with_context(|| format!("launching {}", binary.display()))
}

async fn wait_for_session(profile: &Path, workspace: Option<&str>) -> Result<Session> {
    let ws_url = cdp::wait_for_active_port(profile, Duration::from_secs(20)).await?;
    let mut cdp = Cdp::connect(&ws_url).await?;
    let session = loop {
        let cookies = cdp.cookies().await?;
        debug(&format!(
            "slack cookies: {:?}",
            cookies.iter().filter(|c| c.domain.contains("slack")).map(|c| c.name.as_str()).collect::<Vec<_>>()
        ));
        if let Some(cookie) = session_cookie(&cookies) {
            let pages = cdp.pages().await?;
            debug(&format!("pages: {:?}", pages.iter().map(|(_, u)| u).collect::<Vec<_>>()));
            if let Some((domain, token)) = token_from_client(&mut cdp, &pages, workspace).await {
                break Session { domain, token, cookie };
            }
            let urls: Vec<String> = pages.iter().map(|(_, u)| u.clone()).collect();
            if let Some(domain) = workspace.map(str::to_owned).or_else(|| pick_workspace(&urls))
                && let Some(token) = fetch_token(&workspace_url(&domain), &cookie).await?
            {
                break Session { domain, token, cookie };
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    cdp.close_browser().await;
    Ok(session)
}

/// The web client keeps one token per team in localStorage, with the team's domain next to it.
async fn token_from_client(cdp: &mut Cdp, pages: &[(String, String)], workspace: Option<&str>) -> Option<(String, String)> {
    for (id, _) in pages.iter().filter(|(id, url)| !id.is_empty() && url.starts_with("https://app.slack.com/")) {
        let raw = cdp.evaluate(id, "localStorage.getItem('localConfig_v2')").await.ok()?;
        if let Some(found) = raw.as_str().and_then(|r| team_token(r, workspace)) {
            return Some(found);
        }
    }
    None
}

#[derive(Deserialize)]
struct LocalConfig {
    teams: IndexMap<String, Team>,
}

#[derive(Deserialize)]
struct Team {
    domain: Option<String>,
    token: Option<String>,
}

pub fn team_token(local_config: &str, workspace: Option<&str>) -> Option<(String, String)> {
    let config: LocalConfig = serde_json::from_str(local_config).ok()?;
    let mut found = config.teams.into_values().filter_map(|t| Some((t.domain?, t.token?))).filter(|(_, tok)| tok.starts_with("xox"));
    match workspace {
        Some(w) => found.find(|(d, _)| d == w),
        None => found.next(),
    }
}

fn session_cookie(cookies: &[Cookie]) -> Option<String> {
    cookies.iter().find(|c| c.name == "d" && c.domain.ends_with("slack.com") && c.value.starts_with("xoxd-")).map(|c| c.value.clone())
}

static WORKSPACE_URL: LazyLock<Regex> = LazyLock::new(|| regex(r"^https://([a-z0-9-]+)\.slack\.com(?:/|$)"));
const NOT_WORKSPACES: [&str; 6] = ["app", "www", "slack", "api", "a", "files"];

pub fn pick_workspace(urls: &[String]) -> Option<String> {
    urls.iter().filter_map(|u| WORKSPACE_URL.captures(u).map(|c| c[1].to_owned())).find(|d| !NOT_WORKSPACES.contains(&d.as_str()))
}

static API_TOKEN: LazyLock<Regex> = LazyLock::new(|| regex(r#""api_token"\s*:\s*"(xox[a-z]-[A-Za-z0-9-]+)""#));

pub fn token_in_html(html: &str) -> Option<String> {
    API_TOKEN.captures(html).map(|c| c[1].to_owned())
}

/// The workspace home page embeds the web client's token when the `d` cookie is a live session.
pub async fn fetch_token(url: &str, cookie: &str) -> Result<Option<String>> {
    let http = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Safari/537.36")
        .timeout(Duration::from_secs(20))
        .build()?;
    let html = http.get(url).header("Cookie", format!("d={cookie}")).send().await?.text().await?;
    Ok(token_in_html(&html))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn cookie(name: &str, value: &str) -> Cookie {
        Cookie { name: name.into(), value: value.into(), domain: ".slack.com".into() }
    }

    #[test]
    fn finds_the_session_cookie_only() {
        let cookies = vec![cookie("d", "not-a-session"), cookie("b", "xoxd-nope"), cookie("d", "xoxd-real")];
        assert_eq!(session_cookie(&cookies).as_deref(), Some("xoxd-real"));
        assert_eq!(session_cookie(&[]), None);
    }

    #[test]
    fn picks_the_workspace_from_open_tabs() {
        let urls = vec!["https://app.slack.com/client/T1/C1".to_owned(), "https://acme.slack.com/ssb/redirect".to_owned()];
        assert_eq!(pick_workspace(&urls).as_deref(), Some("acme"));
        assert_eq!(pick_workspace(&["https://slack.com/signin".to_owned()]), None);
    }

    #[test]
    fn reads_team_token_from_local_config() {
        let cfg = r#"{"teams":{"T1":{"domain":"acme","token":"xoxc-1"},"T2":{"domain":"beta","token":"xoxc-2"}}}"#;
        assert_eq!(team_token(cfg, Some("beta")), Some(("beta".into(), "xoxc-2".into())));
        assert_eq!(team_token(cfg, Some("nope")), None);
        assert!(team_token(cfg, None).is_some());
        assert_eq!(team_token("{}", None), None);
    }

    #[test]
    fn without_a_workspace_takes_the_first_team_with_a_token_in_file_order() {
        let cfg = r#"{"teams":{"T9":{"domain":"zeta"},"T5":{"domain":"acme","token":"xoxc-5"},"T1":{"domain":"beta","token":"xoxc-1"}}}"#;
        assert_eq!(team_token(cfg, None), Some(("acme".into(), "xoxc-5".into())));
    }

    #[test]
    fn extracts_token_from_boot_data() {
        let html = r#"<script>var boot = {"api_token":"xoxc-123-abc","other":1};</script>"#;
        assert_eq!(token_in_html(html).as_deref(), Some("xoxc-123-abc"));
        assert_eq!(token_in_html("<html>login</html>"), None);
    }

    #[tokio::test]
    async fn fetch_token_sends_cookie() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(header("Cookie", "d=xoxd-1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"api_token":"xoxc-9"}"#))
            .mount(&server)
            .await;
        assert_eq!(fetch_token(&format!("{}/", server.uri()), "xoxd-1").await.unwrap().as_deref(), Some("xoxc-9"));
    }
}
