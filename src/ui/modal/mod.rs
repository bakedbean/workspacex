use crate::config::usage_window::UsageWindow;
use crate::data::store::RepoId;
use crate::git::forge::BranchLifecycle;
use crate::ui::dashboard::status::Status;
use crate::ui::drawer::Hint;
use crate::ui::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::collections::{HashMap, HashSet};

mod agents_panel;
mod name_color_picker;
mod process_list;
mod prompt_tag;
mod remote_workspace_list;
mod repo_settings;
mod setup_log;
mod textarea;
mod updates_panel;
mod usage_picker;
pub mod workspace_tags;

// Panel renderers called from app::render via `crate::ui::modal::*`.
pub use agents_panel::render_agents_panel;
pub use name_color_picker::{Dir, move_selection, render_name_color_picker};
pub use process_list::render_process_list;
pub use prompt_tag::{EnterAction, PromptTagModal, TagStage, render_prompt_tag};
pub use remote_workspace_list::render_remote_workspace_list;
pub use repo_settings::render_repo_settings;
pub use setup_log::{SetupLogView, StoredLog, render_setup_log};
pub use textarea::TextArea;
pub use updates_panel::{
    PANEL_MAX_WIDTH, PanelInputs, PanelView, ordered_workspaces_for_panel, panel_sections,
    render_updates_panel,
};
pub use usage_picker::render_usage_window_picker;

/// Whose tags the `#` editor writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagTarget {
    Local(crate::data::store::WorkspaceId),
    /// A workspace on a shared host, written over ssh while attached to it.
    Remote {
        dest: String,
        repo: String,
        workspace: String,
    },
}

