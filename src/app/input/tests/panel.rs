//! The bottom panel's summon rules (`app::input::panel`), across every panel.

use super::common::*;
use super::*;
use crate::ui::modal::Modal;
use crossterm::event::{KeyEvent, KeyModifiers};

/// `test_app_with_two_ready_workspaces` with the first workspace selected.
fn app_on_workspace() -> (App, crate::data::store::WorkspaceId) {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let ws = app.workspaces[0].1.id;
    let idx = app
        .selectable
        .iter()
        .position(|t| *t == crate::app::SelectionTarget::Workspace(ws))
        .unwrap_or_else(|| {
            app.selectable
                .push(crate::app::SelectionTarget::Workspace(ws));
            app.selectable.len() - 1
        });
    app.select_index(idx);
    (app, ws)
}

async fn modal_key(app: &mut App, k: KeyEvent) {
    let shared = shared_app();
    handle_key_modal(app, &shared, k).await.unwrap();
}

async fn modal_char(app: &mut App, c: char) {
    modal_key(app, key(KeyCode::Char(c))).await;
}

async fn ctrl_x(app: &mut App) {
    modal_key(
        app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await;
}

#[tokio::test]
async fn every_summon_key_replaces_a_hide_rules_card() {
    for (c, opened) in [
        (
            '#',
            (|m: &Modal| matches!(m, Modal::WorkspaceTags { .. })) as fn(&Modal) -> bool,
        ),
        ('?', |m| matches!(m, Modal::WorkspaceActions)),
        ('s', |m| matches!(m, Modal::RepoSettings { .. })),
        ('C', |m| matches!(m, Modal::NameColorPicker { .. })),
        ('K', |m| matches!(m, Modal::ProcessList { .. })),
    ] {
        let (mut app, _) = app_on_workspace();
        press(&mut app, 'F', KeyModifiers::SHIFT).await;
        assert!(matches!(app.modal, Some(Modal::HideRules { .. })));
        modal_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT),
        )
        .await;
        assert!(
            app.modal.as_ref().is_some_and(opened),
            "{c} should replace the hide-rules card, got {:?}",
            app.modal
        );
    }
}

#[tokio::test]
async fn own_key_closes_and_other_keys_chain() {
    let (mut app, _) = app_on_workspace();
    press(&mut app, 's', KeyModifiers::NONE).await;
    modal_char(&mut app, 'F').await;
    assert!(matches!(app.modal, Some(Modal::HideRules { .. })));
    modal_char(&mut app, 'K').await;
    assert!(matches!(app.modal, Some(Modal::ProcessList { .. })));
    // `K` is the process list's SIGKILL verb, so it does not toggle it.
    modal_char(&mut app, 'K').await;
    assert!(matches!(app.modal, Some(Modal::ProcessList { .. })));
    modal_char(&mut app, '?').await;
    assert!(matches!(app.modal, Some(Modal::WorkspaceActions)));
    modal_char(&mut app, '?').await;
    assert!(app.modal.is_none());
}

#[tokio::test]
async fn a_card_taking_text_keeps_summon_keys_as_text() {
    let (mut app, _) = app_on_workspace();
    press(&mut app, '#', KeyModifiers::SHIFT).await;
    modal_char(&mut app, 'F').await;
    modal_char(&mut app, 's').await;
    let Some(Modal::WorkspaceTags { input, .. }) = &app.modal else {
        panic!("tag editor should stay open, got {:?}", app.modal);
    };
    assert_eq!(input, "Fs");
}

#[tokio::test]
async fn a_summon_key_replaces_a_confirmation() {
    let (mut app, _) = app_on_workspace();
    press(&mut app, 'd', KeyModifiers::NONE).await;
    assert!(matches!(app.modal, Some(Modal::ConfirmArchive { .. })));
    modal_char(&mut app, 'F').await;
    assert!(matches!(app.modal, Some(Modal::HideRules { .. })));
}

