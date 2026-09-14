//! Drawing an attached view: the local split tree of PTY panes, the
//! ssh-attached remote pane, and the leader-key nav overlay.

use super::*;
use crate::app::App;

/// The local split tree of attached PTY panes.
pub(super) fn draw_attached(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    use crate::ui::attached;
    let crate::ui::View::Attached(state) = &app.view else {
        return;
    };
    // If any leaf's session has gone away (e.g. workspace was
    // archived from elsewhere), bounce back to dashboard. Matches
    // the previous single-pane fallback at handle_key_attached.
    if state
        .leaves()
        .iter()
        .any(|t| app.sessions.get(t.instance).is_none())
    {
        app.leader_pending = false;
        app.view = crate::ui::View::Dashboard;
        return;
    }
    let focused_target = match state.focused_target() {
        Some(t) => t,
        None => {
            app.leader_pending = false;
            app.view = crate::ui::View::Dashboard;
            return;
        }
    };
    let focused_id = focused_target.workspace_id;
    let (focused_repo, focused_name): (String, String) = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == focused_id)
        .map(|(_, w)| {
            let repo_name = app
                .repos
                .iter()
                .find(|r| r.id == w.repo_id)
                .map(|r| r.name.clone())
                .unwrap_or_default();
            (repo_name, w.name.clone())
        })
        .unwrap_or_default();
    let focused_label = if focused_repo.is_empty() {
        focused_name.clone()
    } else {
        format!("{focused_repo}/{focused_name}")
    };
    let focused_agent = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == focused_id)
        .map(|(_, w)| w.agent);

    // The attention items follow the bottom line's label prefix, so
    // shrink their width budget by the prefix and offset their click
    // rects by it too — `info_line_prefix_width` is the single source
    // of truth shared with the renderer.
    let prefix_w = attached::info_line_prefix_width(&focused_label, focused_agent) as usize;
    let max_width = (area.width as usize).saturating_sub(3 + prefix_w);
    let attention = if matches!(
        app.modal,
        Some(crate::ui::modal::Modal::UpdatesPanel { .. })
    ) {
        None
    } else {
        compute_attention_line(app, Some(focused_id), max_width)
    };

    // Pinned commands resolve against the FOCUSED pane's workspace.
    let global_pinned = app.store.get_setting("pinned_commands").ok().flatten();
    let repo_pinned = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == focused_id)
        .and_then(|(_, w)| {
            app.repos
                .iter()
                .find(|r| r.id == w.repo_id)
                .and_then(|r| r.pinned_commands.clone())
        });
    let pinned = crate::commands::pinned::resolve(global_pinned.as_deref(), repo_pinned.as_deref());

    // PR chip for the focused pane's workspace, drawn right-justified on
    // the chip row. Same `(lifecycle, number)` source the dashboard
    // detail header uses, so the chip text and click behaviour match.
    let pr = app.pr_number.get(&focused_id).copied().and_then(|n| {
        app.pr_lifecycle
            .get(&focused_id)
            .copied()
            .map(|lc| crate::ui::attached::ChipPr {
                lifecycle: lc,
                number: n,
                review: app.pr_review.get(&focused_id).copied(),
                unresolved: app.pr_unresolved.get(&focused_id).copied(),
            })
    });

    // Diff stats for the focused pane, drawn just left of the PR chip.
    // Same `app.workspace_diff` cache the dashboard `+N −N` cell reads,
    // so the chip-row count matches the dashboard and refreshes on the
    // same 10s diff poll as the agent makes commits.
    let diff = app.workspace_diff.get(&focused_id).copied();

    // Running-process count for the focused workspace, drawn in the chip
    // row's flush-right block. Same `app.workspace_processes`
    // map the dashboard row/detail bar count, so the chip-row `● Np`
    // matches them and refreshes on the same process-rescan tick.
    let procs = app
        .workspace_processes
        .get(&focused_id)
        .map(|v| v.len() as u32)
        .unwrap_or(0);

    let instances = app.store.workspace_agents(focused_id).unwrap_or_default();
    // Usage belongs to the focused agent, not its workspace. Only the primary
    // shares the dashboard's event cache; an untracked peer must not borrow it.
    let focused_events = if instances
        .iter()
        .any(|instance| instance.id == focused_target.instance && instance.is_primary)
    {
        app.workspace_events.get(&focused_id)
    } else {
        app.agent_events.get(&focused_target.instance)
    };
    let model_tokens = focused_events
        .and_then(crate::ui::detail_modules::session_summary::format_chip_model_tokens);

    // Build the agent pill list for the chip row's flush-right block. Only
    // shown when the focused workspace has more than its primary agent.
    let focused_agents_list: Vec<(
        crate::data::store::AgentInstanceId,
        crate::pty::session::AgentKind,
        String,
        Option<char>,
    )> = {
        if instances.len() > 1 {
            // Keys cap at 10 (see `agent_switch_keys`); agents past the
            // pool get `None` so they still render and stay clickable
            // rather than being silently dropped by a `zip`.
            let keys = attached::agent_switch_keys(instances.len());
            instances
                .into_iter()
                .enumerate()
                .map(|(i, inst)| (inst.id, inst.agent, inst.label(), keys.get(i).copied()))
                .collect()
        } else {
            Vec::new()
        }
    };
    let (info_area, separator_area, pane_area, chip_area) = attached::layout_chrome(area);

    let crate::ui::split::LayoutResult { panes, dividers } = state.layout(pane_area);
    let multi_pane = panes.len() > 1;

    // The agent instance in the focused pane is the "active" one; its chip
    // row pill thickens its identity bar so it's clear which attached agent
    // you're currently driving.
    let active_agent = panes
        .iter()
        .find(|(_, path, _)| *path == state.focus)
        .map(|(target, _, _)| target.instance);

    // Resize each session's PTY to its pane area (minus title row when multi-pane).
    for (target, _path, rect) in &panes {
        if let Some(session) = app.sessions.get(target.instance) {
            attached::resize_pane(&session, *rect, multi_pane);
        }
    }

    // Build PaneSpec list. Use owned sessions + labels to keep
    // them alive while rendering. The leaf carries the agent instance
    // directly; resolve the session from it and the label/agent kind
    // from the instance (falling back to the workspace name + agent).
    let pane_data: Vec<PaneData> = panes
        .into_iter()
        .filter_map(|(target, path, rect)| {
            let session = app.session_for(target.instance)?;
            let instance = app
                .store
                .workspace_agents_by_id(target.instance)
                .ok()
                .flatten();
            let (label, agent) = match instance {
                Some(inst) => (inst.label(), Some(inst.agent)),
                None => app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == target.workspace_id)
                    .map(|(_, w)| (w.name.clone(), Some(w.agent)))
                    .unwrap_or_default(),
            };
            let focused = path == state.focus;
            Some((session, label, rect, focused, agent))
        })
        .collect();
    let specs: Vec<crate::ui::attached::PaneSpec<'_>> = pane_data
        .iter()
        .map(|(s, l, r, f, a)| crate::ui::attached::PaneSpec {
            session: s,
            label: l.as_str(),
            rect: *r,
            focused: *f,
            agent: *a,
        })
        .collect();

    let out = attached::render_panes(
        f,
        &specs,
        &dividers,
        info_area,
        separator_area,
        chip_area,
        &app.bar_specs,
        &focused_repo,
        &focused_name,
        focused_agent,
        attention,
        &pinned,
        procs,
        diff,
        pr,
        model_tokens,
        &focused_agents_list,
        active_agent,
        &app.theme,
    );
    app.chip_rects = out.chip_rects;
    app.pr_link_rect = out.pr_link_rect.map(|r| (focused_id, r));
    app.procs_link_rect = out.procs_link_rect.map(|r| (focused_id, r));
    app.attention_rects = out.attention_rects;
    app.attention_more_rect = out.attention_more_rect;
    app.attached_pane_rects = out.pane_rects;
    app.agent_chip_rects = out.agent_chip_rects;
    app.footer_hint_rects = out.footer_hint_rects;
    app.pinned_commands_cache = pinned;
}

