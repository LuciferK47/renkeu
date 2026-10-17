//! Fuzzy search over a candidate list, backed by `nucleo-matcher` (Helix's
//! matcher). Replaces the frontend `fuse.js` (masterplan.md §10.1). Pure and
//! stateless — the route layer supplies the candidates.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

/// One ranked match. Higher `score` = better.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Hit {
    pub value: String,
    pub score: u32,
}

/// Rank `candidates` against `query`, best first.
///
/// An empty query returns every candidate in input order (score 0) so callers
/// can show the full list before the user types. `Config::match_paths` biases
/// toward path-like strings (file trees, command ids).
#[must_use]
pub fn fuzzy_rank(query: &str, candidates: &[String]) -> Vec<Hit> {
    if query.is_empty() {
        return candidates
            .iter()
            .map(|c| Hit {
                value: c.clone(),
                score: 0,
            })
            .collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart)
        .match_list(candidates, &mut matcher)
        .into_iter()
        .map(|(value, score)| Hit {
            value: value.clone(),
            score,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands() -> Vec<String> {
        ["src/main.rs", "src/lib.rs", "README.md", "Cargo.toml"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn ranks_matches_and_drops_non_matches() {
        let hits = fuzzy_rank("mainrs", &cands());
        assert_eq!(hits.first().map(|h| h.value.as_str()), Some("src/main.rs"));
        assert!(
            hits.iter().all(|h| h.value != "README.md"),
            "non-match leaked in"
        );
    }

    #[test]
    fn empty_query_returns_all_in_order() {
        let hits = fuzzy_rank("", &cands());
        assert_eq!(hits.len(), 4);
        assert_eq!(hits[0].value, "src/main.rs");
    }

    #[test]
    fn no_match_returns_empty() {
        assert_eq!(fuzzy_rank("zzzzz", &cands()).len(), 0);
    }
}
