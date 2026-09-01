//! ctrl d suppression tests.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn encode_key_swallows_ctrl_d() {
    let ev = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert!(
        encode_key(ev).is_empty(),
        "Ctrl-D must not be forwarded to the PTY"
    );
    // Upper-case form (Shift+Ctrl-D, or CapsLock) is the same byte.
    let ev_upper = KeyEvent::new(KeyCode::Char('D'), KeyModifiers::CONTROL);
    assert!(encode_key(ev_upper).is_empty());
}

#[test]
fn encode_key_still_forwards_neighboring_ctrl_keys() {
    // Sanity: keys adjacent on the keyboard keep their control bytes.
    let ev_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(encode_key(ev_c), vec![0x03]);
    let ev_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL);
    assert_eq!(encode_key(ev_f), vec![0x06]);
}
