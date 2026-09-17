use crate::api::ChannelKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Public,
    Private,
    Dm,
    GroupDm,
}

impl From<ChannelKind> for Kind {
    fn from(k: ChannelKind) -> Self {
        match k {
            ChannelKind::Public => Kind::Public,
            ChannelKind::Private => Kind::Private,
            ChannelKind::Dm => Kind::Dm,
            ChannelKind::GroupDm => Kind::GroupDm,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelRow {
    pub id: String,
    pub label: String,
    pub kind: Kind,
    /// Sidebar group: "starred", a custom section name, "channels" or "direct".
    pub section: String,
    pub muted: bool,
}

impl ChannelRow {
    pub fn new(id: &str, label: &str, kind: Kind) -> Self {
        let section = match kind {
            Kind::Dm | Kind::GroupDm => "direct",
            _ => "channels",
        };
        Self { id: id.into(), label: label.into(), kind, section: section.into(), muted: false }
    }
}

/// Unread state of one conversation, from `client.counts` at load and RTM afterwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Badge {
    pub unread: bool,
    pub mentions: u64,
}

/// Puts rows in sidebar order: starred, custom sections, channels, direct; muted last in each group.
pub fn arrange(rows: Vec<ChannelRow>, sections: &[crate::api::Section], muted: &[String]) -> Vec<ChannelRow> {
    let placed: Vec<(String, Vec<String>)> = sections
        .iter()
        .filter(|s| !s.channel_ids_page.channel_ids.is_empty())
        .filter(|s| s.kind == "stars" || s.kind == "standard")
        .map(|s| (if s.kind == "stars" { "starred".to_owned() } else { s.name.to_lowercase() }, s.channel_ids_page.channel_ids.clone()))
        .collect();
    let mut rows: Vec<ChannelRow> = rows
        .into_iter()
        .map(|mut r| {
            if let Some((name, _)) = placed.iter().find(|(_, ids)| ids.contains(&r.id)) {
                r.section = name.clone();
            }
            r.muted = muted.contains(&r.id);
            r
        })
        .collect();
    let rank = |r: &ChannelRow| -> (usize, bool, String) {
        let group = match r.section.as_str() {
            "starred" => 0,
            "channels" => 2 + placed.len(),
            "direct" => 3 + placed.len(),
            custom => 1 + placed.iter().position(|(n, _)| n == custom).unwrap_or(0),
        };
        (group, r.muted, r.label.trim_start_matches(['#', '🔒', '@']).to_lowercase())
    };
    rows.sort_by_cached_key(rank);
    rows
}

/// What the sidebar draws: headers and spacers between groups, rows inside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarRow {
    Spacer,
    Header(String),
    Channel(usize),
}

pub fn sidebar_rows(channels: &[&ChannelRow], grouped: bool) -> Vec<SidebarRow> {
    let mut out = Vec::new();
    let mut current = String::new();
    for (i, c) in channels.iter().enumerate() {
        if grouped && c.section != current {
            if !out.is_empty() {
                out.push(SidebarRow::Spacer);
            }
            out.push(SidebarRow::Header(c.section.to_uppercase()));
            current = c.section.clone();
        }
        out.push(SidebarRow::Channel(i));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrange_groups_and_sorts_muted_last() {
        use crate::api::{Section, SectionPage};
        let rows = vec![
            ChannelRow::new("C1", "#zeta", Kind::Public),
            ChannelRow::new("C2", "#alpha", Kind::Public),
            ChannelRow::new("C3", "#ops", Kind::Private),
            ChannelRow::new("D1", "@bob", Kind::Dm),
            ChannelRow::new("C4", "#infra", Kind::Public),
        ];
        let sections = vec![
            Section { kind: "stars".into(), name: String::new(), channel_ids_page: SectionPage { channel_ids: vec!["C3".into()] } },
            Section { kind: "standard".into(), name: "Team".into(), channel_ids_page: SectionPage { channel_ids: vec!["C4".into()] } },
            Section { kind: "channels".into(), name: "Channels".into(), channel_ids_page: SectionPage::default() },
        ];
        let arranged = arrange(rows, &sections, &["C2".to_string()]);
        let order: Vec<(&str, &str, bool)> = arranged.iter().map(|r| (r.label.as_str(), r.section.as_str(), r.muted)).collect();
        assert_eq!(
            order,
            [
                ("#ops", "starred", false),
                ("#infra", "team", false),
                ("#zeta", "channels", false),
                ("#alpha", "channels", true),
                ("@bob", "direct", false)
            ]
        );
        let refs: Vec<&ChannelRow> = arranged.iter().collect();
        let rows = sidebar_rows(&refs, true);
        assert_eq!(rows[0], SidebarRow::Header("STARRED".into()));
        assert_eq!(rows[2], SidebarRow::Spacer);
        assert_eq!(rows[3], SidebarRow::Header("TEAM".into()));
        assert_eq!(rows.iter().filter(|r| matches!(r, SidebarRow::Channel(_))).count(), 5);
        assert_eq!(sidebar_rows(&refs, false).len(), 5);
    }
}
