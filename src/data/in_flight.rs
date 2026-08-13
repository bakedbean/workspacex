//! Registry of background workspace work owned by `App`.
//!
//! Create and archive both run as detached tokio tasks. Their progress sink
//! and cancellation token used to live inside `Modal::SetupRunning`, which
//! meant closing the modal dropped the only handle to the running work —
//! the reason Esc cancelled instead of backgrounding. `App` owns them now
//! and the modal borrows a view, so the modal can be opened and closed
//! freely while the work continues.
//!
//! This registry — not the persisted `SetupStatus::Running` — is the source
//! of truth for the dashboard's in-flight badges. It lives in this process,
//! so an entry's presence proves a task is genuinely alive.

use crate::data::progress::SharedProgress;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Which lifecycle operation is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFlightKind {
    Create,
    /// Archive is not cancellable — the token is carried for uniformity and
    /// is never fired. See `workspace::archive_with_app`.
    Archive,
}

/// One in-flight lifecycle operation against a workspace.
#[derive(Debug, Clone)]
pub struct InFlight {
    pub kind: InFlightKind,
    pub progress: SharedProgress,
    pub cancel: CancellationToken,
    pub started: Instant,
}

impl InFlight {
    pub fn create(progress: SharedProgress, cancel: CancellationToken) -> Self {
        Self {
            kind: InFlightKind::Create,
            progress,
            cancel,
            started: Instant::now(),
        }
    }

    pub fn archive(progress: SharedProgress, cancel: CancellationToken) -> Self {
        Self {
            kind: InFlightKind::Archive,
            progress,
            cancel,
            started: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_archive_constructors_set_their_kind() {
        let c = InFlight::create(
            crate::data::progress::SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        );
        assert_eq!(c.kind, InFlightKind::Create);

        let a = InFlight::archive(
            crate::data::progress::SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        );
        assert_eq!(a.kind, InFlightKind::Archive);
    }

    #[test]
    fn cancel_handle_is_shared_with_the_caller() {
        let token = tokio_util::sync::CancellationToken::new();
        let f = InFlight::create(
            crate::data::progress::SetupProgress::shared(),
            token.clone(),
        );
        assert!(!f.cancel.is_cancelled());
        token.cancel();
        assert!(
            f.cancel.is_cancelled(),
            "the registry must hold a live handle, not a detached copy"
        );
    }
}
