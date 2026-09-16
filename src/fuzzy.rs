//! Subsequence matching the way fzf does it: every query char must appear in order,
//! word starts and runs of consecutive hits score higher, gaps cost.

const WORD_START: i64 = 10;
const CONSECUTIVE: i64 = 6;
const GAP: i64 = 1;

pub fn score(query: &str, candidate: &str) -> Option<i64> {
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = candidate.chars().flat_map(char::to_lowercase).collect();
    let word_start = |i: usize| i == 0 || !text[i - 1].is_alphanumeric();
    // best[i]: best score with the current query char matched at text index i
    let mut best: Vec<Option<i64>> =
        (0..text.len()).map(|i| (text[i] == query[0]).then(|| 1 + if word_start(i) { WORD_START } else { 0 })).collect();
    for &q in &query[1..] {
        let mut next: Vec<Option<i64>> = vec![None; text.len()];
        for i in 1..text.len() {
            if text[i] != q {
                continue;
            }
            let bonus = if word_start(i) { WORD_START } else { 0 };
            next[i] = (0..i)
                .filter_map(|j| {
                    let prev = best[j]?;
                    let link = if word_start(i) {
                        0
                    } else if j + 1 == i {
                        CONSECUTIVE
                    } else {
                        -GAP * (i - j - 1) as i64
                    };
                    Some(prev + 1 + bonus + link)
                })
                .max();
        }
        best = next;
    }
    best.into_iter().flatten().max().map(|s| s - text.len() as i64 / 2)
}

/// Candidates ordered best first, ties keeping the input order.
pub fn rank<T>(query: &str, items: impl IntoIterator<Item = (String, T)>) -> Vec<(i64, T)> {
    let mut scored: Vec<(i64, T)> = items.into_iter().filter_map(|(label, item)| score(query, &label).map(|s| (s, item))).collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored
}

/// The single obvious match, when one candidate clearly beats the rest.
pub fn best<T>(query: &str, items: impl IntoIterator<Item = (String, T)>) -> Option<T> {
    let mut ranked = rank(query, items).into_iter();
    let (top, item) = ranked.next()?;
    let runner_up = ranked.next().map(|(s, _)| s).unwrap_or(i64::MIN);
    (top > 0 && (runner_up <= 0 || top * 3 > runner_up * 4)).then_some(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(query: &str, items: &[&str]) -> Vec<String> {
        rank(query, items.iter().map(|i| (i.to_string(), i.to_string()))).into_iter().map(|(_, i)| i).collect()
    }

    #[test]
    fn subsequence_required() {
        assert!(score("vvt", "#vivien-vault").is_some());
        assert!(score("xyz", "#vivien-vault").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn word_starts_and_prefixes_win() {
        assert_eq!(order("gen", &["#engineering", "#general", "#agenda"]), ["#general", "#agenda", "#engineering"]);
        assert_eq!(order("dl", &["#design-landing", "#deploys-log", "#dl"])[0], "#dl");
        assert_eq!(order("vv", &["#vivien-vault", "#dev-vv"])[0], "#vivien-vault");
    }

    #[test]
    fn case_insensitive_and_shorter_preferred() {
        assert_eq!(order("BOB", &["@bobby-tables", "@bob"])[0], "@bob");
    }

    #[test]
    fn best_needs_a_clear_winner() {
        let items = || ["#general", "#general-fr", "#random"].map(|s| (s.to_string(), s.to_string()));
        assert_eq!(best("rand", items()).as_deref(), Some("#random"));
        assert_eq!(best("gener", items()), None);
        assert_eq!(best("zzz", items()), None);
    }
}
