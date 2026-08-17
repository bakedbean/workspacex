//! Memoised "does this repo live on github.com?" answers.
//!
//! The dashboard gates its per-repo PR affordance on this, so the question
//! gets asked once per visible repo per frame — but answering it costs a
//! `git remote get-url` subprocess. This cache moves that cost to
//! `App::refresh`, where it runs once per repo for the life of the process.

use crate::data::store::{Repo, RepoId};
use std::collections::HashMap;
use std::path::Path;

/// Per-repo GitHub-remote answers, probed lazily and kept until the repo
/// is unregistered. A repo's remote can change under us, but only via a
/// `git remote set-url` the user runs outside wsx; re-probing every refresh
/// to catch that would cost a subprocess per repo per data change, which
/// isn't worth it for an affordance a restart fixes.
#[derive(Debug, Default)]
pub struct GithubRemotes {
    known: HashMap<RepoId, bool>,
}

impl GithubRemotes {
    /// Probe every repo in `repos` that hasn't been probed yet, and forget
    /// repos that are no longer registered.
    pub fn sync(&mut self, repos: &[Repo]) {
        self.sync_with(repos, super::forge::repo_has_github_remote);
    }

    /// `sync` with the probe injected, so tests can drive it without git.
    fn sync_with(&mut self, repos: &[Repo], probe: impl Fn(&Path) -> bool) {
        self.known.retain(|id, _| repos.iter().any(|r| r.id == *id));
        for repo in repos {
            self.known
                .entry(repo.id)
                .or_insert_with(|| probe(&repo.path));
        }
    }

    /// Whether `id`'s origin points at github.com. Unprobed repos read as
    /// `false` so a caller that runs before the first `sync` hides the
    /// affordance rather than offering a dead one.
    pub fn is_github(&self, id: RepoId) -> bool {
        self.known.get(&id).copied().unwrap_or(false)
    }

    /// A cache with `answers` already filled in, for tests elsewhere in the
    /// crate that need a known set of GitHub repos without touching git.
    #[cfg(test)]
    pub(crate) fn probed(answers: impl IntoIterator<Item = (RepoId, bool)>) -> Self {
        Self {
            known: answers.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn repo(id: i64, path: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: format!("repo{id}"),
            path: PathBuf::from(path),
            branch_prefix: "x".into(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
            sort_order: id,
        }
    }

    /// A probe that records every path it was asked about and answers by
    /// path prefix.
    fn recording_probe(seen: &RefCell<Vec<PathBuf>>) -> impl Fn(&Path) -> bool + '_ {
        move |p: &Path| {
            seen.borrow_mut().push(p.to_path_buf());
            p.starts_with("/gh")
        }
    }

    #[test]
    fn records_each_repos_probe_result() {
        let seen = RefCell::new(Vec::new());
        let repos = [repo(1, "/gh/a"), repo(2, "/gitlab/b")];
        let mut cache = GithubRemotes::default();
        cache.sync_with(&repos, recording_probe(&seen));

        assert!(cache.is_github(RepoId(1)));
        assert!(!cache.is_github(RepoId(2)));
    }

    #[test]
    fn probes_each_repo_only_once_across_syncs() {
        let seen = RefCell::new(Vec::new());
        let repos = [repo(1, "/gh/a")];
        let mut cache = GithubRemotes::default();
        cache.sync_with(&repos, recording_probe(&seen));
        cache.sync_with(&repos, recording_probe(&seen));
        cache.sync_with(&repos, recording_probe(&seen));

        assert_eq!(
            seen.borrow().len(),
            1,
            "refresh runs on every data change; the subprocess must not"
        );
        assert!(cache.is_github(RepoId(1)));
    }

    #[test]
    fn probes_a_newly_registered_repo() {
        let seen = RefCell::new(Vec::new());
        let mut cache = GithubRemotes::default();
        cache.sync_with(&[repo(1, "/gh/a")], recording_probe(&seen));
        cache.sync_with(
            &[repo(1, "/gh/a"), repo(2, "/gh/b")],
            recording_probe(&seen),
        );

        assert_eq!(
            *seen.borrow(),
            [PathBuf::from("/gh/a"), PathBuf::from("/gh/b")]
        );
        assert!(cache.is_github(RepoId(2)));
    }

    #[test]
    fn forgets_an_unregistered_repo() {
        let seen = RefCell::new(Vec::new());
        let mut cache = GithubRemotes::default();
        cache.sync_with(
            &[repo(1, "/gh/a"), repo(2, "/gh/b")],
            recording_probe(&seen),
        );
        cache.sync_with(&[repo(1, "/gh/a")], recording_probe(&seen));

        assert!(!cache.is_github(RepoId(2)), "dropped with the repo");
    }

    #[test]
    fn an_unprobed_repo_is_not_github() {
        let cache = GithubRemotes::default();
        assert!(!cache.is_github(RepoId(7)));
    }
}
