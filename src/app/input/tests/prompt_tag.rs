//! The prompt-tag modal: pick → body → insert, and the manager keys.

use super::common::*;
use super::*;
use crate::commands::tags::{self, PromptTag};
use crate::data::store::Store;
use crate::ui::modal::{Modal, PromptTagModal, TagStage};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

async fn type_str(app: &mut App, shared: &SharedApp, s: &str) {
    for c in s.chars() {
        handle_key_modal(app, shared, key(KeyCode::Char(c)))
            .await
            .unwrap();
    }
}

fn modal(app: &App) -> PromptTagModal {
    match &app.modal {
        Some(Modal::PromptTag(m)) => m.clone(),
        other => panic!("expected the prompt-tag modal, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pick_enter_on_a_new_name_creates_it_and_opens_the_body() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    type_str(&mut app, &shared, "context").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Enter))
        .await
        .unwrap();
    assert_eq!(
        modal(&app).stage,
        TagStage::Body {
            name: "context".into()
        }
    );
    assert_eq!(
        tags::load(&app.store).unwrap(),
        vec![PromptTag {
            name: "context".into(),
            uses: 0
        }],
        "a created tag is persisted at zero uses"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pick_enter_on_an_invalid_name_does_nothing() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    type_str(&mut app, &shared, "1bad").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Enter))
        .await
        .unwrap();
    assert_eq!(modal(&app).stage, TagStage::Pick);
    assert!(tags::load(&app.store).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pick_digit_jumps_to_the_nth_tag_only_while_the_field_is_empty() {
    let store = Store::open_in_memory().unwrap();
    tags::save(
        &store,
        &[
            PromptTag {
                name: "context".into(),
                uses: 5,
            },
            PromptTag {
                name: "task".into(),
                uses: 2,
            },
        ],
    )
    .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    handle_key_modal(&mut app, &shared, key(KeyCode::Char('2')))
        .await
        .unwrap();
    assert_eq!(
        modal(&app).stage,
        TagStage::Body {
            name: "task".into()
        }
    );

    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    type_str(&mut app, &shared, "t2").await;
    assert_eq!(modal(&app).stage, TagStage::Pick);
    assert_eq!(
        modal(&app).name_field,
        "t2",
        "digits are text once the field has content"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_d_deletes_the_selected_tag_and_persists() {
    let store = Store::open_in_memory().unwrap();
    tags::save(
        &store,
        &[
            PromptTag {
                name: "context".into(),
                uses: 5,
            },
            PromptTag {
                name: "task".into(),
                uses: 2,
            },
        ],
    )
    .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    handle_key_modal(&mut app, &shared, key(KeyCode::Down))
        .await
        .unwrap();
    handle_key_modal(&mut app, &shared, ctrl('d'))
        .await
        .unwrap();
    assert_eq!(
        tags::load(&app.store).unwrap(),
        vec![PromptTag {
            name: "context".into(),
            uses: 5
        }]
    );
    assert_eq!(modal(&app).selected, 0, "selection clamps after the delete");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_inserts_the_wrapped_text_bumps_the_count_and_closes() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    let ws_id = spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "line one").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Enter))
        .await
        .unwrap();
    type_str(&mut app, &shared, "line two").await;
    handle_key_modal(&mut app, &shared, ctrl('s'))
        .await
        .unwrap();
    assert!(app.modal.is_none(), "insert closes the modal");
    assert_eq!(
        tags::load(&app.store).unwrap(),
        vec![PromptTag {
            name: "context".into(),
            uses: 1
        }]
    );

    // The session is `cat`, which echoes what it receives; the paste
    // markers are CSI sequences vt100 swallows, leaving the tag lines.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let screen = session.parser.lock().unwrap().screen().contents();
    assert!(screen.contains("<context>"), "{screen:?}");
    assert!(screen.contains("line one"), "{screen:?}");
    assert!(screen.contains("line two"), "{screen:?}");
    assert!(screen.contains("</context>"), "{screen:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_with_a_blank_body_stays_open_and_records_nothing() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "   ").await;
    handle_key_modal(&mut app, &shared, ctrl('s'))
        .await
        .unwrap();
    assert!(matches!(app.modal, Some(Modal::PromptTag(_))));
    assert!(tags::load(&app.store).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_esc_returns_to_pick_keeping_the_draft() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "draft").await;
    handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();
    let m = modal(&app);
    assert_eq!(m.stage, TagStage::Pick);
    assert_eq!(m.body.text(), "draft");
    handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();
    assert!(app.modal.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pasting_into_the_body_keeps_tabs_verbatim() {
    // The non-attached (modal-open) paste fallback feeds each pasted char
    // through `paste_char_to_key`, which maps '\t' to `KeyCode::Tab`; the
    // body box must have a `Tab` arm or the char vanishes.
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    handle_paste(&mut app, &shared, "a\tb".into())
        .await
        .unwrap();
    assert_eq!(modal(&app).body.text(), "a\tb");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_without_a_session_reports_an_error() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    // Dashboard view: no focused pane to insert into.
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "x").await;
    handle_key_modal(&mut app, &shared, ctrl('s'))
        .await
        .unwrap();
    assert!(
        matches!(&app.modal, Some(Modal::Error { message }) if message.contains("no running agent")),
        "{:?}",
        app.modal
    );
    assert!(tags::load(&app.store).unwrap().is_empty());
}
