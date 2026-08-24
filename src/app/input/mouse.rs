//! Mouse events: clicks, wheel, and the hit-testing that maps a cell
//! back to the pane, container, or row under the cursor.

use super::*;
use crate::app::{App, attach_workspace};
use crate::ui::modal::Modal;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

/// Whether a click landed inside `r`.
pub(in crate::app::input) fn rect_contains(
    r: &ratatui::layout::Rect,
    m: &crossterm::event::MouseEvent,
) -> bool {
    m.column >= r.x
        && m.column < r.x.saturating_add(r.width)
        && m.row >= r.y
        && m.row < r.y.saturating_add(r.height)
}

/// Returns the slot index of the detail-bar container under (col, row),
/// or None if no container rect matches.
pub(in crate::app::input) fn container_under_cursor(
    app: &App,
    col: u16,
    row: u16,
) -> Option<usize> {
    app.detail_container_rects
        .iter()
        .enumerate()
        .find_map(|(i, slot)| {
            let r = slot.as_ref()?;
            let in_rect = col >= r.x
                && col < r.x.saturating_add(r.width)
                && row >= r.y
                && row < r.y.saturating_add(r.height);
            if in_rect { Some(i) } else { None }
        })
}

/// Returns the `(session, rect)` of the attached-view pane under (col, row),
/// or None when the cursor is over chrome / no pane. Mirrors
/// `container_under_cursor`'s saturating bounds check.
pub(in crate::app::input) fn pane_under_cursor(
    app: &App,
    col: u16,
    row: u16,
) -> Option<(
    std::sync::Arc<crate::pty::session::Session>,
    ratatui::layout::Rect,
)> {
    app.attached_pane_rects.iter().find_map(|(session, r)| {
        let in_rect = col >= r.x
            && col < r.x.saturating_add(r.width)
            && row >= r.y
            && row < r.y.saturating_add(r.height);
        if in_rect {
            Some((std::sync::Arc::clone(session), *r))
        } else {
            None
        }
    })
}

/// Bump the scroll offset for container `slot` by `delta` rows. Clamped
/// to [0, u16::MAX] here; the next draw clamps further to the actual
/// content height in `render_container`.
pub(in crate::app::input) fn adjust_detail_scroll(
    app: &mut App,
    slot: usize,
    delta: u16,
    up: bool,
) {
    if slot >= app.detail_scroll_offsets.len() {
        return;
    }
    let cur = app.detail_scroll_offsets[slot];
    app.detail_scroll_offsets[slot] = if up {
        cur.saturating_sub(delta)
    } else {
        cur.saturating_add(delta)
    };
}