/// The single full-screen pane of an ssh-attached remote workspace.
pub(super) fn draw_attached_remote(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
    use crate::ui::attached;
    if let Some(session) = app.remote.as_ref() {
        let label = app
            .remote_target
            .as_ref()
            .map(|t| format!("{}/{}", t.host_name, t.tmux))
            .unwrap_or_else(|| "remote".to_string());
        // A remote attach is just an `ssh -t … tmux attach` PTY stream,
        // so the host never ships the workspace's live process/diff/model
        // stats — those stay off. But two chip-row elements ARE reachable
        // locally and worth showing:
        //   - the GLOBAL pinned commands (`resolve(global, None)`): they
        //     dispatch by writing bytes into the focused PTY, which here
        //     is the ssh hop into the remote tmux, so they drive the
        //     remote agent just like a local pane. Repo-scoped pins are
        //     skipped — we don't know the remote workspace's repo config.
        //   - the PR chip, recovered from the retained `remote_list`
        //     record whose agent owns the tmux session we attached to
        //     (the same `lifecycle`/`pr_number` the H picker colors by).
        let global_pinned = app.store.get_setting("pinned_commands").ok().flatten();
        let pinned = crate::commands::pinned::resolve(global_pinned.as_deref(), None);
        let pr = app.remote_list.as_ref().and_then(|list| {
            let tmux = app.remote_target.as_ref()?.tmux.as_str();
            list.records
                .iter()
                .find(|rec| {
                    rec.agents
                        .iter()
                        .any(|a| a.tmux_session.as_deref() == Some(tmux))
                })
                .and_then(|rec| {
                    // The shared-workspace wire contract carries no
                    // review verdict, so a remote pane's chip stays
                    // unmarked rather than claiming "not gated".
                    rec.pr_number.and_then(|n| {
                        rec.lifecycle.map(|lc| crate::ui::attached::ChipPr {
                            lifecycle: lc,
                            number: n,
                            review: None,
                            unresolved: None,
                        })
                    })
                })
        });
        let (info_area, separator_area, pane_area, chip_area) = attached::layout_chrome(area);
        attached::resize_pane(session, pane_area, false);
        let specs = [crate::ui::attached::PaneSpec {
            session,
            label: &label,
            rect: pane_area,
            focused: true,
            agent: None,
        }];
        let out = attached::render_panes(
            f,
            &specs,
            &[],
            info_area,
            separator_area,
            chip_area,
            &app.bar_specs,
            "",
            &label,
            None,
            None,
            &pinned,
            0,
            None,
            pr,
            None,
            &[],
            None,
            &app.theme,
        );
        app.attached_pane_rects = out.pane_rects;
        app.footer_hint_rects = out.footer_hint_rects;
        app.chip_rects = out.chip_rects;
        app.pinned_commands_cache = pinned;
        // The PR chip renders but isn't clickable: opening a PR keys off a
        // local WorkspaceId we don't have for a remote workspace, so
        // `out.pr_link_rect` is deliberately dropped. The other hit-test
        // state the remote frame doesn't populate (pr/procs/agent/
        // attention rects) is already reset by `draw()` at frame start, so
        // no stale target from a prior local Attached frame survives here.
    } else {
        // ssh client went away; bounce to dashboard on next event.
        app.leader_pending = false;
        app.view = crate::ui::View::Dashboard;
    }
}