#[derive(Debug, Clone)]
pub enum Modal {
    NewWorkspace {
        repo_id: RepoId,
        name_buffer: String,
        /// Tags to apply at creation, separated by spaces or commas;
        /// validated on Enter.
        tags_buffer: String,
        /// Typing goes to `tags_buffer` instead of `name_buffer`; ↑/↓ flips it.
        tags_focused: bool,
        yolo: bool,
        shared: bool,
        agent: crate::pty::session::AgentKind,
        /// Inline error line (e.g. a duplicate name), cleared on next edit.
        /// Mirrors `RenameWorkspace`'s `notice` field.
        notice: Option<String>,
    },
    ConfirmArchive {
        workspace_id: crate::data::store::WorkspaceId,
        name: String,
    },
    ConfirmShare {
        workspace_id: crate::data::store::WorkspaceId,
        name: String,
        /// `true` = converting to tmux-shared, `false` = converting to direct.
        to_shared: bool,
        /// Snapshot of how many instances currently have a running session,
        /// taken when the modal was opened (by `T`'s dashboard handler) —
        /// purely for the confirmation message; the actual restart in
        /// `toggle_workspace_shared` re-checks liveness at commit time.
        running_count: usize,
        /// Instances WITHOUT a running session at modal-open time. Sharing
        /// eagerly starts these inside tmux (see `toggle_workspace_shared`),
        /// and the confirmation message must say so.
        stopped_count: usize,
    },
    /// Viewer for a workspace's setup log: the live tail while a create or
    /// archive is in flight, the persisted log file otherwise.
    SetupLog {
        workspace_id: crate::data::store::WorkspaceId,
        /// The persisted log, read once when this became the source. `None`
        /// means the live `in_flight` tail is what's on screen — the file is
        /// still buffered mid-run and would read back empty.
        stored: Option<StoredLog>,
        /// The kind of live work this viewer opened onto, if any. Needed at
        /// handover: once the `in_flight` entry is gone its kind is gone with
        /// it, and only a CREATE has a persisted log to fall back to.
        live_kind: Option<crate::data::in_flight::InFlightKind>,
        /// Lines scrolled up from the end of the log; 0 follows the tail.
        /// Clamped by the renderer against the body height it lays out, and
        /// written back so Up/Down always move what the user can see.
        scroll: usize,
    },
    /// Shown when `q` is pressed while `App::in_flight` is non-empty.
    /// `y` cancels any in-flight creates and quits; archive is abandoned
    /// mid-flight, which is safe because it is self-healing (see the
    /// key handler for detail).
    ConfirmQuit {
        creates: usize,
        archives: usize,
    },
    Error {
        message: String,
    },
    UpdatesPanel {
        /// Index into the modal's ordered workspace list. Up/Down adjust
        /// it; Enter switches `app.view` to that workspace.
        selected: usize,
        /// `None` = normal key handling. `Some(buf)` = filter-input mode,
        /// where printable keys are filter text rather than shortcuts.
        /// `Some("")` is a real state: `/` was pressed, nothing typed yet,
        /// every row still visible. Not persisted, like `sort`.
        filter: Option<String>,
    },
    ProcessList {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize,
        /// `None` = list mode; `Some(buffer)` = the user is typing a command to run.
        input: Option<String>,
        /// Last launch result (success path or error), shown below the list.
        notice: Option<String>,
    },
    RenameWorkspace {
        workspace_id: crate::data::store::WorkspaceId,
        /// Pre-filled with the current name; edited in place.
        name_buffer: String,
        /// Inline error line (e.g. rename failure); cleared on next edit.
        notice: Option<String>,
    },
    RepoSettings {
        repo_id: crate::data::store::RepoId,
        selected: usize,
    },
    /// Pick or name an XML tag and type the body to wrap in it; see
    /// `ui::modal::prompt_tag`. Rendered from `draw()` because it lists
    /// the live tag cache.
    PromptTag(PromptTagModal),
    AgentMissing {
        ws_id: crate::data::store::WorkspaceId,
        agent: crate::pty::session::AgentKind,
        binary: String,
    },
    AgentPicker {
        ws_id: crate::data::store::WorkspaceId,
        selected: usize,
        current: crate::pty::session::AgentKind,
    },
    AgentsPanel {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize, // index into AgentKind::ALL for the add-picker
    },
    UsageWindowPicker {
        /// Index into `UsageWindow::ALL` for the cursor selection. The current
        /// (applied) window is read separately from the store at render time.
        selected: usize,
    },
    /// Static reference card for the workspace-only actions
    /// (edit/term/diff/lazygit/rename/…) — the ones that act only on a
    /// selected workspace. Carries no state — dismissed without side effects.
    WorkspaceActions,
    /// Browse the workspace listing fetched from a remote wsx host
    /// (`App::remote_list`), opened by `reconcile_remote_list`. Rows come from
    /// `crate::app::remote_rows` — one per live agent session, then one per
    /// workspace Enter can share — and `selected` indexes into that list. `notice` surfaces inline feedback
    /// (e.g. "no live session to attach to") the way `ProcessList::notice`
    /// does, without needing a separate modal round-trip.
    RemoteWorkspaceList {
        selected: usize,
        notice: Option<String>,
    },
    /// `H`-key picker over the configured shared hosts (`shared_hosts`
    /// setting), sorted by name. Self-contained snapshot like
    /// `AgentPicker` — `(name, dest)` pairs plus a cursor. Enter allocates
    /// a remote-fetch generation and swaps to `RemoteListLoading`.
    RemoteHostPicker {
        hosts: Vec<(String, String)>,
        selected: usize,
    },
    /// The `#` tag editor. `known`/`tags` are snapshots refreshed after each
    /// write; `selected` indexes `workspace_tags::tag_items(..)`.
    WorkspaceTags {
        target: TagTarget,
        name: String,
        known: Vec<String>,
        tags: Vec<String>,
        input: String,
        selected: usize,
        /// Set by ↑/↓, cleared by typing: space/enter then act on the
        /// highlighted row instead of the typed text.
        browsing: bool,
        notice: Option<String>,
    },
    /// The `F` checklist of hide rules for one repo: tag rows (sorted) then
    /// PR rows, each with how many of the repo's workspaces it matches,
    /// snapshotted on open.
    HideRules {
        repo_id: crate::data::store::RepoId,
        repo_name: String,
        rules: crate::data::hide_rules::HideRules,
        rows: Vec<(crate::data::hide_rules::HideRule, usize)>,
        selected: usize,
    },
    /// The `C` name-color picker for one workspace: an xterm-256 swatch grid
    /// narrowed by a hex/name filter. `current` is a snapshot of the color the
    /// workspace already has (marked in the grid), like `AgentPicker::current`;
    /// `selected` indexes into the FILTERED list, so it is re-seeded to 0 on
    /// every filter edit rather than being clamped.
    NameColorPicker {
        workspace_id: crate::data::store::WorkspaceId,
        current: Option<u8>,
        selected: usize,
        filter: String,
    },
    /// Shown while the background `fetch_shared_list` task for `host_name`
    /// is in flight. Esc closes it and clears `pending_remote_gen`, so the
    /// eventual (stale) reconcile no-ops via its gen guard instead of
    /// reopening a modal the user backed out of.
    RemoteListLoading {
        host_name: String,
    },
    /// Enter on a dormant row of `RemoteWorkspaceList`: confirm sharing
    /// `repo/workspace` on `host_name`, which restarts its running agents
    /// there. `selected` is the list row to return to.
    RemoteShareConfirm {
        selected: usize,
        host_name: String,
        repo: String,
        workspace: String,
        /// Already shared (agents stopped) vs. direct (agents restart).
        shared: bool,
    },
    /// Shown while `share_remote` runs on the host. Esc returns to the list
    /// and drops the pending generation, like `RemoteListLoading`; the share
    /// itself carries on on the host.
    RemoteShareRunning {
        selected: usize,
        host_name: String,
        workspace: String,
    },
    /// `n`/`N` in `RemoteWorkspaceList`: fetching the host's repos for the
    /// new-workspace form. Esc returns to the list like `RemoteShareRunning`.
    /// `yolo` and `repo` seed the form once the repos arrive.
    RemoteReposLoading {
        selected: usize,
        host_name: String,
        yolo: bool,
        repo: Option<String>,
    },
    /// The new-workspace form for a remote host.
    RemoteNewWorkspace(RemoteNewWorkspaceForm),
    /// Shown while `create_remote` runs on the host. Holds the form so a
    /// failure can reopen it with the reason; Esc returns to the list and the
    /// create carries on on the host.
    RemoteCreateRunning(RemoteNewWorkspaceForm),
}

