//! Translating between crossterm key events and the bytes a PTY expects,
//! plus the `WSX_INPUT_TRACE` diagnostics.

use crossterm::event::{Event as CtEvent, KeyCode, KeyModifiers};

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

pub(in crate::app::input) fn encode_key(k: crossterm::event::KeyEvent) -> Vec<u8> {
    use KeyCode::*;
    match k.code {
        Char(c) => {
            if k.modifiers.contains(KeyModifiers::CONTROL) && c.is_ascii_alphabetic() {
                // Ctrl-Z encodes to 0x1a (SUSP). Forwarding it into the child
                // PTY suspends whatever job owns the pane (a shell, or the
                // agent's own process) — and because wsx captures every
                // keystroke in the attached view there's no reachable prompt
                // left to `fg` it back, wedging the pane. It's an easy
                // fat-finger right next to the Ctrl-x leader, so swallow it:
                // an accidental Ctrl-Z becomes a harmless no-op.
                if c.eq_ignore_ascii_case(&'z') {
                    return vec![];
                }
                vec![(c.to_ascii_lowercase() as u8) - b'a' + 1]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        Enter => b"\r".to_vec(),
        Backspace => b"\x7f".to_vec(),
        Tab => b"\t".to_vec(),
        Esc => b"\x1b".to_vec(),
        Left => b"\x1b[D".to_vec(),
        Right => b"\x1b[C".to_vec(),
        Up => b"\x1b[A".to_vec(),
        Down => b"\x1b[B".to_vec(),
        _ => vec![],
    }
}

/// Translate a pasted character into the `KeyEvent` crossterm would have
/// emitted if it were typed live. Matters for the non-attached fallback:
/// `\n`/`\r` are Enter (modal submit), `\t` is Tab (focus / autocomplete),
/// printable chars pass through as `Char(c)`.
pub(in crate::app::input) fn paste_char_to_key(c: char) -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyEvent, KeyModifiers};
    let code = match c {
        '\n' | '\r' => KeyCode::Enter,
        '\t' => KeyCode::Tab,
        _ => KeyCode::Char(c),
    };
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Wrap a paste payload with the bracketed-paste escape markers claude
/// reads to render `[Pasted N lines]` instead of treating the content as
/// typed input. The output is what gets written to the PTY in one send.
pub(crate) fn wrap_paste_bytes(content: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

/// Whether the diagnostic input trace is on. Set `WSX_INPUT_TRACE=1` to
/// enable; read once and cached, so the hot key path costs one atomic load.
pub(in crate::app::input) fn input_trace_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("WSX_INPUT_TRACE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

/// First or last `n` chars of a payload, control chars escaped, for the trace.
pub(in crate::app::input) fn trace_preview(s: &str, head: bool, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let slice: String = if head {
        chars.iter().take(n).collect()
    } else {
        chars.iter().skip(chars.len().saturating_sub(n)).collect()
    };
    slice.escape_debug().to_string()
}

/// Log one crossterm event to `wsx.log` when `WSX_INPUT_TRACE=1`.
///
/// This exists to settle a question that cannot be answered from inside
/// `handle_paste`: whether a paste reached wsx as a single `Event::Paste`
/// (the terminal's `ESC[200~ … ESC[201~` markers survived) or as a burst of
/// individual key presses (the markers were lost upstream). The two are
/// indistinguishable after the fact, and they have opposite fixes — but they
/// produce the same user-visible symptom, because a bare `\r` forwarded to an
/// agent PTY submits whatever is in its composer.
pub(in crate::app::input) fn trace_event(evt: &CtEvent) {
    if !input_trace_enabled() {
        return;
    }
    match evt {
        CtEvent::Paste(content) => tracing::info!(
            target: "wsx::input_trace",
            bytes = content.len(),
            chars = content.chars().count(),
            newlines = content.chars().filter(|c| *c == '\n' || *c == '\r').count(),
            head = %trace_preview(content, true, 48),
            tail = %trace_preview(content, false, 48),
            "paste"
        ),
        CtEvent::Key(k) => tracing::info!(
            target: "wsx::input_trace",
            code = ?k.code,
            mods = ?k.modifiers,
            kind = ?k.kind,
            "key"
        ),
        _ => {}
    }
}
