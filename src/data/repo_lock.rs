//! Per-repository serialization of git operations.
//!
//! `git worktree add` and `git worktree remove` both mutate `.git/worktrees/`
//! and repo-level refs, so two of them racing in the same repo can corrupt
//! that admin state. Before backgrounding this could not happen — the create
//! and archive modals made concurrent operations impossible. Now three
//! creates can start within a few seconds, so the git phases take a lock.
//!
//! Scope is deliberately narrow: only the git calls are guarded. Setup
//! scripts stay fully parallel — they are the slow part (~18s on ssk-web)
//! and each touches only its own worktree.
//!
//! Process-local by design. A `wsx workspace create` CLI invocation is a
//! separate process and is not covered; git's own locking is the backstop
//! there, and the CLI creates one workspace at a time regardless.

use crate::data::store::RepoId;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

static LOCKS: LazyLock<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The lock for `id`, creating it on first use. Entries are never evicted:
/// one empty mutex per repo ever registered is negligible, and eviction
/// would risk handing out a fresh lock while another task holds the old one.
pub fn for_repo(id: RepoId) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = LOCKS.lock().unwrap();
    locks
        .entry(id.0)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::RepoId;

    #[test]
    fn same_repo_shares_one_lock() {
        let a = for_repo(RepoId(7));
        let b = for_repo(RepoId(7));
        assert!(
            std::sync::Arc::ptr_eq(&a, &b),
            "same repo must share a lock"
        );
    }

    #[test]
    fn different_repos_get_different_locks() {
        let a = for_repo(RepoId(101));
        let b = for_repo(RepoId(102));
        assert!(
            !std::sync::Arc::ptr_eq(&a, &b),
            "unrelated repos must not serialize against each other"
        );
    }

    #[tokio::test]
    async fn lock_is_mutually_exclusive() {
        let l = for_repo(RepoId(303));
        let held = l.lock().await;
        assert!(l.try_lock().is_err(), "second acquisition must block");
        drop(held);
        assert!(l.try_lock().is_ok());
    }
}
