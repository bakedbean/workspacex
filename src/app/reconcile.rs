//! Applying the result of a background create or archive back onto
//! `App`, guarded by the generation counter that issued it.

use super::*;

/// Reconcile the outcome of a spawned `workspace::create_with_app` task.
/// Locks the app briefly and — on success — selects the new workspace.
/// Does NOT touch `in_flight`: with concurrent creates reachable (the
/// blocking modal that used to serialize them is gone), only the task that
/// inserted an entry knows for certain it's the one that finished, so
/// `create_with_app` itself removes its own entry by id on every exit path.
/// A blanket removal here could delete a different, still-running create's
/// entry. There is no modal bookkeeping either: a failed create is carried
/// by the row badge (not the transient error modal this replaced), and
/// `Modal::SetupProgress` is a viewer the user may already have closed, so
/// it is never touched here — EXCEPT for the `Err(_)` backstop below.
/// Regardless of outcome, `refresh()` runs so the dashboard reflects any
/// state written to the store.
///
/// `repo_id`/`name` are the exact `(repo_id, name)` the caller resolved and
/// asked `create_with_app` to insert (the caller pre-resolves an auto-
/// generated name too, rather than letting `create_with_app` pick one, so
/// this is always the real attempted name). They are only consulted on
/// `Err(_)`, to answer "did a row for this attempt ever get inserted?" —
/// see that arm.
pub(crate) async fn reconcile_create_result(
    app: SharedApp,
    my_gen: u64,
    repo_id: crate::data::store::RepoId,
    name: String,
    result: Result<crate::data::workspace::CreatedWorkspace>,
) {
    let mut g = app.lock().await;
    let is_mine = g.pending_create_gen == Some(my_gen);
    if is_mine {
        g.pending_create_gen = None;
    }
    let new_ws = result
        .as_ref()
        .ok()
        .map(|c| (c.workspace.id, c.workspace.repo_id));
    match result {
        Ok(_) => {
            let _ = g.refresh();
            // Select the newly created workspace so the dashboard lands on it.
            if let Some((id, repo_id)) = new_ws {
                // Unfold the owning repo first. If it was collapsed (explicit
                // fold or `default_fold` of an idle/empty repo), the new
                // workspace would be hidden from `visible_targets` on the next
                // draw, so the selection below would land on an invisible row
                // and get parked — no highlight, and the nav cursor clamped
                // onto an unrelated neighbor. Expanding makes the row visible
                // so the selection sticks.
                g.dashboard.folded.insert(repo_id.0 as u64, false);
                if let Some(idx) = g
                    .selectable
                    .iter()
                    .position(|t| *t == SelectionTarget::Workspace(id))
                {
                    g.select_index(idx);
                }
            }
        }
        Err(crate::error::Error::Cancelled) => {
            // The `x` binding on the workspace-actions card fires `cancel`
            // on a live create's token; a `Modal::ConfirmQuit` `y` does too.
            // Refresh so the dashboard reflects setup_status=Cancelled.
            let _ = g.refresh();
        }
        Err(_) => {
            let _ = g.refresh();
            // F5 backstop: failures AFTER the row exists are carried by the
            // row badge — deliberately silent here, same as any other
            // `Err(_)`. But `create_with_app` can also fail BEFORE the row
            // is ever inserted (`resolve_branch_prefix`, `insert_workspace`,
            // or `add_primary_agent` erroring in Phase 1/2, ahead of the
            // wrapped async block) — most commonly a `UNIQUE(repo_id, name)`
            // violation, though the Enter handler now validates that case
            // up front. With no row there is no badge and therefore no
            // feedback at all; the modal would just have closed. Whether a
            // row exists is the simplest reliable signal available here (no
            // id is returned on `Err`, so this is the only way to tell) —
            // look it up by the exact `(repo_id, name)` this attempt used,
            // and only pop `Modal::Error` when it's genuinely missing.
            let row_exists = g
                .store
                .workspaces(repo_id)
                .map(|rows| rows.iter().any(|w| w.name == name))
                .unwrap_or(false);
            if !row_exists {
                g.modal = Some(crate::ui::modal::Modal::Error {
                    message: format!("failed to create workspace '{name}'"),
                });
            }
        }
    }
}

