//! The command line: every subcommand and its flags, as clap parses them.
use crate::version;
use clap::{Args, Parser, Subcommand};

/// The whole command line: global flags plus the subcommand to run.
#[derive(Parser, Debug)]
#[command(name = "slack", version = version::label(), about = "Slack from your terminal, as yourself.", propagate_version = true)]
pub struct Cli {
    /// Workspace domain (the `acme` in acme.slack.com). Defaults to the last login.
    #[arg(short, long, global = true, env = "SLACK_WORKSPACE")]
    pub workspace: Option<String>,
    /// Print machine-readable JSON instead of the pretty output.
    #[arg(long, global = true)]
    pub json: bool,
    /// What to do. Left out on a terminal, opens the TUI.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Every subcommand `slack` understands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Log in through your browser and store the session in the keychain.
    Login(LoginArgs),
    /// Forget a workspace: keychain entry, config and cache.
    Logout {
        /// Workspace domain; the default workspace when omitted.
        workspace: Option<String>,
    },
    /// Show who you are logged in as.
    Whoami,
    /// Post a message, markdown by default.
    #[command(visible_alias = "post")]
    Send(SendArgs),
    /// List the latest messages of a channel or DM.
    #[command(visible_aliases = ["ls", "history"])]
    Messages(MessagesArgs),
    /// Show a thread from a permalink or `<channel> <ts>`.
    Thread(RefArgs),
    /// Search messages.
    Search(SearchArgs),
    /// Add an emoji reaction to a message.
    React(ReactArgs),
    /// List channels and DMs you are in.
    Channels(ChannelsArgs),
    /// List people.
    Users(UsersArgs),
    /// Call any Web API method: `slack api chat.postMessage channel=C1 text=hi`.
    Api(ApiArgs),
    /// Interactive terminal client.
    Tui,
    /// Unread DMs, mentions and thread replies. Interactive on a terminal, a list when piped.
    Inbox(InboxArgs),
    /// Stream every message from every conversation as one ticker, like tailing logs.
    Firehose(FirehoseArgs),
    /// Follow-ups you promised in your own messages and have not closed yet (needs `[typesafe] enabled`).
    #[command(visible_alias = "todo")]
    Promises(PromisesArgs),
    /// Generate shell completions.
    Completions {
        /// Which shell to generate for.
        shell: clap_complete::Shell,
    },
    /// Rebuild and install the latest `slack` with cargo.
    Update(UpdateArgs),
}

/// Flags of `slack update`.
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Install even when the running binary is already the latest commit.
    #[arg(short, long)]
    pub force: bool,
}

/// Flags of `slack login`.
#[derive(Args, Debug)]
pub struct LoginArgs {
    /// Workspace domain, e.g. `acme` or `acme.slack.com`. Detected from the browser when omitted.
    pub workspace: Option<String>,
    /// Browser to drive: brave, chrome, chromium, edge, or a path to its binary.
    #[arg(long)]
    pub browser: Option<String>,
    /// Reuse an existing browser profile directory instead of a throwaway one.
    #[arg(long, value_name = "DIR")]
    pub profile: Option<std::path::PathBuf>,
    /// Run the browser without a window (only useful with an already logged-in --profile).
    #[arg(long, requires = "profile")]
    pub headless: bool,
    /// Skip the browser: use this `d` cookie value (or `-` to read it from stdin).
    #[arg(long, value_name = "XOXD", requires = "workspace")]
    pub cookie: Option<String>,
    /// Seconds to wait for the login to complete.
    #[arg(long, default_value_t = 300)]
    pub timeout: u64,
}

/// Flags of `slack send`.
#[derive(Args, Debug)]
pub struct SendArgs {
    /// `#channel`, `@user`, a channel id, or a message permalink to reply to.
    pub target: String,
    /// The message. Reads stdin when omitted.
    pub text: Option<String>,
    /// Block Kit JSON, a file path or `-` for stdin.
    #[arg(long, value_name = "FILE", conflicts_with = "raw")]
    pub blocks: Option<String>,
    /// Send the text as Slack mrkdwn instead of converting markdown.
    #[arg(long)]
    pub raw: bool,
    /// Reply in a thread: a permalink or the root message ts.
    #[arg(short, long, value_name = "REF")]
    pub thread: Option<String>,
    /// Also show the thread reply in the channel.
    #[arg(long, requires = "thread")]
    pub broadcast: bool,
    /// Print the payload instead of sending it.
    #[arg(long)]
    pub dry_run: bool,
}

/// Flags of `slack messages`.
#[derive(Args, Debug)]
pub struct MessagesArgs {
    /// `#channel`, `@user` or a channel id.
    pub channel: String,
    /// Keep only the newest N messages [default: 30, or all of them with `--since`].
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,
    /// Only messages after: `2h`, `3d`, `1w` or `2026-09-01`.
    #[arg(long)]
    pub since: Option<String>,
    /// Expand thread replies inline.
    #[arg(short, long)]
    pub threads: bool,
    /// Keep printing new messages as they arrive (ctrl-c to stop).
    #[arg(short, long)]
    pub follow: bool,
}

