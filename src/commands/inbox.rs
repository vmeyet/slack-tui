use crate::cli::InboxArgs;
use crate::ctx::Ctx;
use crate::inbox::{self, Item, State};
use crate::render;
use crate::typesafe::TypeSafe;
use anyhow::Result;
use std::io::IsTerminal;

pub async fn run(mut ctx: Ctx, args: InboxArgs) -> Result<()> {
    let interactive = !args.list && !ctx.json && std::io::stdout().is_terminal();
    if interactive {
        return crate::tui::run_inbox(ctx).await;
    }
    let me = ctx.slack.auth_test().await?.user_id;
    let items = inbox::fetch(&ctx.slack, &mut ctx.dir, &me).await?;
    let workspace = ctx.workspace.clone().unwrap_or_else(|| "env".into());
    let items = State::load(&workspace).visible(items, inbox::local_now());
    let items = if ctx.config.typesafe.enabled { ranked(&ctx, items, &me).await } else { items };
    if ctx.json {
        return ctx.emit(&items);
    }
    print!("{}", render::inbox(&ctx.theme, &ctx.dir.names(), &items));
    Ok(())
}

/// Jev's order when it answers; the fetched order, with a notice, when it cannot.
async fn ranked(ctx: &Ctx, items: Vec<Item>, me: &str) -> Vec<Item> {
    let names = ctx.dir.names();
    let verdicts = async { inbox::prioritize(&TypeSafe::connect()?, &ctx.cache, &items, &names, &names.user_label(me)).await }.await;
    match verdicts {
        Ok(verdicts) => inbox::rank(items, &verdicts),
        Err(unavailable) => {
            eprintln!("{}", ctx.theme.dim(&unavailable.notice()));
            items
        }
    }
}
