use crate::ctx::Ctx;
use crate::render;
use anyhow::Result;

pub async fn run(ctx: &mut Ctx) -> Result<()> {
    let me = ctx.slack.auth_test().await?;
    if ctx.json {
        return ctx.emit(&me);
    }
    print!("{}", render::whoami(&ctx.theme, &me, ctx.workspace.as_deref()));
    Ok(())
}
