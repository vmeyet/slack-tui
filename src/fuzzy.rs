//! Subsequence matching the way fzf does it: every query char must appear in order,
//! word starts and runs of consecutive hits score higher, gaps cost.

const WORD_START: i64 = 10;
const CONSECUTIVE: i64 = 6;
const GAP: i64 = 1;

pub fn score(query: &str, candidate: &str) -> Option<i64> {
    score_chars(&lowercase_chars(query), candidate)
}

fn lowercase_chars(s: &str) -> Vec<char> {
    s.chars().flat_map(char::to_lowercase).collect()
}

fn score_chars(query: &[char], candidate: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = lowercase_chars(candidate);
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
pub fn rank<'a, T>(query: &str, items: impl IntoIterator<Item = (&'a str, T)>) -> Vec<(i64, T)> {
    let query = lowercase_chars(query);
    let mut scored: Vec<(i64, T)> = items.into_iter().filter_map(|(label, item)| score_chars(&query, label).map(|s| (s, item))).collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored
}

/// Levenshtein distance, for typos a subsequence match cannot see (`jion` → `join`).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Suggestions for a mistyped word: fuzzy matches first, then anything within two edits.
pub fn suggestions<'a>(query: &str, candidates: impl IntoIterator<Item = &'a str>, limit: usize) -> Vec<&'a str> {
    let candidates: Vec<&str> = candidates.into_iter().collect();
    let mut out: Vec<&str> = rank(query, candidates.iter().map(|c| (*c, *c))).into_iter().map(|(_, c)| c).collect();
    let mut close: Vec<(usize, &str)> = candidates
        .iter()
        .filter(|c| !out.contains(c))
        .map(|c| (edit_distance(&query.to_lowercase(), &c.to_lowercase()), *c))
        .filter(|(d, _)| *d <= 2)
        .collect();
    close.sort_by_key(|(d, _)| *d);
    out.extend(close.into_iter().map(|(_, c)| c));
    out.truncate(limit);
    out
}

/// The single obvious match, when one candidate clearly beats the rest.
pub fn best<'a, T>(query: &str, items: impl IntoIterator<Item = (&'a str, T)>) -> Option<T> {
    let mut ranked = rank(query, items).into_iter();
    let (top, item) = ranked.next()?;
    let runner_up = ranked.next().map(|(s, _)| s).unwrap_or(i64::MIN);
    (top > 0 && (runner_up <= 0 || top * 3 > runner_up * 4)).then_some(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order<'a>(query: &str, items: &[&'a str]) -> Vec<&'a str> {
        rank(query, items.iter().map(|i| (*i, *i))).into_iter().map(|(_, i)| i).collect()
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
    fn suggestions_cover_typos() {
        assert_eq!(edit_distance("jion", "join"), 2);
        assert_eq!(edit_distance("", "abc"), 3);
        let verbs = ["join", "leave", "go", "msg"];
        assert_eq!(suggestions("jion", verbs, 3), vec!["join"]);
        assert_eq!(suggestions("lev", verbs, 3), vec!["leave"]);
        assert!(suggestions("zzzzzz", verbs, 3).is_empty());
    }

    #[test]
    fn best_needs_a_clear_winner() {
        let items = || ["#general", "#general-fr", "#random"].map(|s| (s, s));
        assert_eq!(best("rand", items()), Some("#random"));
        assert_eq!(best("gener", items()), None);
        assert_eq!(best("zzz", items()), None);
    }
}
