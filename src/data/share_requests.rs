//! Requests from a sibling `wsx` process (`wsx workspace share --restart`)
//! asking the running dashboard to share a workspace and start its agents
//! inside tmux. Only the dashboard can do the restart: a direct agent is its
//! child process. The CLI enqueues a row; the dashboard claims it on its tick,
//! acts, and records the outcome for the CLI to read back.
//!
//! Several dashboards can share one database, but a direct agent is the child
//! of exactly one of them, and only that one can kill it. So a dashboard
//! running a direct agent of the workspace claims at once, and any other
//! waits `DEFER_MS` first, giving the owner the chance to claim it.
//!
//! A request the dashboard never claims is withdrawn by the CLI, and one that
//! has sat unclaimed past `STALE_AFTER_MS` (its CLI was killed) is discarded
//! rather than acted on, so a dashboard started later doesn't restart a
//! workspace's agents on behalf of a command that already gave up.

use crate::data::store::{Store, WorkspaceId, now_ms};
use crate::error::Result;
use rusqlite::OptionalExtension;

/// An unclaimed request older than this is abandoned, not acted on.
pub const STALE_AFTER_MS: i64 = 30_000;

/// How long a dashboard that doesn't own any of the workspace's running
/// agents leaves a request for one that does. Well inside the CLI's claim
/// wait, so a lone dashboard still answers in time.
pub const DEFER_MS: i64 = 1_500;

/// Finished rows the CLI never cleaned up (it was killed while waiting), and
/// claimed ones a crashed dashboard never finished, are purged at this age.
const PURGE_FINISHED_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareRequest {
    pub id: i64,
    pub workspace_id: WorkspaceId,
}

/// What one `claim_share_requests` pass took, and whether it left fresh
/// requests for their owner — the caller should look again shortly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShareClaim {
    pub claimed: Vec<ShareRequest>,
    pub deferred: bool,
}

/// Where a request stands, as the waiting CLI sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareRequestState {
    Pending,
    Claimed,
    Finished {
        error: Option<String>,
    },
    /// The row no longer exists (withdrawn, purged, or never existed).
    Gone,
}