#[tokio::test]
async fn leader_chords_swap_panels_over_a_dashboard_card() {
    let (mut app, _) = app_on_workspace();
    press(&mut app, 'F', KeyModifiers::SHIFT).await;
    ctrl_x(&mut app).await;
    assert!(app.leader_pending);
    modal_char(&mut app, 'a').await;
    assert!(!app.leader_pending);
    assert!(matches!(app.modal, Some(Modal::AgentsPanel { .. })));
    // `a` alone is the agents panel's own add-agent verb, not a summon.
    ctrl_x(&mut app).await;
    modal_char(&mut app, 'a').await;
    assert!(app.modal.is_none(), "the chord that opened it closes it");
}

#[tokio::test]
async fn leader_chords_swap_panels_over_an_attached_pane() {
    let (mut app, ws) = app_on_workspace();
    app.view = crate::ui::View::Attached(AttachedState::single(test_target(&app, ws)));
    app.modal = Some(Modal::UpdatesPanel {
        selected: 0,
        filter: None,
    });
    ctrl_x(&mut app).await;
    modal_char(&mut app, 'k').await;
    assert!(
        matches!(app.modal, Some(Modal::ProcessList { workspace_id, .. }) if workspace_id == ws)
    );
    ctrl_x(&mut app).await;
    modal_char(&mut app, '<').await;
    assert!(matches!(app.modal, Some(Modal::PromptTag(_))));
    // The prompt tag takes text, but a chord still reaches past it.
    ctrl_x(&mut app).await;
    modal_char(&mut app, 'u').await;
    assert!(matches!(app.modal, Some(Modal::UpdatesPanel { .. })));
    // A key that is no chord disarms the leader and leaves the panel up.
    ctrl_x(&mut app).await;
    modal_key(&mut app, key(KeyCode::Esc)).await;
    assert!(!app.leader_pending);
    assert!(matches!(app.modal, Some(Modal::UpdatesPanel { .. })));
}

#[tokio::test]
async fn clicking_a_panel_away_abandons_an_armed_chord() {
    let (mut app, _) = app_on_workspace();
    press(&mut app, 'C', KeyModifiers::SHIFT).await;
    ctrl_x(&mut app).await;
    assert!(app.leader_pending);
    // No swatch rects were laid out, so the click lands outside the grid.
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        mouse_event(crossterm::event::MouseEventKind::Down(
            crossterm::event::MouseButton::Left,
        )),
    )
    .await;
    assert!(app.modal.is_none());
    assert!(!app.leader_pending, "the chord must not outlive the panel");
    press(&mut app, 'a', KeyModifiers::NONE).await;
    assert!(app.modal.is_none(), "a bare `a` is not `Ctrl-x a`");
}

#[tokio::test]
async fn in_flight_cards_keep_summon_keys_and_chords() {
    let (mut app, _) = app_on_workspace();
    for card in [
        Modal::RemoteListLoading {
            host_name: "h".into(),
        },
        Modal::RemoteShareRunning {
            selected: 0,
            host_name: "h".into(),
            workspace: "w".into(),
        },
        Modal::RemoteReposLoading {
            selected: 0,
            host_name: "h".into(),
            yolo: false,
            repo: None,
        },
    ] {
        app.modal = Some(card.clone());
        modal_char(&mut app, 'F').await;
        ctrl_x(&mut app).await;
        modal_char(&mut app, 'a').await;
        assert!(!app.leader_pending);
        assert_eq!(
            std::mem::discriminant(app.modal.as_ref().unwrap()),
            std::mem::discriminant(&card),
            "{card:?} must stay up"
        );
    }
}

#[tokio::test]
async fn host_picker_opens_from_a_panel_and_toggles_on_its_key() {
    let (mut app, _) = app_on_workspace();
    app.store
        .set_setting("shared_hosts", "box=user@box")
        .unwrap();
    press(&mut app, 'F', KeyModifiers::SHIFT).await;
    modal_char(&mut app, 'H').await;
    assert!(matches!(app.modal, Some(Modal::RemoteHostPicker { .. })));
    modal_char(&mut app, 'H').await;
    assert!(app.modal.is_none());
}