/// Reconcile the outcome of a spawned `workspace::archive_with_app` task.
/// Locks the app briefly and always removes `ws_id`'s own `in_flight` entry
/// by id — never a blanket sweep of every `Archive` entry — since multiple
/// archives can be in flight concurrently and a blanket retain could evict a
/// different, still-running archive's entry. `pending_archive_gen` is kept
/// only as informational bookkeeping, mirroring `pending_create_gen`. There
/// is no modal to touch on success or failure: a failed removal is logged
/// (there is no persisted failure status for archive to badge off, unlike
/// create's `SetupStatus::Failed`), and `refresh()` always runs so the
/// dashboard reflects the store mutation (or lack of one, on failure).
pub(crate) async fn reconcile_archive_result(
    app: SharedApp,
    my_gen: u64,
    ws_id: crate::data::store::WorkspaceId,
    result: Result<crate::data::setup::SetupResult>,
) {
    let mut g = app.lock().await;
    if g.pending_archive_gen == Some(my_gen) {
        g.pending_archive_gen = None;
    }
    g.in_flight.remove(&ws_id);
    if let Err(e) = &result {
        tracing::warn!(error = %e, workspace_id = ?ws_id, "archive failed");
    }
    let _ = g.refresh();
}

#[cfg(test)]
mod reconcile_create_tests {
    use super::*;
    use crate::data::store::NewWorkspace;
    use crate::error::Error;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn make_app_with_repo() -> (App, crate::data::store::RepoId) {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let app = App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
        (app, repo_id)
    }

    /// F5 part 2: a create can fail before its row is ever inserted
    /// (`resolve_branch_prefix`, `insert_workspace`, or `add_primary_agent`
    /// erroring in Phase 1/2, ahead of `create_with_app`'s wrapped async
    /// block). With no row there is no badge and therefore no feedback at
    /// all. The `Err(_)` backstop must pop `Modal::Error` in exactly this
    /// case — detected here by no row existing for the attempted
    /// `(repo_id, name)`.
    #[tokio::test]
    async fn err_with_no_matching_row_pops_an_error_modal() {
        let (app, repo_id) = make_app_with_repo();
        let my_gen = 0;
        let shared = Arc::new(Mutex::new(app));
        reconcile_create_result(
            shared.clone(),
            my_gen,
            repo_id,
            "ghost".to_string(),
            Err(Error::UserInput("boom".into())),
        )
        .await;
        let g = shared.lock().await;
        assert!(
            matches!(g.modal, Some(crate::ui::modal::Modal::Error { .. })),
            "a create that failed before any row existed must surface an \
             error modal — nothing else can carry that feedback: {:?}",
            g.modal
        );
    }

    /// Failures AFTER the row exists are carried by the row badge — a
    /// deliberate design decision — so the `Err(_)` arm must stay silent
    /// when a row for the attempted `(repo_id, name)` is actually present
    /// (e.g. a fetch/checkout/setup failure inside the wrapped async block,
    /// all of which run after the row is inserted).
    #[tokio::test]
    async fn err_with_a_matching_row_stays_silent() {
        let (app, repo_id) = make_app_with_repo();
        app.store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ghost",
                branch: "repo/ghost",
                worktree_path: std::path::Path::new("/tmp/r/ghost"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let my_gen = 0;
        let shared = Arc::new(Mutex::new(app));
        reconcile_create_result(
            shared.clone(),
            my_gen,
            repo_id,
            "ghost".to_string(),
            Err(Error::Setup("boom".into())),
        )
        .await;
        let g = shared.lock().await;
        assert!(
            g.modal.is_none(),
            "a failure after the row exists must stay silent — the row \
             badge carries it: {:?}",
            g.modal
        );
    }

    /// A cancellation must never pop `Modal::Error` either way — cancelling
    /// is a deliberate user action (the `x` binding, or `ConfirmQuit`'s `y`),
    /// not a failure that needs surfacing.
    #[tokio::test]
    async fn cancelled_never_pops_an_error_modal_even_with_no_row() {
        let (app, repo_id) = make_app_with_repo();
        let my_gen = 0;
        let shared = Arc::new(Mutex::new(app));
        reconcile_create_result(
            shared.clone(),
            my_gen,
            repo_id,
            "ghost".to_string(),
            Err(Error::Cancelled),
        )
        .await;
        let g = shared.lock().await;
        assert!(
            g.modal.is_none(),
            "cancellation must stay silent: {:?}",
            g.modal
        );
    }
}

#[cfg(test)]
mod reconcile_archive_tests {
    use super::*;
    use crate::data::in_flight::InFlight;
    use crate::data::setup::SetupResult;
    use crate::error::Error;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    fn make_app() -> (App, TempDir) {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        let app = App::new(store, tmp.path().to_path_buf()).unwrap();
        (app, tmp)
    }

