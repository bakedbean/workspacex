//! Key encoding, paste bracketing, and scrollback reset on input.

use super::*;
use crate::data::store::Store;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use super::common::*;
use crossterm::event::KeyModifiers;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keystroke_to_pty_resets_scrollback() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);
    app.sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap()
        .scroll_up(20);
    assert!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .is_scrolled()
    );
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        !app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .is_scrolled(),
        "any keystroke flowing to PTY must snap to live"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leader_keystroke_does_not_reset_scrollback() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);
    app.sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap()
        .scroll_up(20);
    // Ctrl-x is the leader. It's consumed by wsx and never reaches the PTY.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending);
    assert!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .is_scrolled(),
        "leader key consumed by wsx; offset should be preserved"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arrow_key_resets_scrollback_and_forwards_to_pty() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);
    app.sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap()
        .scroll_up(20);
    // Up arrow flows to the PTY (Claude Code prompt history) — must
    // also snap scrollback back to live.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        !app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .is_scrolled()
    );
}

#[test]
fn wrap_paste_bytes_wraps_with_bracketed_markers() {
    let out = wrap_paste_bytes("hello world");
    assert_eq!(out, b"\x1b[200~hello world\x1b[201~");
}

#[test]
fn wrap_paste_bytes_handles_empty_content() {
    // Edge case: a paste of empty string still emits the markers so the
    // far side sees a zero-length paste boundary rather than nothing.
    let out = wrap_paste_bytes("");
    assert_eq!(out, b"\x1b[200~\x1b[201~");
}

#[test]
fn wrap_paste_bytes_preserves_multiline_and_special_chars() {
    let out = wrap_paste_bytes("line1\nline2\t  trailing");
    assert_eq!(out, b"\x1b[200~line1\nline2\t  trailing\x1b[201~");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paste_in_attached_view_sends_bracketed_payload_to_pty() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let _ws_id = spawn_attached_workspace(&mut app);
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));

    handle_event(&mut app, &shared, CtEvent::Paste("hello paste".into()))
        .await
        .unwrap();

    // cat echoes input back. The bracketed-paste markers are unknown
    // CSI sequences to vt100 and get swallowed; the inner content
    // appears on the screen verbatim.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let session = active_session(&app).unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("hello paste"),
        "paste content must reach the PTY; got: {screen_text:?}"
    );
}

// `paste_in_dashboard_with_pm_focused_sends_bracketed_to_pm` is gone:
// Dashboard+ProjectManager focus no longer targets a PTY (the digest
// has none), so `active_session` returns `None` and paste falls
// through to the per-char fallback instead.

#[test]
fn paste_char_to_key_translates_newline_to_enter() {
    let k = paste_char_to_key('\n');
    assert!(matches!(k.code, KeyCode::Enter));
}

#[test]
fn paste_char_to_key_translates_cr_to_enter() {
    let k = paste_char_to_key('\r');
    assert!(matches!(k.code, KeyCode::Enter));
}

#[test]
fn paste_char_to_key_translates_tab() {
    let k = paste_char_to_key('\t');
    assert!(matches!(k.code, KeyCode::Tab));
}

#[test]
fn paste_char_to_key_passes_through_printable() {
    let k = paste_char_to_key('a');
    assert!(matches!(k.code, KeyCode::Char('a')));
}
