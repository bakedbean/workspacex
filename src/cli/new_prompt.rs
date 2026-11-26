//! The questions `wsx new` asks when no repo is given, and its defaults.

use crate::data::store::{Repo, Store, Workspace};
use crate::error::{Error, Result};
use crate::pty::session::AgentKind;
use std::io::{BufRead, Write};

#[derive(Debug, PartialEq)]
pub(crate) struct Answers {
    pub repo: String,
    pub task: Option<String>,
}

/// Registered repos, the one with the newest workspace first; repos without
/// workspaces keep dashboard order at the end.
pub(crate) fn repos_by_recency(store: &Store) -> Result<Vec<Repo>> {
    let mut keyed = Vec::new();
    for repo in crate::data::repo::list(store)? {
        let newest = store
            .workspaces(repo.id)?
            .iter()
            .map(|w| w.created_at)
            .max();
        keyed.push((newest, repo));
    }
    keyed.sort_by_key(|(newest, _)| std::cmp::Reverse(*newest));
    Ok(keyed.into_iter().map(|(_, r)| r).collect())
}

/// One trimmed answer; end of input means the user backed out.
fn answer(label: &str, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<String> {
    write!(out, "{label}")?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(Error::Cancelled);
    }
    Ok(line.trim().to_string())
}

pub(crate) fn ask(repos: &[Repo], input: &mut dyn BufRead, out: &mut dyn Write) -> Result<Answers> {
    let repo = match repos {
        [] => {
            return Err(Error::UserInput(
                "no repos registered; add one with: wsx repo add <path>".into(),
            ));
        }
        [only] => only,
        _ => {
            let menu: Vec<String> = repos
                .iter()
                .enumerate()
                .map(|(i, r)| format!("{} {}", i + 1, r.name))
                .collect();
            writeln!(out, " {}", menu.join("   "))?;
            loop {
                let a = answer("repo> ", input, out)?;
                let picked = if a.is_empty() {
                    repos.first()
                } else {
                    // A name wins over a position, for a repo named "2".
                    repos.iter().find(|r| r.name == a).or_else(|| {
                        let n: usize = a.parse().ok()?;
                        repos.get(n.checked_sub(1)?)
                    })
                };
                if let Some(r) = picked {
                    break r;
                }
            }
        }
    };
    let task = answer("task (optional)> ", input, out)?;
    Ok(Answers {
        repo: repo.name.clone(),
        task: (!task.is_empty()).then_some(task),
    })
}

/// The repo and task, asking only when no repo was given.
pub(crate) fn resolve(
    store: &Store,
    repo: Option<String>,
    task: Option<String>,
    interactive: bool,
) -> Result<Answers> {
    if let Some(repo) = repo {
        return Ok(Answers { repo, task });
    }
    if !interactive {
        return Err(Error::Usage {
            group: Some("new"),
            msg: "give a repo: there is no terminal to ask on".into(),
        });
    }
    ask(
        &repos_by_recency(store)?,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
    )
}