    fn seed_archive_entry(app: &mut App, ws_id: crate::data::store::WorkspaceId) {
        app.in_flight.insert(
            ws_id,
            InFlight::archive(
                crate::data::progress::SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
        );
    }

    #[tokio::test]
    async fn reconcile_ok_removes_the_archived_workspaces_in_flight_entry() {
        let (mut app, _tmp) = make_app();
        let ws_id = crate::data::store::WorkspaceId(1);
        seed_archive_entry(&mut app, ws_id);
        app.pending_archive_gen = Some(7);
        app.next_archive_gen = 8;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(shared.clone(), 7, ws_id, Ok(SetupResult::Ok)).await;
        let g = shared.lock().await;
        assert!(
            !g.in_flight.contains_key(&ws_id),
            "the archived workspace's in_flight entry should be removed"
        );
        assert!(
            g.pending_archive_gen.is_none(),
            "pending_archive_gen should clear after matching reconcile"
        );
    }

    #[tokio::test]
    async fn reconcile_err_still_removes_the_in_flight_entry() {
        let (mut app, _tmp) = make_app();
        let ws_id = crate::data::store::WorkspaceId(1);
        seed_archive_entry(&mut app, ws_id);
        app.pending_archive_gen = Some(7);
        app.next_archive_gen = 8;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(shared.clone(), 7, ws_id, Err(Error::Setup("boom".into()))).await;
        let g = shared.lock().await;
        assert!(
            !g.in_flight.contains_key(&ws_id),
            "a failed archive must still clear its own in_flight entry so the badge stops spinning"
        );
        assert!(
            g.pending_archive_gen.is_none(),
            "pending_archive_gen should clear after matching reconcile"
        );
    }

    /// Regression: a blanket `retain(|_, f| f.kind != Archive)` would delete
    /// a different, still-running archive's entry — the same bug fix-round-1
    /// found on the create side. `pending_archive_gen` cannot be trusted to
    /// mean only one archive is ever in flight: it is a single slot that a
    /// second, concurrently-started archive silently clobbers (nothing
    /// serializes archives once the blocking modal is gone). This drives a
    /// reconcile for one workspace and asserts an unrelated concurrent
    /// archive's entry survives.
    #[tokio::test]
    async fn reconcile_removes_only_its_own_entry_leaving_a_concurrent_archive_alone() {
        let (mut app, _tmp) = make_app();
        let finishing = crate::data::store::WorkspaceId(1);
        let still_running = crate::data::store::WorkspaceId(2);
        seed_archive_entry(&mut app, finishing);
        seed_archive_entry(&mut app, still_running);
        app.pending_archive_gen = Some(7);
        app.next_archive_gen = 8;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(shared.clone(), 7, finishing, Ok(SetupResult::Ok)).await;
        let g = shared.lock().await;
        assert!(
            !g.in_flight.contains_key(&finishing),
            "the finishing archive's own entry should be removed"
        );
        assert!(
            g.in_flight.contains_key(&still_running),
            "a concurrent, still-running archive's entry must survive"
        );
    }

    #[tokio::test]
    async fn reconcile_with_stale_gen_still_removes_its_own_entry_but_leaves_gen_alone() {
        let (mut app, _tmp) = make_app();
        let ws_id = crate::data::store::WorkspaceId(1);
        seed_archive_entry(&mut app, ws_id);
        // Simulate: pending_archive_gen has already advanced past the value
        // our (real, completed) archive task carries — e.g. a second archive
        // started after this one. The gen mismatch must not stop this task's
        // own in_flight entry from being cleaned up; it genuinely finished.
        app.pending_archive_gen = Some(99);
        app.next_archive_gen = 100;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(
            shared.clone(),
            7, // stale — does not match pending_archive_gen
            ws_id,
            Err(Error::Setup("ignored".into())),
        )
        .await;
        let g = shared.lock().await;
        assert!(
            !g.in_flight.contains_key(&ws_id),
            "a stale-gen reconcile must still remove its own in_flight entry"
        );
        assert_eq!(
            g.pending_archive_gen,
            Some(99),
            "a stale reconcile must not clear a newer pending_archive_gen"
        );
    }
}
