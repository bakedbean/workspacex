//! Related repos: parser, resolver, and read-only system-prompt builder
//! for the per-repo `related_repos` config.

use crate::store::Repo;
use std::path::PathBuf;

/// Parse a `related_repos` config value into trimmed, non-empty name strings.
/// Comma-separated; whitespace around commas trimmed; blank entries dropped.
pub fn parse(spec: &str) -> Vec<String> {
    spec.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Resolve each name in `spec` to its (name, source_path) by looking up
/// `all_repos`. Names with no matching repo are tracing::warn!'d and dropped.
/// Returns entries in input order.
pub fn resolve(spec: Option<&str>, all_repos: &[Repo]) -> Vec<(String, PathBuf)> {
    let Some(s) = spec else { return Vec::new() };
    let names = parse(s);
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        match all_repos.iter().find(|r| r.name == name) {
            Some(r) => out.push((name, r.path.clone())),
            None => tracing::warn!(name = %name, "related_repos: unknown repo name; skipping"),
        }
    }
    out
}

/// Build the read-only system-prompt fragment claude sees when related
/// repos are present. Returns None when `resolved` is empty.
pub fn build_read_only_prompt(resolved: &[(String, PathBuf)]) -> Option<String> {
    if resolved.is_empty() {
        return None;
    }
    let mut listing = String::new();
    for (name, path) in resolved {
        listing.push_str(&format!("  - {} (wsx repo: {})\n", path.display(), name));
    }
    Some(format!(
        "The following directories were added via --add-dir for read-only \
         reference. They are the source paths of related wsx-managed repos:\n\
         {listing}\n\
         You MUST NOT edit files in these directories. They sit on whatever \
         branch the source repo's main worktree happens to be on; a write \
         there would land outside your workspace and could clobber other \
         active work.\n\n\
         When a task requires changes in a related repo, drive wsx from \
         this session to spin up a sibling workspace, then work in it:\n\n\
         \x20 1. `wsx workspace create <repo> --name <slug>` — create a \
         workspace in the related repo. `<slug>` is a 2-4 word kebab-case \
         summary of the task (e.g. `add-widgets-endpoint`); wsx applies \
         that repo's configured branch_prefix, so the resulting branch is \
         `<prefix>/<slug>`. Do NOT pass a full branch name as the slug.\n\
         \x20 2. `wsx workspace path <repo> <slug>` — prints the new \
         worktree path. `cd` there to make changes, commit, and push.\n\
         \x20 3. Each repo gets its own branch and its own PR. To \
         coordinate \"ship together\", cross-link the PRs in each \
         description and ask the user to merge in dependency order.\n\n\
         Workspaces in different repos do not share Claude session state. \
         If you split work across sessions, propagate API contracts and \
         decisions via commits or PR bodies, not by assuming the other \
         session remembers.\n\n\
         Other useful commands: `wsx workspace list [<repo>]`, \
         `wsx workspace rename <repo> <old-slug> <new-slug>`, \
         `wsx workspace archive <repo> <slug>`.\n\n\
         Read, grep, and quote freely from these read-only paths. Just \
         don't write to them.\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Repo, RepoId};

    fn repo(id: i64, name: &str, path: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: name.into(),
            path: PathBuf::from(path),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            created_at: 0,
        }
    }

    #[test]
    fn parse_single_name() {
        assert_eq!(parse("frontend"), vec!["frontend".to_string()]);
    }

    #[test]
    fn parse_comma_separated_with_whitespace() {
        assert_eq!(
            parse(" frontend , marketing,backend "),
            vec![
                "frontend".to_string(),
                "marketing".to_string(),
                "backend".to_string()
            ]
        );
    }

    #[test]
    fn parse_skips_blank_entries() {
        assert_eq!(
            parse("frontend,,marketing,"),
            vec!["frontend".to_string(), "marketing".to_string()]
        );
    }

    #[test]
    fn parse_empty_string_returns_empty() {
        assert!(parse("").is_empty());
        assert!(parse("   ").is_empty());
        assert!(parse(",,, ,").is_empty());
    }

    #[test]
    fn resolve_returns_matching_repos_in_input_order() {
        let repos = vec![
            repo(1, "frontend", "/work/frontend"),
            repo(2, "backend", "/work/backend"),
            repo(3, "marketing", "/work/marketing"),
        ];
        let out = resolve(Some("marketing, frontend"), &repos);
        assert_eq!(
            out,
            vec![
                ("marketing".to_string(), PathBuf::from("/work/marketing")),
                ("frontend".to_string(), PathBuf::from("/work/frontend")),
            ]
        );
    }

    #[test]
    fn resolve_drops_unknown_names() {
        let repos = vec![repo(1, "frontend", "/work/frontend")];
        let out = resolve(Some("frontend, ghost"), &repos);
        assert_eq!(
            out,
            vec![("frontend".to_string(), PathBuf::from("/work/frontend"))]
        );
    }

    #[test]
    fn resolve_none_returns_empty() {
        let repos = vec![repo(1, "frontend", "/work/frontend")];
        assert!(resolve(None, &repos).is_empty());
    }

    #[test]
    fn resolve_empty_spec_returns_empty() {
        let repos = vec![repo(1, "frontend", "/work/frontend")];
        assert!(resolve(Some(""), &repos).is_empty());
        assert!(resolve(Some("   "), &repos).is_empty());
    }

    #[test]
    fn build_read_only_prompt_empty_returns_none() {
        assert!(build_read_only_prompt(&[]).is_none());
    }

    #[test]
    fn build_read_only_prompt_single_entry_lists_it() {
        let r = vec![("frontend".to_string(), PathBuf::from("/work/frontend"))];
        let out = build_read_only_prompt(&r).unwrap();
        assert!(out.contains("/work/frontend"), "prompt missing path: {out}");
        assert!(
            out.contains("wsx repo: frontend"),
            "prompt missing label: {out}"
        );
        assert!(
            out.contains("MUST NOT edit"),
            "prompt missing read-only directive: {out}"
        );
    }

    #[test]
    fn build_read_only_prompt_includes_orchestration_commands() {
        let r = vec![("frontend".to_string(), PathBuf::from("/work/frontend"))];
        let out = build_read_only_prompt(&r).unwrap();
        assert!(
            out.contains("wsx workspace create"),
            "prompt missing workspace create command: {out}"
        );
        assert!(
            out.contains("wsx workspace path"),
            "prompt missing workspace path command: {out}"
        );
        assert!(
            out.contains("branch_prefix"),
            "prompt missing branch_prefix explanation: {out}"
        );
        assert!(
            out.contains("Do NOT pass a full branch name"),
            "prompt missing slug-vs-branch warning: {out}"
        );
    }

    #[test]
    fn build_read_only_prompt_multiple_entries_lists_all() {
        let r = vec![
            ("frontend".to_string(), PathBuf::from("/work/frontend")),
            ("marketing".to_string(), PathBuf::from("/work/marketing")),
        ];
        let out = build_read_only_prompt(&r).unwrap();
        assert!(out.contains("/work/frontend"));
        assert!(out.contains("/work/marketing"));
        assert!(out.contains("wsx repo: frontend"));
        assert!(out.contains("wsx repo: marketing"));
    }
}
