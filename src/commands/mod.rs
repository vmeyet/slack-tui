pub mod api;
pub mod channels;
pub mod firehose;
pub mod inbox;
pub mod login;
pub mod messages;
pub mod react;
pub mod search;
pub mod send;
pub mod thread;
pub mod update;
pub mod users;
pub mod whoami;

use crate::ctx::Ctx;
use crate::permalink::{self, MessageRef};
use anyhow::{Result, bail};

/// A permalink, or `<channel> <ts>`, to a concrete message.
pub async fn parse_ref(ctx: &mut Ctx, args: &[String]) -> Result<MessageRef> {
    match args {
        [one] if permalink::is_permalink(one) => permalink::parse(one),
        [one] => bail!("expected a message permalink, got `{one}`"),
        [channel, ts] if permalink::is_ts(ts) => {
            Ok(MessageRef { channel: ctx.dir.channel_id(channel).await?, ts: ts.clone(), thread_ts: None })
        }
        [_, ts] => bail!("`{ts}` is not a message ts (expected 1234567890.123456)"),
        _ => bail!("expected a permalink or `<channel> <ts>`"),
    }
}

pub fn read_input(spec: &str) -> Result<String> {
    if spec == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        return Ok(buf);
    }
    std::fs::read_to_string(spec).map_err(|e| anyhow::anyhow!("reading {spec}: {e}"))
}
