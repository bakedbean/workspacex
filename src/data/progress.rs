//! Live progress sink for workspace creation. `workspace::create_with_app`
//! writes the current phase and the setup script's output lines here; the TUI
//! `SetupRunning` modal reads it each frame to render a phase label and a live
//! tail. A plain `std::sync::Mutex` (not tokio) is used deliberately: both the
//! writer (the synchronous `on_line` callback) and the reader (`render`) are
//! synchronous and hold the lock only for microseconds, never across `.await`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Coarse phase of `create_with_app`, shown in the modal header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPhase {
    /// `git fetch` of the base branch.
    Fetching,
    /// `git worktree add`.
    CreatingWorktree,
    /// The repo's setup script.
    RunningSetup,
}

impl SetupPhase {
    /// Header label for the modal (no trailing ellipsis; the renderer adds it).
    pub fn label(self) -> &'static str {
        match self {
            SetupPhase::Fetching => "Fetching base",
            SetupPhase::CreatingWorktree => "Creating worktree",
            SetupPhase::RunningSetup => "Running setup",
        }
    }
}

/// Max output lines retained in the ring buffer.
const CAP: usize = 64;

/// Progress state shared between the create task and the modal renderer.
#[derive(Debug)]
pub struct SetupProgress {
    phase: SetupPhase,
    lines: VecDeque<String>,
}

/// Shared handle. Clone to hand one copy to the modal and one to the create task.
pub type SharedProgress = Arc<Mutex<SetupProgress>>;

impl SetupProgress {
    /// A new handle, starting in the `Fetching` phase with no output.
    pub fn shared() -> SharedProgress {
        Arc::new(Mutex::new(SetupProgress {
            phase: SetupPhase::Fetching,
            lines: VecDeque::new(),
        }))
    }

    /// Set the current phase.
    pub fn set_phase(&mut self, phase: SetupPhase) {
        self.phase = phase;
    }

    /// Return the current phase.
    pub fn phase(&self) -> SetupPhase {
        self.phase
    }

    /// Strip ANSI escapes, trim trailing whitespace, and append. Drops the
    /// oldest line once at capacity. Blank results are ignored.
    pub fn push_line(&mut self, raw: &str) {
        let clean = strip_ansi_escapes::strip_str(raw);
        let clean = clean.trim_end();
        if clean.is_empty() {
            return;
        }
        if self.lines.len() == CAP {
            self.lines.pop_front();
        }
        self.lines.push_back(clean.to_string());
    }

    /// The last `n` lines, oldest-first, for the modal tail.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let start = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(start).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_round_trips_and_labels() {
        let p = SetupProgress::shared();
        assert_eq!(p.lock().unwrap().phase(), SetupPhase::Fetching);
        p.lock().unwrap().set_phase(SetupPhase::RunningSetup);
        assert_eq!(p.lock().unwrap().phase(), SetupPhase::RunningSetup);
        assert_eq!(SetupPhase::Fetching.label(), "Fetching base");
        assert_eq!(SetupPhase::CreatingWorktree.label(), "Creating worktree");
        assert_eq!(SetupPhase::RunningSetup.label(), "Running setup");
    }

    #[test]
    fn push_line_strips_ansi_and_trims() {
        let p = SetupProgress::shared();
        p.lock().unwrap().push_line("\x1b[32mgreen text\x1b[0m   ");
        assert_eq!(p.lock().unwrap().recent(5), vec!["green text"]);
    }

    #[test]
    fn push_line_skips_blank() {
        let p = SetupProgress::shared();
        p.lock().unwrap().push_line("   ");
        p.lock().unwrap().push_line("");
        assert!(p.lock().unwrap().recent(5).is_empty());
    }

    #[test]
    fn ring_buffer_evicts_oldest_at_cap() {
        let p = SetupProgress::shared();
        for i in 0..(CAP + 5) {
            p.lock().unwrap().push_line(&format!("line {i}"));
        }
        let g = p.lock().unwrap();
        let all = g.recent(CAP + 100);
        assert_eq!(all.len(), CAP, "buffer should be capped");
        assert_eq!(all[0], format!("line {}", 5), "oldest 5 evicted");
        assert_eq!(all[CAP - 1], format!("line {}", CAP + 4));
    }

    #[test]
    fn recent_returns_last_n_oldest_first() {
        let p = SetupProgress::shared();
        for i in 0..10 {
            p.lock().unwrap().push_line(&format!("l{i}"));
        }
        assert_eq!(p.lock().unwrap().recent(3), vec!["l7", "l8", "l9"]);
    }
}
