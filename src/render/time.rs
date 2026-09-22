use chrono::{DateTime, Datelike, Local, TimeZone, Utc};

pub fn parse_ts(ts: &str) -> Option<DateTime<Local>> {
    let secs: f64 = ts.parse().ok()?;
    Utc.timestamp_opt(secs.trunc() as i64, 0).single().map(|d| d.with_timezone(&Local))
}

pub fn hhmm(ts: &str) -> String {
    parse_ts(ts).map_or_else(|| "??:??".into(), |d| d.format("%H:%M").to_string())
}

pub fn day_label(ts: &str) -> String {
    let Some(d) = parse_ts(ts) else { return String::new() };
    let today = Local::now().date_naive();
    let date = d.date_naive();
    if date == today {
        "Today".into()
    } else if date == today.pred_opt().unwrap_or(today) {
        "Yesterday".into()
    } else if date.year() == today.year() {
        d.format("%a %-d %b").to_string()
    } else {
        d.format("%-d %b %Y").to_string()
    }
}

pub fn relative(ts: &str) -> String {
    let Some(d) = parse_ts(ts) else { return String::new() };
    let secs = (Local::now() - d).num_seconds().max(0);
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86400 => format!("{}h ago", s / 3600),
        s if s < 86400 * 14 => format!("{}d ago", s / 86400),
        _ => d.format("%-d %b").to_string(),
    }
}

/// `2h`, `3d`, `1w`, `45m`, or `YYYY-MM-DD`, to a Slack ts.
pub fn since_to_ts(spec: &str) -> Option<String> {
    let spec = spec.trim();
    if let Ok(date) = chrono::NaiveDate::parse_from_str(spec, "%Y-%m-%d") {
        let start = Local.from_local_datetime(&date.and_hms_opt(0, 0, 0)?).single()?;
        return Some(format!("{}.000000", start.timestamp()));
    }
    let (num, unit) = spec.split_at(spec.len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    let secs = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        "w" => 7 * 86400,
        _ => return None,
    };
    Some(format!("{}.000000", Utc::now().timestamp() - n * secs))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn parses_slack_ts() {
        assert!(parse_ts("1694700000.123456").is_some());
        assert!(parse_ts("nope").is_none());
        assert_eq!(hhmm("nope"), "??:??");
    }

    #[test]
    fn since_specs() {
        let now = Utc::now().timestamp();
        let two_hours: f64 = since_to_ts("2h").unwrap().parse().unwrap();
        assert!((now - 7200 - two_hours as i64).abs() <= 1);
        assert!(since_to_ts("1w").unwrap().ends_with(".000000"));
        assert!(since_to_ts("2026-09-01").is_some());
        assert_eq!(since_to_ts("soon"), None);
        assert_eq!(since_to_ts(""), None);
    }

    #[test]
    fn day_labels() {
        let now = Utc::now().timestamp();
        assert_eq!(day_label(&format!("{now}.000000")), "Today");
        assert_eq!(day_label(&format!("{}.000000", now - 86400)), "Yesterday");
        assert_eq!(day_label("x"), "");
    }
}