/// What the remote new-workspace form has been given so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteNewWorkspaceForm {
    /// The `RemoteWorkspaceList` row to return to.
    pub selected: usize,
    pub host_name: String,
    /// The host's repos, from `repo list --json`; never empty.
    pub repos: Vec<String>,
    /// Index into `repos`.
    pub repo: usize,
    pub name_buffer: String,
    /// `None` takes the host's `coding_agent` setting.
    pub agent: Option<crate::pty::session::AgentKind>,
    pub yolo: bool,
    /// Inline error line, cleared on the next edit.
    pub notice: Option<String>,
}

impl RemoteNewWorkspaceForm {
    /// The `create_remote` request this form describes.
    pub fn request(&self) -> crate::commands::shared_hosts::RemoteCreate {
        let name = self.name_buffer.trim();
        crate::commands::shared_hosts::RemoteCreate {
            repo: self.repos[self.repo].clone(),
            name: (!name.is_empty()).then(|| name.to_string()),
            agent: self.agent,
            yolo: self.yolo,
        }
    }
}

/// Open a drawer (see `ui::drawer`) sized to a panel's `body_h` rows of
/// content, footer included, and return the body rect. Shared framing for the
/// panel renderers (updates, processes, repo settings, agents) so they look
/// identical; each caller lays its own body/footer split inside the returned
/// rect, which may be shorter than asked (height cap) or taller (the drawer
/// never shrinks while a panel stays open).
fn panel_frame<'a>(
    f: &mut Frame,
    area: Rect,
    body_h: usize,
    title: impl Into<Line<'a>>,
    theme: &Theme,
) -> Rect {
    let body_h = u16::try_from(body_h).unwrap_or(u16::MAX);
    crate::ui::drawer::frame(f, area, body_h, title, &[], theme)
}

