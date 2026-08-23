//! ctrl z suppression tests.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn encode_key_swallows_ctrl_z() {
    let ev = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(
        encode_key(ev).is_empty(),
        "Ctrl-Z must not be forwarded to the PTY"
    );
    // Upper-case form (Shift+Ctrl-Z, or CapsLock) is the same byte.
    let ev_upper = KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::CONTROL);
    assert!(encode_key(ev_upper).is_empty());
}

#[test]
fn encode_key_still_forwards_other_ctrl_keys() {
    // Sanity: a neighboring control key like Ctrl-C is untouched (0x03).
    let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(encode_key(ev), vec![0x03]);
}
