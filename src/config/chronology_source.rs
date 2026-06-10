//! Adapter implementing the crate's `ConfigSource` over wsx's `Store`/`Repo`.
use crate::chronology::ConfigSource;
use crate::data::store::{Repo, Store};

pub struct StoreConfigSource<'a> {
    pub(crate) store: &'a Store,
    pub(crate) repo: Option<&'a Repo>,
}

impl ConfigSource for StoreConfigSource<'_> {
    fn global_json(&self) -> Option<String> {
        self.store.get_setting("chronology_config").ok().flatten()
    }
    fn repo_override_json(&self) -> Option<String> {
        self.repo.and_then(|r| r.chronology_config.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{RepoId, Store};
    use std::path::PathBuf;

    fn test_repo(chronology_config: Option<&str>) -> Repo {
        Repo {
            id: RepoId(1),
            name: "demo".into(),
            path: PathBuf::from("/r"),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            chronology_config: chronology_config.map(|s| s.to_string()),
            created_at: 0,
            sort_order: 0,
        }
    }

    #[test]
    fn resolve_applies_repo_override_side_left() {
        let store = Store::open_in_memory().unwrap();
        let repo = test_repo(Some(r#"{"side":"left"}"#));
        let src = StoreConfigSource {
            store: &store,
            repo: Some(&repo),
        };
        let cfg = crate::chronology::resolve(&src);
        assert_eq!(cfg.side, crate::chronology::Side::Left);
    }
}
