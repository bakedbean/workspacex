use std::path::Path;

use serde::Serialize;

use crate::data::store::{ReportedState, Store};
use crate::error::Result;

#[derive(Serialize, Debug, PartialEq)]
pub struct StatusPayload {
    pub text: String,
    pub class: String,
    pub tooltip: String,
}

fn rank(state: ReportedState) -> u8 {
    match state {
        ReportedState::Blocked => 4,
        ReportedState::Done => 3,
        ReportedState::Waiting => 2,
        ReportedState::Working | ReportedState::Busy => 1,
    }
}

fn class_name(state: ReportedState) -> &'static str {
    match state {
        ReportedState::Blocked => "blocked",
        ReportedState::Done => "done",
        ReportedState::Waiting => "waiting",
        ReportedState::Working | ReportedState::Busy => "working",
    }
}

fn glyph(state: Option<ReportedState>) -> &'static str {
    match state {
        Some(ReportedState::Blocked) => "!",
        Some(ReportedState::Done) => "\u{2713}",
        Some(ReportedState::Waiting) => "\u{2026}",
        Some(ReportedState::Working | ReportedState::Busy) => "\u{21bb}",
        None => "\u{b7}",
    }
}

pub(crate) fn escape_pango(s: &str) -> String {
    let escaped = s
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    // Control characters (incl. '\n', '\t') would otherwise let a status
    // message inject extra tooltip lines; collapse them to a single space.
    escaped
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

pub fn status_payload(store: &Store) -> Result<StatusPayload> {
    let repos = crate::data::repo::list(store)?;
    if repos.is_empty() {
        return Ok(StatusPayload {
            text: String::new(),
            class: "idle".into(),
            tooltip: String::new(),
        });
    }
    let statuses = store.all_workspace_status()?;
    let mut count = 0usize;
    let mut best: Option<ReportedState> = None;
    let mut lines = Vec::new();
    for repo in &repos {
        lines.push(escape_pango(&repo.name));
        let workspaces = store.workspaces(repo.id)?;
        if workspaces.is_empty() {
            lines.push("  (no workspaces)".into());
        }
        for ws in &workspaces {
            count += 1;
            let st = statuses.get(&ws.id);
            if let Some(st) = st {
                if best.is_none_or(|b| rank(st.state) > rank(b)) {
                    best = Some(st.state);
                }
            }
            let mut line = format!(
                "  {} {}",
                glyph(st.map(|s| s.state)),
                escape_pango(&ws.name)
            );
            if let Some(msg) = st.and_then(|s| s.message.as_deref()) {
                line.push_str(" \u{2014} ");
                line.push_str(&escape_pango(msg));
            }
            lines.push(line);
        }
    }
    Ok(StatusPayload {
        text: format!("\u{e725} {count}"), // nf-dev-git_branch
        class: best.map(class_name).unwrap_or("idle").to_string(),
        tooltip: lines.join("\n"),
    })
}

/// Never fails: waybar runs this every 5s; on any error emit an empty payload
/// so the module hides instead of flashing errors in the bar.
pub fn print_status(db_path: &Path) {
    let json = Store::open(db_path)
        .and_then(|store| status_payload(&store))
        .and_then(|p| Ok(serde_json::to_string(&p)?));
    match json {
        Ok(j) => println!("{j}"),
        Err(_) => println!(r#"{{"text":""}}"#),
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, ReportedState, Store, WorkspaceState};
    use crate::pty::session::AgentKind;

    fn seed() -> Store {
        let store = Store::open_in_memory().unwrap();
        let r1 = store
            .add_repo(std::path::Path::new("/tmp/alpha"), "alpha", "feat")
            .unwrap();
        let _r2 = store
            .add_repo(std::path::Path::new("/tmp/empty"), "empty", "feat")
            .unwrap();
        for name in ["one", "two"] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id: r1,
                    name,
                    branch: &format!("feat/{name}"),
                    worktree_path: &std::path::PathBuf::from(format!("/tmp/wt-{name}")),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
        }
        store
    }

    #[test]
    fn counts_workspaces_and_defaults_to_idle() {
        let store = seed();
        let p = status_payload(&store).unwrap();
        assert!(p.text.ends_with(" 2"), "text was {:?}", p.text);
        assert_eq!(p.class, "idle");
        assert!(p.tooltip.contains("alpha"));
        assert!(p.tooltip.contains("one"));
        assert!(p.tooltip.contains("empty")); // repos with no workspaces still listed
        assert!(p.tooltip.contains("(no workspaces)"));
    }

    #[test]
    fn class_uses_priority_blocked_over_done_over_waiting_over_working() {
        let store = seed();
        let ws = store.all_workspaces().unwrap();
        store
            .set_workspace_status(ws[0].id, ReportedState::Working, Some("hacking"), "model")
            .unwrap();
        assert_eq!(status_payload(&store).unwrap().class, "working");
        store
            .set_workspace_status(ws[1].id, ReportedState::Waiting, None, "model")
            .unwrap();
        assert_eq!(status_payload(&store).unwrap().class, "waiting");
        store
            .set_workspace_status(ws[0].id, ReportedState::Done, None, "model")
            .unwrap();
        assert_eq!(status_payload(&store).unwrap().class, "done");
        store
            .set_workspace_status(
                ws[1].id,
                ReportedState::Blocked,
                Some("need input"),
                "model",
            )
            .unwrap();
        let p = status_payload(&store).unwrap();
        assert_eq!(p.class, "blocked");
        assert!(p.tooltip.contains("need input"));
    }

    #[test]
    fn busy_maps_to_working_and_pango_is_escaped() {
        let store = seed();
        let ws = store.all_workspaces().unwrap();
        store
            .set_workspace_status(ws[0].id, ReportedState::Busy, Some("a <b> & c"), "hook")
            .unwrap();
        let p = status_payload(&store).unwrap();
        assert_eq!(p.class, "working");
        assert!(p.tooltip.contains("a &lt;b&gt; &amp; c"));
        assert!(!p.tooltip.contains("<b>"));
    }

    #[test]
    fn embedded_newline_in_message_cannot_inject_tooltip_lines() {
        let store = seed();
        let ws = store.all_workspaces().unwrap();
        // Baseline: alpha, one, two, empty, (no workspaces) = 5 lines.
        let baseline_lines = status_payload(&store).unwrap().tooltip.lines().count();
        store
            .set_workspace_status(ws[0].id, ReportedState::Working, Some("a\nb"), "hook")
            .unwrap();
        let p = status_payload(&store).unwrap();
        assert!(p.tooltip.contains("a b"), "tooltip was {:?}", p.tooltip);
        assert!(!p.tooltip.contains("a\nb"), "tooltip was {:?}", p.tooltip);
        assert_eq!(
            baseline_lines,
            p.tooltip.lines().count(),
            "embedded newline must not add a tooltip line; tooltip was {:?}",
            p.tooltip
        );
    }

    #[test]
    fn no_repos_hides_module() {
        let store = Store::open_in_memory().unwrap();
        let p = status_payload(&store).unwrap();
        assert_eq!(p.text, "");
    }

    #[test]
    fn json_shape() {
        let store = seed();
        let json = serde_json::to_string(&status_payload(&store).unwrap()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("text").is_some() && v.get("class").is_some() && v.get("tooltip").is_some());
    }
}
