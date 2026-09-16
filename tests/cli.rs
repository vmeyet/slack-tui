use assert_cmd::Command;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Env {
    server: MockServer,
    dir: tempfile::TempDir,
}

impl Env {
    async fn new() -> Self {
        Self { server: MockServer::start().await, dir: tempfile::tempdir().unwrap() }
    }

    async fn mock(&self, api_method: &str, body: Value) {
        let mut body = body;
        if body.get("ok").is_none() {
            body["ok"] = json!(true);
        }
        Mock::given(method("POST"))
            .and(path(format!("/{api_method}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    fn slack(&self) -> Command {
        let mut cmd = Command::cargo_bin("slack").unwrap();
        cmd.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.dir.path())
            .env("SLACK_TOKEN", "xoxc-test")
            .env("SLACK_COOKIE", "xoxd-test")
            .env("SLACK_CLI_API_URL", self.server.uri())
            .env("SLACK_CLI_CACHE_DIR", self.dir.path().join("cache"))
            .env("SLACK_CLI_CONFIG_DIR", self.dir.path().join("config"))
            .env("NO_COLOR", "1")
            .env("COLUMNS", "100")
            .env("TZ", "UTC");
        cmd
    }
}

fn stdout(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn stderr_failure(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(!out.status.success(), "expected failure, stdout: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stderr).unwrap()
}

fn channels_payload() -> Value {
    json!({"channels": [
        {"id": "C1", "name": "general", "is_member": true, "num_members": 42, "topic": {"value": "Company wide"}},
        {"id": "C2", "name": "vivien-vault", "is_private": true, "is_member": true, "num_members": 1},
        {"id": "C3", "name": "random", "is_member": false, "num_members": 40},
        {"id": "D1", "is_im": true, "user": "U2"}
    ]})
}

fn users_payload() -> Value {
    json!({"members": [
        {"id": "U1", "name": "vmeyet", "real_name": "Vivien Meyet", "profile": {"display_name": "vivien", "title": "Engineer"}},
        {"id": "U2", "name": "bob", "real_name": "Bob Builder", "profile": {"display_name": "", "title": ""}}
    ]})
}

#[tokio::test(flavor = "multi_thread")]
async fn whoami_pretty_and_json() {
    let env = Env::new().await;
    env.mock("auth.test", json!({"team_id": "T1", "team": "Acme", "user_id": "U1", "user": "vivien", "url": "https://acme.slack.com/"}))
        .await;
    let out = stdout(env.slack().arg("whoami"));
    assert_eq!(out, "✓ vivien @ Acme (https://acme.slack.com/ · U1)\n");
    let json: Value = serde_json::from_str(&stdout(env.slack().args(["whoami", "--json"]))).unwrap();
    assert_eq!(json["user"], "vivien");
}

#[tokio::test(flavor = "multi_thread")]
async fn not_logged_in_is_a_clear_error() {
    let env = Env::new().await;
    let err = stderr_failure(env.slack().env_remove("SLACK_TOKEN").arg("whoami"));
    assert_eq!(err, "✗ not logged in. Run `slack login <workspace>` first\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn api_error_is_reported() {
    let env = Env::new().await;
    env.mock("auth.test", json!({"ok": false, "error": "invalid_auth"})).await;
    let err = stderr_failure(env.slack().arg("whoami"));
    assert_eq!(err, "✗ auth.test failed: invalid_auth (session expired? run `slack login`)\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn messages_render() {
    let env = Env::new().await;
    env.mock("conversations.list", channels_payload()).await;
    env.mock("users.list", users_payload()).await;
    env.mock("conversations.history", json!({"messages": [
        {"ts": "1694700100.000200", "user": "U2", "text": "short reply <@U1> :tada:", "thread_ts": "1694700000.000100"},
        {"ts": "1694700000.000100", "user": "U1", "text": "Deploy *v2.3* is out, see <https://acme.io/notes|release notes> and <#C1|general>. This line is long enough to wrap around the terminal width for sure.", "reply_count": 2, "thread_ts": "1694700000.000100", "latest_reply": "1694700100.000200", "reactions": [{"name": "rocket", "count": 3}], "edited": {"ts": "1"}},
        {"ts": "1694600000.000100", "subtype": "channel_join", "user": "U2", "text": "<@U2> has joined"},
        {"ts": "1694500000.000100", "bot_id": "B1", "username": "deploybot", "text": "", "attachments": [{"fallback": "build #12 passed"}], "files": [{"name": "log.txt", "permalink": "https://acme.slack.com/files/log"}]}
    ]})).await;
    let out = stdout(env.slack().args(["messages", "#general", "-n", "4"]));
    insta::assert_snapshot!(out);
}

#[tokio::test(flavor = "multi_thread")]
async fn messages_json_and_since() {
    let env = Env::new().await;
    env.mock("conversations.list", channels_payload()).await;
    env.mock("users.list", users_payload()).await;
    Mock::given(path("/conversations.history"))
        .and(body_string_contains("oldest="))
        .and(body_string_contains("channel=C2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok": true, "messages": [{"ts": "1.000000", "user": "U1", "text": "hi"}]})),
        )
        .expect(1)
        .mount(&env.server)
        .await;
    let json: Value = serde_json::from_str(&stdout(env.slack().args(["messages", "vivien-vault", "--since", "2d", "--json"]))).unwrap();
    assert_eq!(json["channel"], "C2");
    assert_eq!(json["messages"][0]["text"], "hi");
}

#[tokio::test(flavor = "multi_thread")]
async fn send_markdown_dry_run_and_real() {
    let env = Env::new().await;
    env.mock("conversations.list", channels_payload()).await;
    env.mock("users.list", users_payload()).await;
    let dry = stdout(env.slack().args(["send", "#general", "hello **@bob** in #general", "--dry-run"]));
    let payload: Value = serde_json::from_str(&dry).unwrap();
    assert_eq!(payload["channel"], "C1");
    assert_eq!(payload["text"], "hello @bob in #general");
    let elements = &payload["blocks"][0]["elements"][0]["elements"];
    assert_eq!(elements[1], json!({"type": "user", "user_id": "U2"}));
    assert_eq!(elements[3], json!({"type": "channel", "channel_id": "C1"}));

    Mock::given(path("/chat.postMessage"))
        .and(body_string_contains("channel=C1"))
        .and(body_string_contains("blocks=%5B%7B%22type%22%3A%22rich_text%22"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "channel": "C1", "ts": "1694700000.000100"})))
        .expect(1)
        .mount(&env.server)
        .await;
    env.mock("chat.getPermalink", json!({"permalink": "https://acme.slack.com/archives/C1/p1694700000000100"})).await;
    let out = stdout(env.slack().args(["send", "general", "hello **@bob** in #general"]));
    assert_eq!(out, "✓ sent to #general https://acme.slack.com/archives/C1/p1694700000000100\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn send_reply_to_permalink_and_raw_from_stdin() {
    let env = Env::new().await;
    Mock::given(path("/chat.postMessage"))
        .and(body_string_contains("channel=C1"))
        .and(body_string_contains("thread_ts=1694700000.000100"))
        .and(body_string_contains("text=raw+*hi*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "channel": "C1", "ts": "1694700100.000200"})))
        .expect(1)
        .mount(&env.server)
        .await;
    env.mock("chat.getPermalink", json!({"permalink": "https://acme.slack.com/archives/C1/p1694700100000200"})).await;
    let out = stdout(
        env.slack()
            .args(["send", "https://acme.slack.com/archives/C1/p1694700100000150?thread_ts=1694700000.000100&cid=C1", "--raw"])
            .write_stdin("raw *hi*\n"),
    );
    assert_eq!(out, "✓ replied in C1 https://acme.slack.com/archives/C1/p1694700100000200\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn send_blocks_file_is_validated() {
    let env = Env::new().await;
    env.mock("conversations.list", channels_payload()).await;
    let file = env.dir.path().join("blocks.json");
    std::fs::write(&file, r#"{"blocks": [{"type": "header", "text": {"type": "plain_text", "text": "Deploy"}}, {"type": "divider"}]}"#)
        .unwrap();
    let dry = stdout(env.slack().args(["send", "#general", "--blocks", file.to_str().unwrap(), "--dry-run"]));
    let payload: Value = serde_json::from_str(&dry).unwrap();
    assert_eq!(payload["text"], "Deploy");
    assert_eq!(payload["blocks"].as_array().unwrap().len(), 2);

    std::fs::write(&file, r#"{"blocks": []}"#).unwrap();
    let err = stderr_failure(env.slack().args(["send", "#general", "--blocks", file.to_str().unwrap(), "--dry-run"]));
    assert_eq!(err, "✗ Slack accepts 1 to 50 blocks, got 0\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_channel_suggests() {
    let env = Env::new().await;
    env.mock("conversations.list", channels_payload()).await;
    let err = stderr_failure(env.slack().args(["send", "#gen", "x", "--dry-run"]));
    assert_eq!(err, "✗ channel `#gen` not found. Did you mean: #general\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn thread_render() {
    let env = Env::new().await;
    env.mock("conversations.list", channels_payload()).await;
    env.mock("users.list", users_payload()).await;
    Mock::given(path("/conversations.replies"))
        .and(body_string_contains("ts=1694700000.000100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "messages": [
            {"ts": "1694700000.000100", "user": "U1", "text": "root question?", "reply_count": 1, "thread_ts": "1694700000.000100"},
            {"ts": "1694700100.000200", "user": "U2", "text": "answer", "thread_ts": "1694700000.000100"}
        ]})))
        .expect(2)
        .mount(&env.server)
        .await;
    let by_link =
        stdout(env.slack().args(["thread", "https://acme.slack.com/archives/C1/p1694700100000200?thread_ts=1694700000.000100&cid=C1"]));
    let by_ts = stdout(env.slack().args(["thread", "#general", "1694700000.000100"]));
    assert_eq!(by_link, by_ts);
    insta::assert_snapshot!(by_link);
}

#[tokio::test(flavor = "multi_thread")]
async fn search_render() {
    let env = Env::new().await;
    Mock::given(path("/search.messages"))
        .and(body_string_contains("query=deploy+failed+in%3A%23ops+from%3A%40vivien"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "messages": {"total": 2, "matches": [
            {"ts": "1694700000.000100", "username": "vivien", "text": "deploy failed *again*", "permalink": "https://acme.slack.com/archives/C9/p1694700000000100", "channel": {"id": "C9", "name": "ops"}},
            {"ts": "1694600000.000100", "username": "vivien", "text": "deploy failed", "permalink": "https://acme.slack.com/archives/C9/p1694600000000100", "channel": {"id": "C9", "name": "ops"}}
        ]}})))
        .mount(&env.server)
        .await;
    let out = stdout(env.slack().args(["search", "deploy", "failed", "--in", "ops", "--from", "@vivien"]));
    insta::assert_snapshot!(out);
}

#[tokio::test(flavor = "multi_thread")]
async fn react_by_permalink() {
    let env = Env::new().await;
    Mock::given(path("/reactions.add"))
        .and(body_string_contains("channel=C1"))
        .and(body_string_contains("timestamp=1694700000.000100"))
        .and(body_string_contains("name=tada"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&env.server)
        .await;
    let out = stdout(env.slack().args(["react", "https://acme.slack.com/archives/C1/p1694700000000100", ":tada:"]));
    assert_eq!(out, "✓ reacted :tada:\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn channels_and_users_render() {
    let env = Env::new().await;
    env.mock("conversations.list", channels_payload()).await;
    env.mock("users.list", users_payload()).await;
    insta::assert_snapshot!("channels", stdout(env.slack().arg("channels")));
    insta::assert_snapshot!("channels_all_filtered", stdout(env.slack().args(["channels", "ra", "--all"])));
    insta::assert_snapshot!("users", stdout(env.slack().args(["users"])));
    let json: Value = serde_json::from_str(&stdout(env.slack().args(["users", "bob", "--json"]))).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn channel_cache_is_reused() {
    let env = Env::new().await;
    Mock::given(path("/conversations.list"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                channels_payload()
                    .as_object()
                    .map(|o| {
                        let mut o = o.clone();
                        o.insert("ok".into(), json!(true));
                        Value::Object(o)
                    })
                    .unwrap(),
            ),
        )
        .expect(2)
        .mount(&env.server)
        .await;
    env.mock("users.list", users_payload()).await;
    stdout(env.slack().arg("channels"));
    stdout(env.slack().arg("channels"));
    stdout(env.slack().args(["channels", "--refresh"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_api_call() {
    let env = Env::new().await;
    Mock::given(path("/conversations.info"))
        .and(body_string_contains("channel=C1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "channel": {"id": "C1"}})))
        .mount(&env.server)
        .await;
    let json: Value = serde_json::from_str(&stdout(env.slack().args(["api", "conversations.info", "channel=C1"]))).unwrap();
    assert_eq!(json["channel"]["id"], "C1");
}
