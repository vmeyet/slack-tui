use crate::api::rtm;
use crate::cli::FirehoseArgs;
use crate::ctx::Ctx;
use crate::firehose::{Highlighter, Line};
use crate::render;
use anyhow::{Result, bail};
use std::io::Write;
use tokio::sync::mpsc;

pub async fn run(ctx: &mut Ctx, args: FirehoseArgs) -> Result<()> {
    let patterns: Vec<String> = ctx.config.firehose.highlight.iter().cloned().chain(args.highlight).collect();
    let hl = Highlighter::new(&patterns)?;
    let _ = ctx.dir.channels().await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(rtm::stream(ctx.slack.clone(), tx));
    eprintln!("{}", ctx.theme.dim("firehose… ctrl-c to stop"));
    while let Some(event) = rx.recv().await {
        match &event {
            rtm::Event::Connected => eprintln!("{}", ctx.theme.ok("● live")),
            rtm::Event::Disconnected(reason) if reason.contains("giving up") => bail!("the live feed is unavailable: {reason}"),
            _ => {}
        }
        let Some(line) = Line::from_event(&event) else { continue };
        if let Some(user) = &line.user {
            ctx.dir.learn_ids(std::slice::from_ref(user)).await?;
        }
        if ctx.json {
            println!("{}", serde_json::to_string(&line)?);
        } else {
            print!("{}", render::firehose_line(&ctx.theme, &ctx.dir.names(), &line, &hl));
        }
        std::io::stdout().flush()?;
    }
    Ok(())
}