/// Render the small fixed-size modals — confirmations, prompts, reference
/// cards. Anything that tails live App state (the panels, and the setup-log
/// viewer) has its own renderer called from `draw_modal` instead.
pub fn render(f: &mut Frame, area: Rect, modal: &Modal, theme: &Theme) {
    // UpdatesPanel, ProcessList, and RemoteWorkspaceList are rendered by
    // their dedicated helpers directly from `draw()` because they need live
    // App state. This function should never be called with those variants;
    // guard defensively.
    if matches!(
        modal,
        Modal::UpdatesPanel { .. }
            | Modal::ProcessList { .. }
            | Modal::RepoSettings { .. }
            | Modal::AgentsPanel { .. }
            | Modal::UsageWindowPicker { .. }
            | Modal::NameColorPicker { .. }
            | Modal::RemoteWorkspaceList { .. }
            | Modal::SetupLog { .. }
            | Modal::PromptTag(..)
    ) {
        return;
    }
    // Room for list rows: the tallest drawer minus its rule and each list
    // modal's fixed lines (prompt, blanks, footer, notice), so the footer
    // stays put.
    let list_cap = usize::from(crate::ui::drawer::max_height(area))
        .saturating_sub(9)
        .max(3);
    const CONFIRM: &[Hint] = &[("y", "yes"), ("n", "cancel")];
    const LIST: &[Hint] = &[("↑↓", "move"), ("↵", "confirm"), ("esc", "cancel")];
    let (title, body, hints): (&str, Text<'static>, &[Hint]) = match modal {
        Modal::NewWorkspace {
            name_buffer,
            tags_buffer,
            tags_focused,
            yolo,
            shared,
            agent,
            notice,
            ..
        } => {
            let agent_label = agent.display_name();
            let (name_cursor, tags_cursor) = if *tags_focused {
                ("", "\u{2588}")
            } else {
                ("\u{2588}", "")
            };
            let shared_line = if *shared { "on" } else { "off" };
            let notice_line = notice
                .as_deref()
                .map(|n| format!("{n}\n"))
                .unwrap_or_default();
            (
                if *yolo {
                    "new workspace (permissive)"
                } else {
                    "new workspace"
                },
                format!(
                    "name:   {name_buffer}{name_cursor}\ntags:   {tags_buffer}{tags_cursor}\nagent:  {agent_label}\nshared: {shared_line} (tmux)\n{notice_line}"
                )
                .trim_end()
                .to_string()
                .into(),
                &[
                    ("↵", "create"),
                    ("↑↓", "name/tags"),
                    ("tab", "agent"),
                    ("^s", "shared"),
                    ("esc", "cancel"),
                ],
            )
        }
        Modal::ConfirmArchive { name, .. } => (
            "archive workspace",
            format!("archive '{name}'?").into(),
            CONFIRM,
        ),
        Modal::ConfirmShare {
            name,
            to_shared,
            running_count,
            stopped_count,
            ..
        } => {
            let dest = if *to_shared {
                "shared (tmux)"
            } else {
                "direct (not tmux)"
            };
            let restart_note = if *to_shared {
                // Sharing eagerly puts EVERY agent inside tmux: running
                // sessions restart wrapped in tmux, stopped ones spawn.
                let mut parts = Vec::new();
                if *running_count > 0 {
                    parts.push(format!(
                        "Restarts {running_count} running session(s) inside tmux (conversation resumes via --continue)."
                    ));
                }
                if *stopped_count > 0 {
                    parts.push(format!(
                        "Starts {stopped_count} stopped agent(s) inside tmux."
                    ));
                }
                if parts.is_empty() {
                    "All agents will run inside tmux.".to_string()
                } else {
                    parts.join("\n")
                }
            } else {
                match running_count {
                    0 => "No running sessions to restart.".to_string(),
                    n => format!(
                        "This restarts {n} running session(s) outside tmux (conversation resumes via --continue)."
                    ),
                }
            };
            (
                "toggle sharing",
                format!("switch '{name}' to {dest}?\n\n{restart_note}").into(),
                CONFIRM,
            )
        }
        Modal::ConfirmQuit { creates, archives } => {
            let mut what = Vec::new();
            if *creates > 0 {
                what.push(format!("{creates} setup(s)"));
            }
            if *archives > 0 {
                what.push(format!("{archives} archive(s)"));
            }
            (
                "work in progress",
                format!(
                    "{} still running.\n\n\
                     Quitting stops them: setups are cancelled, and an archive is\n\
                     left part-done (archiving again finishes it).",
                    what.join(" and ")
                )
                .into(),
                &[("y", "quit anyway"), ("n", "stay")],
            )
        }
        Modal::Error { message } => ("error", message.clone().into(), &[("esc", "dismiss")]),
        // UpdatesPanel is handled by the early-return above; this arm is
        // unreachable but required for exhaustiveness.
        Modal::UpdatesPanel { .. } => unreachable!("UpdatesPanel must not reach render()"),
        Modal::ProcessList { .. } => unreachable!("ProcessList must not reach render()"),
        Modal::RepoSettings { .. } => unreachable!("RepoSettings must not reach render()"),
        Modal::SetupLog { .. } => unreachable!("SetupLog must not reach render()"),
        Modal::AgentsPanel { .. } => unreachable!("AgentsPanel must not reach render()"),
        Modal::UsageWindowPicker { .. } => {
            unreachable!("UsageWindowPicker must not reach render()")
        }
        Modal::PromptTag(..) => unreachable!("PromptTag must not reach render()"),
        Modal::AgentMissing { agent, binary, .. } => (
            "agent not installed",
            format!(
                "{name} is not installed.\n\n\
                 The `{binary}` binary was not found on PATH.\n\
                 Install it, then re-enter the workspace.",
                name = capitalize_first(agent.display_name()),
                binary = binary,
            )
            .into(),
            &[("s", "switch agent"), ("esc", "dismiss")],
        ),
        Modal::WorkspaceActions => (
            "workspace actions",
            key_grid(
                &[
                    ("e", "edit"),
                    ("t", "term"),
                    ("v", "diff"),
                    ("g", "lazygit"),
                    ("r", "rename"),
                    ("C", "name color"),
                    ("o", "setup log"),
                    ("x", "cancel setup"),
                    ("#", "tags"),
                ],
                area.width,
                theme,
            ),
            &[("esc", "close")],
        ),
        Modal::RenameWorkspace {
            name_buffer,
            notice,
            ..
        } => {
            let notice_line = notice
                .as_deref()
                .map(|n| format!("{n}\n"))
                .unwrap_or_default();
            (
                "rename workspace",
                format!("name: {name_buffer}\u{2588}\n{notice_line}")
                    .trim_end()
                    .to_string()
                    .into(),
                &[("↵", "rename"), ("esc", "cancel")],
            )
        }
        // RemoteWorkspaceList is handled by the early-return above; this arm
        // is unreachable but required for exhaustiveness.
        Modal::RemoteWorkspaceList { .. } => {
            unreachable!("RemoteWorkspaceList must not reach render()")
        }
        // Likewise: the picker is drawn by `render_name_color_picker` so it can
        // publish its swatch rects for click hit-testing.
        Modal::NameColorPicker { .. } => {
            unreachable!("NameColorPicker must not reach render()")
        }
        Modal::RemoteHostPicker { hosts, selected } => {
            let list = hosts
                .iter()
                .enumerate()
                .map(|(i, (name, dest))| {
                    let marker = if i == *selected { ">" } else { " " };
                    format!("{marker}  {name}  {dest}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                "pick a shared host",
                format!("Choose a host to browse shared workspaces:\n\n{list}").into(),
                &[("↑↓", "move"), ("↵", "fetch"), ("esc", "cancel")],
            )
        }
        Modal::WorkspaceTags {
            name,
            known,
            tags,
            input,
            selected,
            notice,
            ..
        } => {
            let lines: Vec<String> = workspace_tags::tag_items(known, tags, input)
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    let cursor = if i == *selected { ">" } else { " " };
                    let mark = if it.checked { "x" } else { " " };
                    let new = if it.new { "  (new)" } else { "" };
                    format!("{cursor} [{mark}] {}{new}", it.name)
                })
                .collect();
            let list = window_lines(&lines, *selected, list_cap).join("\n");
            let notice = notice
                .as_deref()
                .map(|n| format!("\n{n}"))
                .unwrap_or_default();
            (
                "workspace tags",
                format!("{name}\n\n# {input}\u{2588}\n{list}{notice}").into(),
                &[
                    ("↑↓", "move"),
                    ("space", "toggle"),
                    ("↵", "add"),
                    ("esc", "close"),
                ],
            )
        }
        Modal::HideRules {
            repo_name,
            rules,
            rows,
            selected,
            ..
        } => {
            use crate::data::hide_rules::HideRule;
            let mut lines = Vec::new();
            let mut focus = 0;
            let mut section: Option<bool> = None;
            for (i, (rule, n)) in rows.iter().enumerate() {
                let is_tag = matches!(rule, HideRule::Tag(_));
                if section != Some(is_tag) {
                    lines.push(if is_tag { "  tagged" } else { "  PR state" }.to_string());
                    section = Some(is_tag);
                }
                if i == *selected {
                    focus = lines.len();
                }
                let cursor = if i == *selected { ">" } else { " " };
                let mark = if rules.contains(rule) { "x" } else { " " };
                lines.push(format!("{cursor} [{mark}] {:<14} {n:>3}", rule.label()));
            }
            (
                "hide workspaces",
                format!(
                    "Hide {repo_name} workspaces that are:\n\n{}",
                    window_lines(&lines, focus, list_cap).join("\n")
                )
                .into(),
                &[("↑↓", "move"), ("space", "toggle"), ("esc", "close")],
            )
        }
        Modal::RemoteListLoading { host_name } => (
            "remote workspaces",
            format!("fetching shared workspaces from {host_name}…").into(),
            &[("esc", "cancel")],
        ),
        Modal::RemoteShareConfirm {
            host_name,
            repo,
            workspace,
            shared,
            ..
        } => {
            let note = if *shared {
                "Its agents are stopped; they start inside tmux there."
            } else {
                "Running agents restart inside tmux there (conversations\n\
                 resume); stopped ones start."
            };
            (
                "share remote workspace",
                format!(
                    "Share {repo}/{workspace} on {host_name} and attach?\n\n{note}\n\
                     Needs wsx running on {host_name}."
                )
                .into(),
                &[("y", "share & attach"), ("n", "back")],
            )
        }
        Modal::RemoteShareRunning {
            host_name,
            workspace,
            ..
        } => (
            "share remote workspace",
            format!(
                "sharing {workspace} on {host_name}…\n\n\
                 waiting for its agents to come up in tmux"
            )
            .into(),
            &[("esc", "back to the list")],
        ),
        Modal::RemoteReposLoading { host_name, .. } => (
            "new remote workspace",
            format!("fetching repos from {host_name}…").into(),
            &[("esc", "back to the list")],
        ),
        Modal::RemoteNewWorkspace(form) => {
            let repo = &form.repos[form.repo];
            let agent = form.agent.map_or("host default", |a| a.display_name());
            let yolo = if form.yolo { "on" } else { "off" };
            let notice_line = form
                .notice
                .as_deref()
                .map(|n| format!("{n}\n"))
                .unwrap_or_default();
            (
                if form.yolo {
                    "new remote workspace (permissive)"
                } else {
                    "new remote workspace"
                },
                format!(
                    "host: {host}\nrepo: {repo}\n\
                     name: {name}\u{2588}\nagent: {agent}\n\
                     yolo: {yolo}\n{notice_line}",
                    host = form.host_name,
                    name = form.name_buffer,
                )
                .trim_end()
                .to_string()
                .into(),
                &[
                    ("↵", "create & attach"),
                    ("↑↓", "repo"),
                    ("tab", "agent"),
                    ("^y", "yolo"),
                    ("esc", "back"),
                ],
            )
        }
        Modal::RemoteCreateRunning(form) => {
            let req = form.request();
            let name = req.name.as_deref().unwrap_or("(generated name)");
            (
                "new remote workspace",
                format!(
                    "creating {repo}/{name} on {host}…\n\n\
                     running its setup, then starting its agents in tmux",
                    repo = req.repo,
                    host = form.host_name,
                )
                .into(),
                &[("esc", "back to the list")],
            )
        }
        Modal::AgentPicker {
            selected, current, ..
        } => {
            let list = crate::pty::session::AgentKind::ALL
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    let marker = if i == *selected { ">" } else { " " };
                    let current_tag = if *k == *current { "  (current)" } else { "" };
                    format!("{marker}  {name}{current_tag}", name = k.display_name())
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                "pick an agent",
                format!("Choose an agent for this workspace:\n\n{list}").into(),
                LIST,
            )
        }
    };
    let style = if matches!(modal, Modal::Error { .. }) {
        theme.err_style()
    } else {
        theme.header_style()
    };
    let body_h = u16::try_from(body.height()).unwrap_or(u16::MAX);
    let inner = crate::ui::drawer::frame(f, area, body_h, title, hints, theme);
    f.render_widget(Paragraph::new(body).style(style), inner);
}