/// Yolo and agent kind: yolo unless `--no-yolo` or a restricted parent
/// workspace says otherwise; the agent as in `workspace create`.
pub(crate) fn create_flags(
    no_yolo: bool,
    agent: Option<&str>,
    parent: Option<&Workspace>,
    default_agent: AgentKind,
) -> (bool, AgentKind) {
    let yolo = !no_yolo && parent.is_none_or(|p| p.yolo);
    let (_, kind) = super::resolve::effective_create_flags(false, agent, parent, default_agent);
    (yolo, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::NewWorkspace;

    fn add_ws(
        store: &Store,
        repo: crate::data::store::RepoId,
        name: &str,
        yolo: bool,
    ) -> Workspace {
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name,
                branch: name,
                worktree_path: std::path::Path::new("/tmp/nowhere").join(name).as_path(),
                yolo,
                agent: AgentKind::Codex,
                shared: false,
            })
            .unwrap();
        store.workspace_by_id(id).unwrap().unwrap()
    }

    fn store_abc() -> Store {
        let store = Store::open_in_memory().unwrap();
        for (name, at) in [("a", Some(10)), ("b", Some(30)), ("c", None)] {
            let id = store
                .add_repo(&std::path::Path::new("/tmp").join(name), name, "")
                .unwrap();
            if let Some(at) = at {
                let w = add_ws(&store, id, name, false);
                store
                    .conn()
                    .execute(
                        "UPDATE workspaces SET created_at = ?1 WHERE id = ?2",
                        [at, w.id.0],
                    )
                    .unwrap();
            }
        }
        store
    }

    fn run(store: &Store, input: &str) -> (Result<Answers>, String) {
        let repos = repos_by_recency(store).unwrap();
        let mut out = Vec::new();
        let r = ask(&repos, &mut input.as_bytes(), &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    fn ans(repo: &str, task: Option<&str>) -> Answers {
        Answers {
            repo: repo.into(),
            task: task.map(Into::into),
        }
    }

    #[test]
    fn repos_come_most_recently_used_first() {
        let names: Vec<_> = repos_by_recency(&store_abc())
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["b", "a", "c"]);
    }

    #[test]
    fn picks_by_number_name_or_enter() {
        let s = store_abc();
        assert_eq!(run(&s, "2\n\n").0.unwrap(), ans("a", None));
        assert_eq!(run(&s, "\nfix it\n").0.unwrap(), ans("b", Some("fix it")));
        assert_eq!(run(&s, " c \n  \n").0.unwrap(), ans("c", None));
    }

    #[test]
    fn asks_again_for_an_unlisted_repo() {
        let (r, out) = run(&store_abc(), "9\nnope\n0\nb\n\n");
        assert_eq!(r.unwrap(), ans("b", None));
        assert_eq!(out.matches("repo> ").count(), 4, "{out}");
    }

    #[test]
    fn one_repo_skips_the_repo_question() {
        let store = Store::open_in_memory().unwrap();
        store
            .add_repo(std::path::Path::new("/tmp/o"), "only", "")
            .unwrap();
        let (r, out) = run(&store, "go\n");
        assert_eq!(r.unwrap(), ans("only", Some("go")));
        assert!(!out.contains("repo> "));
    }

    #[test]
    fn no_repos_points_at_repo_add() {
        let store = Store::open_in_memory().unwrap();
        let e = run(&store, "").0.unwrap_err().to_string();
        assert!(e.contains("wsx repo add"), "{e}");
    }

    #[test]
    fn end_of_input_cancels_at_either_question() {
        let s = store_abc();
        assert!(matches!(run(&s, "").0, Err(Error::Cancelled)));
        assert!(matches!(run(&s, "1\n").0, Err(Error::Cancelled)));
    }

    #[test]
    fn yolo_unless_restricted_by_flag_or_parent() {
        let store = store_abc();
        let repo = crate::data::repo::list(&store).unwrap()[0].id;
        let yolo = add_ws(&store, repo, "y", true);
        let restricted = add_ws(&store, repo, "n", false);
        let f = |no: bool, p: Option<&Workspace>| create_flags(no, None, p, AgentKind::Claude).0;
        assert!(f(false, None));
        assert!(f(false, Some(&yolo)));
        assert!(!f(false, Some(&restricted)));
        for p in [None, Some(&yolo), Some(&restricted)] {
            assert!(!f(true, p));
        }
    }

    #[test]
    fn agent_flag_beats_parent_beats_default() {
        let store = store_abc();
        let repo = crate::data::repo::list(&store).unwrap()[0].id;
        let p = add_ws(&store, repo, "p", true);
        let f = |a, p| create_flags(false, a, p, AgentKind::Claude).1;
        assert_eq!(f(Some("pi"), Some(&p)), AgentKind::Pi);
        assert_eq!(f(None, Some(&p)), AgentKind::Codex);
        assert_eq!(f(None, None), AgentKind::Claude);
    }

    #[test]
    fn a_given_repo_asks_nothing_and_no_terminal_is_refused() {
        let s = store_abc();
        let got = resolve(&s, Some("a".into()), Some("t".into()), false).unwrap();
        assert_eq!(got, ans("a", Some("t")));
        assert!(matches!(
            resolve(&s, None, None, false),
            Err(Error::Usage { .. })
        ));
    }
}
