//! `slack api`: call any Web API method by name.
use super::read_input;
use crate::api::Params;
use crate::cli::ApiArgs;
use crate::ctx::Ctx;
use anyhow::{Result, bail};
use serde_json::Value;

fn build_params(pairs: &[String], input: Option<&str>) -> Result<Params> {
    let mut params = Params::new();
    if let Some(raw) = input {
        let Value::Object(map) = serde_json::from_str::<Value>(raw)? else { bail!("--input must be a JSON object") };
        for (k, v) in map {
            params.push((k, value_to_param(&v)));
        }
    }
    for pair in pairs {
        let Some((k, v)) = pair.split_once('=') else { bail!("`{pair}` is not key=value") };
        params.push((k.to_owned(), v.to_owned()));
    }
    Ok(params)
}

fn value_to_param(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Calls the Web API method and prints its JSON answer.
pub async fn run(ctx: &mut Ctx, args: ApiArgs) -> Result<()> {
    let input = args.input.as_deref().map(read_input).transpose()?;
    let params = build_params(&args.params, input.as_deref())?;
    let body = ctx.slack.call(&args.method, params).await?;
    ctx.emit(&body)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn pairs_and_json_input_merge() {
        let params =
            build_params(&["channel=C1".into(), "text=a=b".into()], Some(r#"{"limit": 5, "blocks": [{"type":"divider"}], "name": "x"}"#))
                .unwrap();
        assert_eq!(
            params,
            vec![
                ("limit".to_owned(), "5".to_owned()),
                ("blocks".to_owned(), r#"[{"type":"divider"}]"#.to_owned()),
                ("name".to_owned(), "x".to_owned()),
                ("channel".to_owned(), "C1".to_owned()),
                ("text".to_owned(), "a=b".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_bad_pair() {
        assert!(build_params(&["nope".into()], None).is_err());
        assert!(build_params(&[], Some("[1]")).is_err());
    }
}