/// `keys` laid out as a which-key style grid of pills: as many fixed-width
/// columns as fit in `width`, filled row by row.
fn key_grid(keys: &[Hint], width: u16, theme: &Theme) -> Text<'static> {
    let cell = keys
        .iter()
        .map(|(k, l)| k.chars().count() + l.chars().count() + 3)
        .max()
        .unwrap_or(0)
        + 4;
    let cols = (usize::from(width).saturating_sub(2) / cell.max(1)).max(1);
    keys.chunks(cols)
        .map(|row| {
            let mut spans = Vec::new();
            for (k, l) in row {
                spans.extend(crate::ui::footer::key_pill_spans(k, theme));
                let used = k.chars().count() + 2;
                spans.push(Span::raw(format!(" {l:<w$}", w = cell - used - 1)));
            }
            Line::from(spans)
        })
        .collect::<Vec<_>>()
        .into()
}

/// At most `cap` of `lines`, scrolled so `focus` stays visible.
fn window_lines(lines: &[String], focus: usize, cap: usize) -> &[String] {
    if lines.len() <= cap {
        return lines;
    }
    let start = focus.saturating_sub(cap / 2).min(lines.len() - cap);
    &lines[start..start + cap]
}

/// Truncate `s` to at most `max` characters, appending '…' (which counts
/// toward `max`) when characters are dropped. Single pass over the input. Used
/// to keep setup-output tail lines inside the modal's inner width.
fn truncate_to(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(max);
    let mut chars = s.chars();
    for _ in 0..max {
        match chars.next() {
            // `s` fit entirely within `max` — no truncation, return as-is.
            None => return out,
            Some(c) => out.push(c),
        }
    }
    // Consumed exactly `max` chars; if any remain, truncation occurred.
    if chars.next().is_some() {
        // Drop the last kept char for the ellipsis so the total stays ≤ `max`.
        // When `max == 0` nothing was kept, so there's no room even for '…'.
        if out.pop().is_some() {
            out.push('…');
        }
    }
    out
}