impl Store {
    pub fn enqueue_share_request(&self, workspace_id: WorkspaceId) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO share_requests (workspace_id, created_at) VALUES (?1, ?2)",
            rusqlite::params![workspace_id.0, now_ms()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Claim the pending requests this dashboard should act on: those for a
    /// workspace it `owns` (runs a direct agent of) at once, any other once it
    /// is `DEFER_MS` old. Also discards stale and purges old finished rows.
    /// One IMMEDIATE transaction, so two dashboards on the same database
    /// never both act on a request.
    pub fn claim_share_requests(&self, owns: impl Fn(WorkspaceId) -> bool) -> Result<ShareClaim> {
        let now = now_ms();
        let tx = rusqlite::Transaction::new_unchecked(
            self.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        tx.execute(
            "DELETE FROM share_requests WHERE claimed_at IS NULL AND created_at < ?1",
            [now - STALE_AFTER_MS],
        )?;
        // Finished rows whose CLI never collected them, and claimed rows whose
        // dashboard died mid-share: nobody will read either again.
        tx.execute(
            "DELETE FROM share_requests WHERE finished_at < ?1 \
             OR (finished_at IS NULL AND claimed_at < ?1)",
            [now - PURGE_FINISHED_AFTER_MS],
        )?;
        let pending = {
            let mut stmt = tx.prepare(
                "SELECT id, workspace_id, created_at FROM share_requests \
                 WHERE claimed_at IS NULL ORDER BY id ASC",
            )?;
            stmt.query_map([], |r| {
                Ok((
                    ShareRequest {
                        id: r.get(0)?,
                        workspace_id: WorkspaceId(r.get(1)?),
                    },
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut out = ShareClaim::default();
        for (req, created_at) in pending {
            if owns(req.workspace_id) || created_at <= now - DEFER_MS {
                tx.execute(
                    "UPDATE share_requests SET claimed_at = ?1 WHERE id = ?2",
                    [now, req.id],
                )?;
                out.claimed.push(req);
            } else {
                out.deferred = true;
            }
        }
        tx.commit()?;
        Ok(out)
    }

    pub fn finish_share_request(&self, id: i64, error: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE share_requests SET finished_at = ?1, error = ?2 WHERE id = ?3",
            rusqlite::params![now_ms(), error, id],
        )?;
        Ok(())
    }

    pub fn share_request_state(&self, id: i64) -> Result<ShareRequestState> {
        let row = self
            .conn()
            .query_row(
                "SELECT claimed_at, finished_at, error FROM share_requests WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        Ok(match row {
            None => ShareRequestState::Gone,
            Some((_, Some(_), error)) => ShareRequestState::Finished { error },
            Some((Some(_), None, _)) => ShareRequestState::Claimed,
            Some((None, None, _)) => ShareRequestState::Pending,
        })
    }

    /// Withdraw a request no dashboard has claimed. Returns false when one
    /// claimed it in the meantime — the caller should keep waiting.
    pub fn withdraw_share_request(&self, id: i64) -> Result<bool> {
        let n = self.conn().execute(
            "DELETE FROM share_requests WHERE id = ?1 AND claimed_at IS NULL",
            [id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_share_request(&self, id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM share_requests WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_lifecycle_pending_claimed_finished() {
        let store = Store::open_in_memory().unwrap();
        let id = store.enqueue_share_request(WorkspaceId(7)).unwrap();
        assert_eq!(
            store.share_request_state(id).unwrap(),
            ShareRequestState::Pending
        );

        let claimed = store.claim_share_requests(|_| true).unwrap().claimed;
        assert_eq!(
            claimed,
            vec![ShareRequest {
                id,
                workspace_id: WorkspaceId(7)
            }]
        );
        assert_eq!(
            store.share_request_state(id).unwrap(),
            ShareRequestState::Claimed
        );
        // Claimed once: a second dashboard sees nothing to do.
        assert!(
            store
                .claim_share_requests(|_| true)
                .unwrap()
                .claimed
                .is_empty()
        );
        // And the CLI can no longer withdraw it.
        assert!(!store.withdraw_share_request(id).unwrap());

        store.finish_share_request(id, Some("boom")).unwrap();
        assert_eq!(
            store.share_request_state(id).unwrap(),
            ShareRequestState::Finished {
                error: Some("boom".into())
            }
        );
        store.delete_share_request(id).unwrap();
        assert_eq!(
            store.share_request_state(id).unwrap(),
            ShareRequestState::Gone
        );
    }

    #[test]
    fn unclaimed_request_can_be_withdrawn() {
        let store = Store::open_in_memory().unwrap();
        let id = store.enqueue_share_request(WorkspaceId(1)).unwrap();
        assert!(store.withdraw_share_request(id).unwrap());
        assert!(
            store
                .claim_share_requests(|_| true)
                .unwrap()
                .claimed
                .is_empty()
        );
    }

    #[test]
    fn stale_unclaimed_request_is_discarded_not_claimed() {
        let store = Store::open_in_memory().unwrap();
        let id = store.enqueue_share_request(WorkspaceId(1)).unwrap();
        store
            .conn()
            .execute(
                "UPDATE share_requests SET created_at = ?1 WHERE id = ?2",
                rusqlite::params![now_ms() - STALE_AFTER_MS - 1, id],
            )
            .unwrap();
        assert!(
            store
                .claim_share_requests(|_| true)
                .unwrap()
                .claimed
                .is_empty()
        );
        assert_eq!(
            store.share_request_state(id).unwrap(),
            ShareRequestState::Gone
        );
    }

    #[test]
    fn a_non_owner_leaves_a_fresh_request_for_the_owner() {
        let store = Store::open_in_memory().unwrap();
        let id = store.enqueue_share_request(WorkspaceId(3)).unwrap();

        let other = store.claim_share_requests(|_| false).unwrap();
        assert!(other.claimed.is_empty() && other.deferred);
        assert_eq!(
            store.share_request_state(id).unwrap(),
            ShareRequestState::Pending
        );

        let owner = store
            .claim_share_requests(|ws| ws == WorkspaceId(3))
            .unwrap();
        assert_eq!(owner.claimed.len(), 1);
        assert!(!owner.deferred);
    }

    #[test]
    fn a_non_owner_takes_a_request_nobody_claimed_in_time() {
        let store = Store::open_in_memory().unwrap();
        let id = store.enqueue_share_request(WorkspaceId(3)).unwrap();
        store
            .conn()
            .execute(
                "UPDATE share_requests SET created_at = ?1 WHERE id = ?2",
                rusqlite::params![now_ms() - DEFER_MS, id],
            )
            .unwrap();
        let claim = store.claim_share_requests(|_| false).unwrap();
        assert_eq!(claim.claimed.len(), 1);
        assert!(!claim.deferred);
    }
}
