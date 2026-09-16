use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,
}

impl Credentials {
    pub fn new(token: &str, cookie: Option<&str>) -> Self {
        Self { token: token.to_owned(), cookie: cookie.map(str::to_owned) }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("credentials serialize")
    }

    pub fn from_json(raw: &str) -> Result<Self> {
        serde_json::from_str(raw).context("stored credentials are corrupted; run `slack login` again")
    }

    pub fn cookie_header(&self) -> Option<String> {
        self.cookie.as_ref().map(|c| format!("d={c}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let c = Credentials::new("xoxc-abc", Some("xoxd-def"));
        assert_eq!(Credentials::from_json(&c.to_json()).unwrap(), c);
    }

    #[test]
    fn cookie_is_optional() {
        let c = Credentials::from_json(r#"{"token":"xoxp-1"}"#).unwrap();
        assert_eq!(c.cookie, None);
        assert_eq!(c.cookie_header(), None);
    }

    #[test]
    fn cookie_header_uses_d_name() {
        assert_eq!(Credentials::new("t", Some("xoxd-1")).cookie_header().unwrap(), "d=xoxd-1");
    }

    #[test]
    fn corrupted_json_gives_actionable_error() {
        let err = Credentials::from_json("{nope").unwrap_err();
        assert!(err.to_string().contains("slack login"));
    }
}
