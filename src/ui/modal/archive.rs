//! Extracted from ui/modal.rs.

use super::*;

/// Render the 4-line body of the `ArchiveRunning` modal.
///
/// Each line is one phase of `workspace::archive_with_app`. The
/// `script_present` flag overrides the Script row's marker to
/// "— (skipped)" regardless of `step`, so a no-script repo never
/// shows the Script row spinning during the brief window where
/// `step == Script` and `run_archive` is returning `Skipped`.
pub(super) fn render_archive_steps(step: ArchiveStep, script_present: bool, tick: u32) -> String {
    let spinner = crate::ui::dashboard::spinner::frame(tick);

    // Per-row marker: '✓' done, spinner in-progress, '·' pending.
    // The script row gets a special '(skipped)' rendering when there
    // is no script configured.
    let script_line = if !script_present {
        "  — Archive script (skipped)".to_string()
    } else {
        let m = marker_for(step, ArchiveStep::Script, spinner);
        format!("  {m} Running archive script")
    };
    let worktree_line = {
        let m = marker_for(step, ArchiveStep::RemoveWorktree, spinner);
        format!("  {m} Removing worktree…")
    };
    let branch_line = {
        let m = marker_for(step, ArchiveStep::DeleteBranch, spinner);
        format!("  {m} Deleting branch")
    };
    let cleanup_line = {
        let m = marker_for(step, ArchiveStep::Cleanup, spinner);
        format!("  {m} Cleaning up registry")
    };

    format!("{script_line}\n{worktree_line}\n{branch_line}\n{cleanup_line}")
}

/// Pick the marker character for `row` given the currently running `current` step.
fn marker_for(current: ArchiveStep, row: ArchiveStep, spinner: char) -> char {
    use std::cmp::Ordering;
    match step_ordinal(row).cmp(&step_ordinal(current)) {
        Ordering::Less => '✓',
        Ordering::Equal => spinner,
        Ordering::Greater => '·',
    }
}

fn step_ordinal(s: ArchiveStep) -> u8 {
    match s {
        ArchiveStep::Script => 0,
        ArchiveStep::RemoveWorktree => 1,
        ArchiveStep::DeleteBranch => 2,
        ArchiveStep::Cleanup => 3,
    }
}

#[cfg(test)]
mod render_archive_steps_tests {
    use super::*;

    #[test]
    fn step_script_with_script_present_marks_script_in_progress() {
        let body = render_archive_steps(ArchiveStep::Script, true, 0);
        // Spinner frame for tick=0 is '⠋' (from spinner::frame tests).
        assert!(
            body.contains("⠋ Running archive script"),
            "body was:\n{body}"
        );
        assert!(body.contains("· Removing worktree"), "body was:\n{body}");
        assert!(body.contains("· Deleting branch"), "body was:\n{body}");
        assert!(body.contains("· Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn step_remove_worktree_marks_script_done_and_worktree_in_progress() {
        let body = render_archive_steps(ArchiveStep::RemoveWorktree, true, 0);
        assert!(
            body.contains("✓ Running archive script"),
            "body was:\n{body}"
        );
        assert!(body.contains("⠋ Removing worktree"), "body was:\n{body}");
        assert!(body.contains("· Deleting branch"), "body was:\n{body}");
        assert!(body.contains("· Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn step_cleanup_marks_everything_but_cleanup_done() {
        let body = render_archive_steps(ArchiveStep::Cleanup, true, 0);
        assert!(
            body.contains("✓ Running archive script"),
            "body was:\n{body}"
        );
        assert!(body.contains("✓ Removing worktree"), "body was:\n{body}");
        assert!(body.contains("✓ Deleting branch"), "body was:\n{body}");
        assert!(body.contains("⠋ Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn script_absent_renders_skipped_regardless_of_step() {
        // Even when step is still Script, no-script repos render
        // the Script row as (skipped) — never spinning.
        for step in [
            ArchiveStep::Script,
            ArchiveStep::RemoveWorktree,
            ArchiveStep::DeleteBranch,
            ArchiveStep::Cleanup,
        ] {
            let body = render_archive_steps(step, false, 0);
            assert!(
                body.contains("— Archive script (skipped)"),
                "step={step:?} body was:\n{body}"
            );
            assert!(
                !body.contains("⠋ Running archive script"),
                "script row should never spin when script_present=false; body was:\n{body}"
            );
        }
    }

    #[test]
    fn spinner_frame_varies_with_tick() {
        // The spinner glyph at tick=0 is '⠋'; at tick=8 it's '⠙'.
        // This sanity-checks that render_archive_steps actually
        // threads `tick` through to spinner::frame.
        let body0 = render_archive_steps(ArchiveStep::RemoveWorktree, true, 0);
        let body8 = render_archive_steps(ArchiveStep::RemoveWorktree, true, 8);
        assert!(body0.contains('⠋'));
        assert!(body8.contains('⠙'));
    }
}
