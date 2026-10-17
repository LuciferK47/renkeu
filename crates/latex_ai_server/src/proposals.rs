//! Proposals the server is holding for review.
//!
//! A verified agent run ends with a proposal the user may accept in part.
//! Remembering the **base** it was made against (each file's content and
//! `ETag` when the run started) is what lets the apply step tell a proposal
//! that is stale from one that merely sits beside unrelated edits, and move
//! the latter onto the current text (`core::apply::rebase_edits`).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Proposals kept; the oldest go first.
const MAX_PROPOSALS: usize = 32;
/// A proposal nobody acted on for this long is forgotten.
const TTL: Duration = Duration::from_secs(60 * 60);

/// A file as the proposal's run found it.
#[derive(Debug, Clone)]
pub struct BaseFile {
    pub content: String,
    pub etag: String,
}

#[derive(Debug)]
pub struct Proposal {
    pub id: String,
    pub instruction: String,
    /// Base of every file the proposal touches, by project-relative path.
    pub base: BTreeMap<String, BaseFile>,
    created: Instant,
}

#[derive(Default)]
pub struct ProposalRegistry {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    by_id: HashMap<String, Arc<Proposal>>,
    order: VecDeque<String>,
}

impl ProposalRegistry {
    /// Remember a proposal; returns its id.
    pub fn insert(&self, instruction: String, base: BTreeMap<String, BaseFile>) -> String {
        let id = latex_ai_core::fsutil::unique_id();
        let proposal = Arc::new(Proposal {
            id: id.clone(),
            instruction,
            base,
            created: Instant::now(),
        });
        let mut inner = self.lock();
        inner.prune();
        while inner.order.len() >= MAX_PROPOSALS {
            if let Some(oldest) = inner.order.pop_front() {
                inner.by_id.remove(&oldest);
            }
        }
        inner.by_id.insert(id.clone(), proposal);
        inner.order.push_back(id.clone());
        id
    }

    /// The proposal, if it is still held and has not expired.
    pub fn get(&self, id: &str) -> Option<Arc<Proposal>> {
        let mut inner = self.lock();
        inner.prune();
        inner.by_id.get(id).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl Inner {
    fn prune(&mut self) {
        let expired: Vec<String> = self
            .by_id
            .iter()
            .filter(|(_, p)| p.created.elapsed() > TTL)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.by_id.remove(&id);
            self.order.retain(|o| *o != id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(path: &str, content: &str) -> BTreeMap<String, BaseFile> {
        BTreeMap::from([(
            path.to_owned(),
            BaseFile {
                content: content.to_owned(),
                etag: latex_ai_core::storage::etag_of(content),
            },
        )])
    }

    #[test]
    fn a_stored_proposal_is_found_by_id_with_its_base() {
        let registry = ProposalRegistry::default();
        let id = registry.insert("fix".into(), base("main.tex", "x\n"));
        let p = registry.get(&id).unwrap();
        assert_eq!(p.instruction, "fix");
        assert_eq!(p.base["main.tex"].content, "x\n");
        assert!(registry.get("nope").is_none());
    }

    #[test]
    fn the_oldest_proposals_are_dropped_past_the_cap() {
        let registry = ProposalRegistry::default();
        let first = registry.insert("0".into(), base("a.tex", "0"));
        let mut last = String::new();
        for i in 1..=MAX_PROPOSALS {
            last = registry.insert(i.to_string(), base("a.tex", "x"));
        }
        assert!(registry.get(&first).is_none(), "oldest should be gone");
        assert!(registry.get(&last).is_some());
    }

    #[test]
    fn ids_are_unguessable_and_distinct() {
        let registry = ProposalRegistry::default();
        let a = registry.insert("a".into(), BTreeMap::new());
        let b = registry.insert("b".into(), BTreeMap::new());
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }
}