pub(in crate::app::input) async fn handle_mouse(app: &mut App, m: MouseEvent) {
    // Detail-bar container scroll: consume wheel events on the Dashboard
    // view when the cursor is over a container rect. Fall through for
    // wheel events elsewhere (existing scroll_active routing).
    if matches!(
        m.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && matches!(app.view, crate::ui::View::Dashboard)
    {
        if let Some(slot) = container_under_cursor(app, m.column, m.row) {
            let up = matches!(m.kind, MouseEventKind::ScrollUp);
            adjust_detail_scroll(app, slot, 3, up);
            return;
        }
    }

    // Attached view: a plain wheel over a pane whose agent has mouse
    // reporting on is forwarded to that agent's PTY so it scrolls its own
    // view (notably its full-screen UI, where wsx has no scrollback).
    // Shift+wheel, panes without mouse mode, and scrolls over chrome all
    // fall through to `scroll_active` (wsx scrollback) below.
    if matches!(
        m.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && matches!(
        app.view,
        crate::ui::View::Attached(_) | crate::ui::View::AttachedRemote
    ) && !m.modifiers.contains(KeyModifiers::SHIFT)
    {
        if let Some((session, rect)) = pane_under_cursor(app, m.column, m.row) {
            let up = matches!(m.kind, MouseEventKind::ScrollUp);
            let rel_col = m.column.saturating_sub(rect.x).saturating_add(1);
            let rel_row = m.row.saturating_sub(rect.y).saturating_add(1);
            if let Some(bytes) = session.wheel_report_bytes(up, rel_col, rel_row) {
                let _ = session
                    .writer
                    .send(crate::pty::session::WriteReq::Bytes(bytes))
                    .await;
                return;
            }
        }
    }

    match m.kind {
        MouseEventKind::ScrollUp => scroll_active(app, 3, true),
        MouseEventKind::ScrollDown => scroll_active(app, 3, false),
        MouseEventKind::Down(MouseButton::Left) => {
            // If the usage-window picker is open, a click either applies the
            // option under the cursor or dismisses the picker (click-outside).
            if matches!(app.modal, Some(Modal::UsageWindowPicker { .. })) {
                if let Some(idx) = app.usage_window_option_rects.iter().position(|r| {
                    m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height)
                }) {
                    let win = crate::config::usage_window::UsageWindow::from_index(idx);
                    if let Err(e) = app
                        .store
                        .set_setting("usage_graph_window", win.as_setting())
                    {
                        tracing::warn!(error = %e, "failed to persist usage_graph_window");
                    }
                }
                app.modal = None;
                return;
            }

            // The name-color picker: a click either applies the swatch under
            // the cursor or dismisses the picker (click-outside), mirroring
            // the usage-window picker above.
            if let Some(Modal::NameColorPicker { workspace_id, .. }) = app.modal {
                let hit = app.name_color_swatch_rects.iter().find_map(|(idx, r)| {
                    let inside = m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height);
                    inside.then_some(*idx)
                });
                match hit {
                    Some(idx) => {
                        if let Err(e) = apply_name_color(app, workspace_id, Some(idx)) {
                            tracing::warn!(error = %e, "failed to apply clicked name color");
                        }
                    }
                    None => app.modal = None,
                }
                return;
            }

            // Any other open modal swallows left clicks wholesale: the click
            // rects below belong to chrome the modal is overlaying, and firing
            // them "through" it mutates state the modal assumes stable (e.g.
            // attaching mid-`RemoteListLoading`). One gate here instead of
            // per-arm checks so future click targets can't reopen the hole.
            if app.modal.is_some() {
                return;
            }

            // Footer keybind hint click → behave exactly like pressing the
            // printed key. The footer row doesn't overlap any other click
            // target, so this is checked first and returns early.
            if let Some(action) = app.footer_hint_rects.iter().find_map(|(r, a)| {
                let hit = m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height);
                hit.then_some(*a)
            }) {
                dispatch_footer_hint(app, action).await;
                return;
            }

            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                fire_chip(app, idx).await;
            } else if let Some((ws_id, _)) = app.attention_rects.iter().copied().find(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking an attention entry attaches to that workspace,
                // identical to `Enter` on the dashboard.
                if let Err(e) = attach_workspace(app, ws_id) {
                    tracing::warn!(error = %e, "failed to attach from attention click");
                }
            } else if let Some((inst, _)) = app.agent_chip_rects.iter().copied().find(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking an agent pill retargets the focused pane to that
                // instance, spawning its session if needed.
                if let Err(e) = app.switch_focused_pane_to(inst) {
                    tracing::warn!(error = %e, "failed to switch pane from agent-pill click");
                }
            } else if let Some((ws_id, _)) = app.pr_link_rect.filter(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking the PR chip opens the PR in the browser.
                open_pr_for_workspace(app, ws_id);
            } else if let Some((ws_id, _)) =
                app.dashboard_pr_rects.iter().copied().find(|(_, r)| {
                    m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height)
                })
            {
                // Clicking a row's PR chip in the dashboard PR column opens
                // that PR in the browser, same as the detail-bar chip.
                open_pr_for_workspace(app, ws_id);
            } else if let Some(repo_path) = repo_pr_link_target(app, &m) {
                // Clicking a repo header's PR link opens that repo's open
                // PRs filtered to the signed-in user — the by-hand route of
                // "PRs tab, then filter by me", in one click.
                crate::git::forge::open_author_prs_in_browser(&repo_path);
            } else if let Some((ws_id, _)) = app.procs_link_rect.filter(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking the running-process count opens the process-list
                // modal for that workspace, mirroring `K` on it.
                app.modal = Some(Modal::ProcessList {
                    workspace_id: ws_id,
                    selected: 0,
                    input: None,
                    notice: None,
                });
            } else if app.usage_graph_rect.is_some_and(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking the footer activity graph opens the window picker,
                // seeded with the currently-applied window.
                let current = crate::config::usage_window::resolve(&app.store);
                app.modal = Some(Modal::UsageWindowPicker {
                    selected: current.index(),
                });
            }
        }
        _ => {}
    }
}
