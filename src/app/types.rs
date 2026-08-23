//! Small types the app passes around: events, selection targets, and
//! the enums describing how a session stopped or what is pending.

use super::*;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AppEvent {
    Tick,
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),
    SetupLine(String),
    SetupFinished { id: WorkspaceId, ok: bool },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionTarget {
    Repo(crate::data::store::RepoId),
    Workspace(crate::data::store::WorkspaceId),
}

/// Outcome of `ensure_workspace_session`. `AgentMissing` signals to callers
/// that the spawn failed because the agent binary was not on PATH; the
/// helper already set `Modal::AgentMissing`, so callers should skip the
/// view switch and leave the modal up. `Refused` signals that the session
/// was not touched because `attach_is_blocked` refused it (a live archive
/// is tearing the workspace down) — callers should treat this exactly like
/// `AgentMissing`: skip the view switch, set no session, leave things alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachReady {
    Ok,
    AgentMissing,
    Refused,
}

#[derive(Debug, Clone)]
pub struct PendingEdit {
    pub repo_id: crate::data::store::RepoId,
    pub field: RepoSettingField,
}

/// Why the agent paused at end-of-turn. Distinguishes "asked the user
/// something and is waiting for an answer" from "finished a task".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedKind {
    /// The agent invoked `AskUserQuestion` or `ExitPlanMode` and the
    /// user hasn't responded yet, OR the final assistant text ended
    /// with `?` (fallback). Maps to the "?" dashboard glyph.
    AwaitingAnswer,
    /// The agent finished without asking the user anything. Maps to
    /// the "✓" dashboard glyph.
    Complete,
}

/// How many hourly activity buckets to retain, in memory and in the DB. Sized
/// to the largest selectable usage-graph window (30 days), so the setting is
/// purely a view over already-collected data rather than affecting retention.
pub(crate) const MAX_ACTIVITY_HOURS: u64 = 720;