/// A message reference, as `slack thread` takes it.
#[derive(Args, Debug)]
pub struct RefArgs {
    /// A message permalink, or `<channel> <ts>`.
    #[arg(required = true, num_args = 1..=2)]
    pub reference: Vec<String>,
}

/// A message reference plus the emoji, as `slack react` takes them.
#[derive(Args, Debug)]
pub struct ReactArgs {
    /// A message permalink or `<channel> <ts>`, then the emoji.
    #[arg(required = true, num_args = 2..=3)]
    pub args: Vec<String>,
}

/// Flags of `slack search`.
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Words to look for; Slack search syntax works too.
    #[arg(required = true)]
    pub query: Vec<String>,
    /// Limit to a channel.
    #[arg(long = "in", value_name = "CHANNEL")]
    pub channel: Option<String>,
    /// Limit to an author.
    #[arg(long, value_name = "USER")]
    pub from: Option<String>,
    /// Only after this date (YYYY-MM-DD).
    #[arg(long)]
    pub after: Option<String>,
    /// Only before this date (YYYY-MM-DD).
    #[arg(long)]
    pub before: Option<String>,
    /// How many results.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,
}

/// Flags of `slack channels`.
#[derive(Args, Debug)]
pub struct ChannelsArgs {
    /// Filter by substring.
    pub query: Option<String>,
    /// Include channels you are not a member of.
    #[arg(short, long)]
    pub all: bool,
    /// Refresh the local cache first.
    #[arg(long)]
    pub refresh: bool,
}

/// Flags of `slack users`.
#[derive(Args, Debug)]
pub struct UsersArgs {
    /// Filter by substring.
    pub query: Option<String>,
    /// Refresh the local cache first.
    #[arg(long)]
    pub refresh: bool,
}

/// Flags of `slack firehose`.
#[derive(Args, Debug)]
pub struct FirehoseArgs {
    /// Case-insensitive regex to light up (repeatable, adds to `[firehose] highlight` in config).
    #[arg(short = 'H', long = "highlight", value_name = "REGEX")]
    pub highlight: Vec<String>,
    /// Drop what Jev tags as noise (needs `[typesafe] enabled`).
    #[arg(long)]
    pub hide_noise: bool,
}

/// Flags of `slack inbox`.
#[derive(Args, Debug)]
pub struct InboxArgs {
    /// Print the list instead of opening the interactive view.
    #[arg(short, long)]
    pub list: bool,
}

/// Flags of `slack promises`.
#[derive(Args, Debug)]
pub struct PromisesArgs {
    /// Look back this far: `3d`, `2w` or `2026-09-01`.
    #[arg(long, default_value = crate::promises::DEFAULT_SINCE)]
    pub since: String,
    /// Also list the promises a later reply already closed.
    #[arg(short, long)]
    pub all: bool,
}

/// Flags of `slack api`.
#[derive(Args, Debug)]
pub struct ApiArgs {
    /// Method name, e.g. `conversations.info`.
    pub method: String,
    /// `key=value` pairs. Values starting with `{` or `[` are sent as JSON strings.
    pub params: Vec<String>,
    /// JSON object of parameters, a file path or `-` for stdin.
    #[arg(long, value_name = "FILE")]
    pub input: Option<String>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn send_parses_thread_and_broadcast() {
        let cli = Cli::parse_from(["slack", "send", "#general", "hi", "--thread", "1.2", "--broadcast"]);
        let Some(Command::Send(args)) = cli.command else { panic!() };
        assert_eq!(args.thread.as_deref(), Some("1.2"));
        assert!(args.broadcast);
    }

    #[test]
    fn broadcast_requires_thread() {
        assert!(Cli::try_parse_from(["slack", "send", "#g", "hi", "--broadcast"]).is_err());
    }

    #[test]
    fn update_takes_a_force_flag() {
        let cli = Cli::parse_from(["slack", "update", "-f"]);
        let Some(Command::Update(args)) = cli.command else { panic!() };
        assert!(args.force);
    }

    #[test]
    fn firehose_can_hide_noise() {
        let cli = Cli::parse_from(["slack", "firehose", "--hide-noise", "-H", "prod"]);
        let Some(Command::Firehose(args)) = cli.command else { panic!() };
        assert!(args.hide_noise);
        assert_eq!(args.highlight, ["prod"]);
    }

    #[test]
    fn promises_look_back_two_weeks_unless_told() {
        let Some(Command::Promises(args)) = Cli::parse_from(["slack", "todo"]).command else { panic!() };
        assert_eq!((args.since.as_str(), args.all), ("14d", false));
        let Some(Command::Promises(args)) = Cli::parse_from(["slack", "promises", "--since", "3d", "-a"]).command else { panic!() };
        assert_eq!((args.since.as_str(), args.all), ("3d", true));
    }

    #[test]
    fn global_flags_work_after_subcommand() {
        let cli = Cli::parse_from(["slack", "whoami", "--json", "-w", "acme"]);
        assert!(cli.json);
        assert_eq!(cli.workspace.as_deref(), Some("acme"));
    }
}