/// One attached pane's render inputs: session, label, rect, focus flag,
/// and the workspace's coding agent (`None` when the agent kind can't be
/// resolved).
pub(super) type PaneData = (
    std::sync::Arc<crate::pty::session::Session>,
    String,
    ratatui::layout::Rect,
    bool,
    Option<crate::pty::session::AgentKind>,
);

/// Render the Ctrl-x navigation overlay when the leader is armed in an
/// attached view. Keyed off `leader_pending`, so letter accelerators and the
/// overlay share one state. Context (single vs multi-pane) selects the item
/// list.
pub(super) fn draw_attached_nav_overlay(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
) {
    if !app.leader_pending {
        return;
    }
    let (items, pinned_hint) = match &app.view {
        crate::ui::View::Attached(state) => (
            crate::ui::attached::nav_menu_items(state.leaf_count() > 1),
            !app.pinned_commands_cache.is_empty(),
        ),
        _ => return,
    };
    crate::ui::attached::render_nav_overlay(
        f,
        area,
        &items,
        app.leader_selected,
        pinned_hint,
        &app.theme,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::events::WorkspaceEvents;
    use crate::data::store::Store;
    use crate::pty::session::{AgentKind, SessionStatus};
    use crate::ui::split::AttachTarget;
    use crate::ui::{AttachedState, View};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn footer(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(200, 24)).unwrap();
        terminal.draw(|f| draw_attached(f, app, f.area())).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer[(x, buffer.area.height - 1)].symbol())
            .collect()
    }

    #[test]
    fn attached_footer_tracks_agent_switches_and_split_focus() {
        let mut app = App::new(
            Store::open_in_memory().unwrap(),
            std::path::PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap();
        let workspace_id = app.test_workspace("footer-untracked");
        let primary = app
            .store
            .add_primary_agent(workspace_id, AgentKind::Claude, 1)
            .unwrap()
            .id;
        let peer = app
            .store
            .add_workspace_agent(workspace_id, AgentKind::Claude)
            .unwrap();
        app.test_spawn_session(primary, SessionStatus::Running { pid: 1 });
        app.test_spawn_session(peer.id, SessionStatus::Running { pid: 2 });
        app.workspace_events.insert(
            workspace_id,
            WorkspaceEvents {
                model_id: Some("claude-opus-4-8".into()),
                context_tokens: Some(45_000),
                ..Default::default()
            },
        );
        app.view = View::Attached(AttachedState::single(AttachTarget {
            workspace_id,
            instance: primary,
        }));
        let primary_footer = footer(&mut app);
        assert!(primary_footer.contains("opus 4.8"), "{primary_footer}");
        assert!(primary_footer.contains("45k/200k"), "{primary_footer}");

        app.switch_focused_pane_to(peer.id).unwrap();
        let peer_footer = footer(&mut app);
        assert!(!peer_footer.contains("opus 4.8"), "{peer_footer}");
        assert!(!peer_footer.contains("45k/200k"), "{peer_footer}");

        app.agent_events.insert(
            peer.id,
            WorkspaceEvents {
                model_id: Some("claude-sonnet-4-5".into()),
                context_tokens: Some(90_000),
                ..Default::default()
            },
        );
        let peer_footer = footer(&mut app);
        assert!(peer_footer.contains("sonnet 4.5"), "{peer_footer}");
        assert!(peer_footer.contains("90k/200k"), "{peer_footer}");
        assert!(!peer_footer.contains("opus 4.8"), "{peer_footer}");

        // Splitting focuses the new pane. Both agents remain visible, but the
        // shared footer must follow focus rather than the workspace or first leaf.
        let View::Attached(state) = &mut app.view else {
            panic!("expected attached view");
        };
        state.split(
            crate::ui::split::SplitDirection::Horizontal,
            AttachTarget {
                workspace_id,
                instance: primary,
            },
        );
        let primary_footer = footer(&mut app);
        assert!(primary_footer.contains("opus 4.8"), "{primary_footer}");
        assert!(primary_footer.contains("45k/200k"), "{primary_footer}");
        assert!(!primary_footer.contains("sonnet 4.5"), "{primary_footer}");

        app.switch_focused_pane_to(peer.id).unwrap();
        let peer_footer = footer(&mut app);
        assert!(peer_footer.contains("sonnet 4.5"), "{peer_footer}");
        assert!(peer_footer.contains("90k/200k"), "{peer_footer}");
        assert!(!peer_footer.contains("45k/200k"), "{peer_footer}");

        let codex = app
            .store
            .add_workspace_agent(workspace_id, AgentKind::Codex)
            .unwrap();
        app.test_spawn_session(codex.id, SessionStatus::Running { pid: 3 });
        app.agent_events.insert(
            codex.id,
            WorkspaceEvents {
                model_id: Some("gpt-5-codex".into()),
                context_tokens: Some(77_000),
                ..Default::default()
            },
        );
        app.switch_focused_pane_to(codex.id).unwrap();
        let codex_footer = footer(&mut app);
        assert!(codex_footer.contains("gpt-5-codex"), "{codex_footer}");
        assert!(codex_footer.contains("77k"), "{codex_footer}");
        assert!(!codex_footer.contains("90k/200k"), "{codex_footer}");
    }
}
