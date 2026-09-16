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
async fn body_ctrl_s_folds_the_bump_into_a_sibling_writers_value() {
    // A sibling process (or another wsx) rewrites the row between this
    // modal's read and its ctrl-s write. The bump must land on top of
    // THAT value, not stomp it, because the mutation now goes through
    // `tags::update`'s read-modify-write transaction rather than a
    // load/bump/save sequence.
    let store = Store::open_in_memory().unwrap();
    tags::save(
        &store,
        &[PromptTag {
            name: "context".into(),
            uses: 1,
        }],
    )
    .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "x").await;
    app.store
        .conn()
        .execute(
            "UPDATE settings SET value = ?1 WHERE key = ?2",
            rusqlite::params!["theirs=5\ncontext=1\n", tags::SETTING_KEY],
        )
        .unwrap();
    handle_key_modal(&mut app, &shared, ctrl('s'))
        .await
        .unwrap();
    assert_eq!(
        tags::load(&app.store).unwrap(),
        vec![
            PromptTag {
                name: "theirs".into(),
                uses: 5
            },
            PromptTag {
                name: "context".into(),
                uses: 2
            },
        ]
    );
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
async fn pasting_crlf_into_the_body_normalizes_to_a_single_newline() {
    // `paste_char_to_key` maps both '\r' and '\n' to Enter, so the per-char
    // fallback would double every CRLF line break into two blank-separated
    // lines. The body stage takes a paste as text (CRLF folded to LF), and
    // `handle_paste` folds CRLF once more before the per-char path for
    // every other modal.
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    handle_paste(&mut app, &shared, "a\r\nb\r\n".into())
        .await
        .unwrap();
    assert_eq!(modal(&app).body.text(), "a\nb\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pasting_crlf_on_the_per_char_path_is_one_enter() {
    // The pick stage has no text-paste shortcut, so it exercises the
    // per-char fallback: "a\r\nb" must be `a`, ONE Enter (creating the
    // tag and opening its body), then `b` into the body — not a second
    // Enter that would leave a blank first body line.
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    spawn_attached_workspace(&mut app);
    app.modal = Some(Modal::PromptTag(PromptTagModal::pick()));
    handle_paste(&mut app, &shared, "a\r\nb".into())
        .await
        .unwrap();
    let m = modal(&app);
    assert_eq!(m.stage, TagStage::Body { name: "a".into() });
    assert_eq!(m.body.text(), "b");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_without_a_session_keeps_the_draft_and_shows_a_notice() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    // Dashboard view: no focused pane to insert into.
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "x").await;
    handle_key_modal(&mut app, &shared, ctrl('s'))
        .await
        .unwrap();
    let m = modal(&app);
    assert_eq!(
        m.stage,
        TagStage::Body {
            name: "context".into()
        },
        "the modal stays open on the body stage"
    );
    assert_eq!(m.body.text(), "x", "the draft survives");
    assert!(
        m.notice
            .as_deref()
            .is_some_and(|n| n.contains("no running agent")),
        "{:?}",
        m.notice
    );
    assert!(tags::load(&app.store).unwrap().is_empty());

    // The next key clears the notice before it is handled.
    type_str(&mut app, &shared, "y").await;
    let m = modal(&app);
    assert_eq!(m.notice, None);
    assert_eq!(m.body.text(), "xy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_ctrl_s_with_a_dead_writer_keeps_the_draft_and_does_not_bump() {
    // `insert_fake_session` registers a session whose writer receiver is
    // already dropped — the same closed-channel state as an exited agent —
    // so `insert_text` reports `false`. The draft must survive with an
    // inline notice, and no use may be recorded for text that never landed.
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared_app();
    let ws_id = spawn_attached_workspace(&mut app);
    let inst = test_primary_instance(&app, ws_id);
    app.sessions.remove(inst);
    app.sessions
        .insert_fake_session(inst, crate::pty::session::SessionStatus::Running { pid: 1 });
    app.modal = Some(Modal::PromptTag(PromptTagModal::for_tag("context")));
    type_str(&mut app, &shared, "draft").await;
    handle_key_modal(&mut app, &shared, ctrl('s'))
        .await
        .unwrap();
    let m = modal(&app);
    assert_eq!(
        m.stage,
        TagStage::Body {
            name: "context".into()
        }
    );
    assert_eq!(m.body.text(), "draft");
    assert!(
        m.notice
            .as_deref()
            .is_some_and(|n| n.contains("insert not confirmed")),
        "{:?}",
        m.notice
    );
    assert!(
        tags::load(&app.store).unwrap().is_empty(),
        "no use is counted for an insert that was not confirmed"
    );
}
