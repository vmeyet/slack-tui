use anyhow::Result;
use clap::{CommandFactory, Parser};
use slack::cli::{Cli, Command};
use slack::commands;
use slack::ctx::Ctx;
use slack::render::Theme;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli).await {
        let theme = Theme::detect();
        eprintln!("{} {err}", theme.err("✗"));
        for cause in err.chain().skip(1) {
            eprintln!("  {}", theme.dim(&cause.to_string()));
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Login(args) => return commands::login::run(args, cli.json).await,
        Command::Logout { workspace } => return commands::login::logout(workspace.or(cli.workspace)),
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "slack", &mut std::io::stdout());
            return Ok(());
        }
        Command::Update(args) => return commands::update::run(args),
        _ => {}
    }
    let mut ctx = Ctx::open(cli.workspace.as_deref(), cli.json)?;
    match cli.command {
        Command::Whoami => commands::whoami::run(&mut ctx).await,
        Command::Send(args) => commands::send::run(&mut ctx, args).await,
        Command::Messages(args) => commands::messages::run(&mut ctx, args).await,
        Command::Thread(args) => commands::thread::run(&mut ctx, args).await,
        Command::Search(args) => commands::search::run(&mut ctx, args).await,
        Command::React(args) => commands::react::run(&mut ctx, args).await,
        Command::Channels(args) => commands::channels::run(&mut ctx, args).await,
        Command::Users(args) => commands::users::run(&mut ctx, args).await,
        Command::Api(args) => commands::api::run(&mut ctx, args).await,
        Command::Tui => slack::tui::run(ctx).await,
        Command::Inbox(args) => commands::inbox::run(ctx, args).await,
        Command::Firehose(args) => commands::firehose::run(&mut ctx, args).await,
        Command::Promises(args) => commands::promises::run(&mut ctx, args).await,
        Command::Login(_) | Command::Logout { .. } | Command::Completions { .. } | Command::Update(_) => unreachable!(),
    }
}
