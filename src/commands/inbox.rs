use crate::cli::InboxArgs;
use crate::ctx::Ctx;
use crate::inbox::{self, State};
use crate::render;
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
    if ctx.json {
        return ctx.emit(&items);
    }
    print!("{}", render::inbox(&ctx.theme, &ctx.dir.names(), &items));
    Ok(())
}