/// Uppercase only the first character of `s`. Used to render the agent
/// name in the AgentMissing modal as a proper sentence-start without
/// changing the canonical lowercase form returned by `AgentKind::display_name`.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn hide_rules_picker_draws_section_headers() {
        use crate::data::hide_rules::{HideRule, HideRules, PrCriterion};
        let m = Modal::HideRules {
            repo_id: crate::data::store::RepoId(1),
            repo_name: "wsx".into(),
            rules: HideRules::parse(Some("tag:parked")),
            rows: vec![
                (HideRule::tag("parked"), 2),
                (HideRule::Pr(PrCriterion::Merged), 0),
            ],
            selected: 0,
        };
        let text = render_to_text(&m);
        let tagged = text.find("tagged").expect("tag header");
        let tag_row = text.find("#parked").expect("tag row");
        let pr = text.find("PR state").expect("pr header");
        assert!(tagged < tag_row && tag_row < pr, "{text}");
        assert!(text.contains("[x] #parked"), "{text}");
    }

    #[test]
    fn long_hide_rules_picker_keeps_footer_and_cursor_visible() {
        use crate::data::hide_rules::{HideRule, HideRules};
        let mut rows: Vec<_> = (0..20)
            .map(|i| (HideRule::tag(&format!("t{i:02}")), 0))
            .collect();
        rows.extend(HideRule::PR_RULES.map(|r| (r, 0)));
        let selected = 19;
        let text = render_to_text(&Modal::HideRules {
            repo_id: crate::data::store::RepoId(1),
            repo_name: "wsx".into(),
            rules: HideRules::default(),
            rows,
            selected,
        });
        assert!(text.contains("esc  close"), "{text}");
        assert!(text.contains("> [ ] #t19"), "{text}");
    }

    #[test]
    fn long_tag_editor_keeps_footer_and_cursor_visible() {
        let known: Vec<String> = (0..20).map(|i| format!("t{i:02}")).collect();
        let text = render_to_text(&Modal::WorkspaceTags {
            target: TagTarget::Local(crate::data::store::WorkspaceId(1)),
            name: "ws".into(),
            known,
            tags: Vec::new(),
            input: String::new(),
            selected: 19,
            browsing: false,
            notice: None,
        });
        assert!(text.contains("esc  close"), "{text}");
        assert!(text.contains("> [ ] t19"), "{text}");
    }

    fn render_to_text(modal: &Modal) -> String {
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render(f, f.area(), modal, &theme)).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn new_workspace_shows_tags_row_and_moves_cursor_with_focus() {
        let modal = |tags_focused| Modal::NewWorkspace {
            repo_id: RepoId(1),
            name_buffer: "beta".into(),
            tags_buffer: "ui spike".into(),
            tags_focused,
            yolo: false,
            shared: false,
            agent: crate::pty::session::AgentKind::Claude,
            notice: None,
        };
        let text = render_to_text(&modal(false));
        assert!(text.contains("name:   beta\u{2588}"), "{text}");
        assert!(text.contains("tags:   ui spike "), "{text}");
        let text = render_to_text(&modal(true));
        assert!(text.contains("name:   beta "), "{text}");
        assert!(text.contains("tags:   ui spike\u{2588}"), "{text}");
    }

    #[test]
    fn remote_share_confirm_says_what_restarts_where() {
        let text = render_to_text(&Modal::RemoteShareConfirm {
            selected: 0,
            host_name: "mini".into(),
            repo: "r".into(),
            workspace: "w".into(),
            shared: false,
        });
        assert!(text.contains("Share r/w on mini and attach?"), "{text}");
        assert!(text.contains("restart inside tmux"), "{text}");
        assert!(text.contains("y  share & attach"), "{text}");
    }

    #[test]
    fn remote_new_workspace_form_shows_host_repo_agent_and_yolo() {
        let form = RemoteNewWorkspaceForm {
            selected: 0,
            host_name: "mini".into(),
            repos: vec!["a".into(), "r".into()],
            repo: 1,
            name_buffer: "fresh".into(),
            agent: None,
            yolo: true,
            notice: Some("taken".into()),
        };
        let text = render_to_text(&Modal::RemoteNewWorkspace(form.clone()));
        assert!(text.contains("new remote workspace (permissive)"), "{text}");
        assert!(text.contains("host: mini"), "{text}");
        assert!(text.contains("repo: r"), "{text}");
        assert!(text.contains("name: fresh"), "{text}");
        assert!(text.contains("agent: host default"), "{text}");
        assert!(text.contains("yolo: on"), "{text}");
        assert!(text.contains("taken"), "{text}");
        let text = render_to_text(&Modal::RemoteCreateRunning(form));
        assert!(text.contains("creating r/fresh on mini"), "{text}");
    }

    /// Sharing eagerly starts stopped agents (see `toggle_workspace_shared`),
    /// so the confirmation must say so — a bare "No running sessions to
    /// restart." would read as "nothing will happen", which is exactly the
    /// expectation mismatch that made shares of stopped workspaces look
    /// like failures.
    #[test]
    fn confirm_share_to_shared_mentions_starting_stopped_agents() {
        let modal = Modal::ConfirmShare {
            workspace_id: crate::data::store::WorkspaceId(1),
            name: "w".into(),
            to_shared: true,
            running_count: 1,
            stopped_count: 2,
        };
        let text = render_to_text(&modal);
        assert!(
            text.contains("Restarts 1 running session(s)"),
            "missing running-restart note:\n{text}"
        );
        assert!(
            text.contains("Starts 2 stopped agent(s)"),
            "missing stopped-start note:\n{text}"
        );
    }

    /// Unsharing keeps the old semantics: only running sessions restart,
    /// stopped agents stay stopped — the copy must not promise spawns.
    #[test]
    fn confirm_share_to_direct_keeps_no_running_sessions_note() {
        let modal = Modal::ConfirmShare {
            workspace_id: crate::data::store::WorkspaceId(1),
            name: "w".into(),
            to_shared: false,
            running_count: 0,
            stopped_count: 2,
        };
        let text = render_to_text(&modal);
        assert!(
            text.contains("No running sessions to restart."),
            "missing no-op note:\n{text}"
        );
        assert!(
            !text.contains("stopped agent"),
            "unshare must not promise to start stopped agents:\n{text}"
        );
    }

    #[test]
    fn truncate_to_handles_fit_truncate_and_zero() {
        // Fits exactly — unchanged, no ellipsis.
        assert_eq!(truncate_to("abc", 3), "abc");
        // Shorter than max — unchanged.
        assert_eq!(truncate_to("ab", 5), "ab");
        // Longer than max — ellipsis counts toward the budget (total == max).
        assert_eq!(truncate_to("abcdef", 3), "ab…");
        assert_eq!(truncate_to("abcdef", 3).chars().count(), 3);
        // max == 0 — never exceeds the budget, even when truncating.
        assert_eq!(truncate_to("abc", 0), "");
        assert_eq!(truncate_to("", 0), "");
        // Multi-byte chars are counted by char, not byte.
        assert_eq!(truncate_to("héllo", 2), "h…");
    }

    #[test]
    fn workspace_actions_overlay_lists_all_actions() {
        let text = render_to_text(&Modal::WorkspaceActions);
        assert!(text.contains("edit"), "missing 'edit':\n{text}");
        assert!(text.contains("term"), "missing 'term':\n{text}");
        assert!(text.contains("diff"), "missing 'diff':\n{text}");
        assert!(text.contains("lazygit"), "missing 'lazygit':\n{text}");
        assert!(!text.contains("chronox"), "unexpected 'chronox':\n{text}");
    }
}
