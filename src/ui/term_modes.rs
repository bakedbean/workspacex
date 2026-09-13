//! The set of terminal modes wsx runs the TUI under, and the two functions
//! that assert and release them.
//!
//! These exist so there is exactly ONE definition of "the modes the TUI needs".
//! Terminal modes are global state on the tty, not per-process: whoever writes
//! the DECSET last wins, and nothing re-asserts them afterwards. wsx hands the
//! terminal to external full-screen programs (`$EDITOR`, via
//! `commands::external::edit_in_editor`), and those reset modes as they exit —
//! vim emits `ESC[?2004l` (bracketed paste off) and `ESC[?1002l` (mouse off)
//! on quit.
//!
//! So the startup path and the resume-from-editor path must enable the same
//! modes, or the TUI silently loses a capability mid-session. In particular,
//! losing bracketed paste means the terminal stops wrapping pastes in
//! `ESC[200~ … ESC[201~` and delivers them as individual key presses instead;
//! in the attached view each pasted newline is then forwarded to the agent's
//! PTY as Enter, so a multi-paragraph paste submits a partial prompt.
//!
//! One mode here is turned OFF rather than on: bell urgency hints (private
//! mode 1042). wsx rings the terminal bell when a workspace needs attention.
//! Alacritty and xterm answer BEL on an unfocused window by asking the window
//! manager for attention; on Wayland that is an xdg-activation request, and
//! Hyprland with `misc:focus_on_activate = true` (Omarchy's default) honours
//! it by switching the OS workspace to the terminal. Clearing 1042 keeps the
//! audible and visual bell while suppressing that side effect. Terminals
//! that don't implement 1042 ignore it.
//!
//! Unlike the other modes, 1042 has no universal default — Alacritty ships
//! with it on, xterm ships with `bellIsUrgent` off — so on the way out wsx
//! restores what it found rather than assuming. [`probe_bell_urgency`] asks
//! the terminal once at startup (DECRQM); [`leave_tui_modes`] re-enables the
//! mode only if that probe said it was on.

use crate::error::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use std::sync::atomic::{AtomicU8, Ordering};

/// `DECRST 1042`: stop the terminal from raising a window-manager urgency
/// hint on BEL. Honoured by Alacritty and xterm; a no-op elsewhere.
struct DisableBellUrgencyHints;

impl crossterm::Command for DisableBellUrgencyHints {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1042l")
    }
}

/// `DECSET 1042`: re-enable urgency hints on BEL, for the shell (and
/// anything it runs) after wsx exits. Only emitted when the startup probe
/// found the mode on.
struct EnableBellUrgencyHints;

impl crossterm::Command for EnableBellUrgencyHints {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1042h")
    }
}

/// What the terminal said about private mode 1042 when wsx started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BellUrgency {
    /// The terminal reported the mode set: restore it on exit.
    On,
    /// The terminal reported the mode reset: leave it off on exit.
    Off,
    /// No usable answer — the terminal does not implement 1042, a
    /// multiplexer swallowed the query, or the probe never ran. Treated
    /// like [`BellUrgency::Off`]: enabling the mode blind is the exact
    /// failure this module exists to prevent.
    Unknown,
}

/// Process-wide record of the startup probe, read by every exit path
/// (normal exit, panic hook, editor hand-off). Terminal modes are per-tty
/// global state, so a per-process global mirrors the thing it describes.
static BELL_URGENCY_ON_ENTRY: AtomicU8 = AtomicU8::new(URGENCY_UNKNOWN);
const URGENCY_UNKNOWN: u8 = 0;
const URGENCY_ON: u8 = 1;
const URGENCY_OFF: u8 = 2;

/// The result of the last [`probe_bell_urgency`], or `Unknown` if it never
/// ran.
pub fn bell_urgency_on_entry() -> BellUrgency {
    match BELL_URGENCY_ON_ENTRY.load(Ordering::Relaxed) {
        URGENCY_ON => BellUrgency::On,
        URGENCY_OFF => BellUrgency::Off,
        _ => BellUrgency::Unknown,
    }
}

/// How long to wait for the terminal to answer the probe. Local terminals
/// answer in well under a millisecond; this bounds a remote hop or a
/// terminal that answers nothing at all.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);

/// Ask the terminal whether bell urgency hints are currently on, and record
/// the answer for [`bell_urgency_on_entry`].
///
/// Sends DECRQM for 1042 followed by a primary device attributes query, and
/// reads the tty until the DA1 reply arrives or [`PROBE_TIMEOUT`] passes.
/// Every real terminal answers DA1, so that reply — not the timer — is the
/// normal stop condition; the timer only covers terminals that answer
/// neither. This is the same technique crossterm uses to detect keyboard
/// enhancement support.
///
/// Must run in raw mode (a cooked tty line-buffers the reply) and before
/// anything else reads stdin, i.e. before the event stream exists. Bytes
/// that arrive during the probe which are not replies (typeahead) are
/// dropped.
pub fn probe_bell_urgency() -> BellUrgency {
    let urgency = probe_bell_urgency_io().unwrap_or(BellUrgency::Unknown);
    let code = match urgency {
        BellUrgency::On => URGENCY_ON,
        BellUrgency::Off => URGENCY_OFF,
        BellUrgency::Unknown => URGENCY_UNKNOWN,
    };
    BELL_URGENCY_ON_ENTRY.store(code, Ordering::Relaxed);
    urgency
}

#[cfg(unix)]
fn probe_bell_urgency_io() -> std::io::Result<BellUrgency> {
    use std::io::{Read, Write};
    use std::os::unix::io::AsRawFd;
    use std::time::Instant;

    // ESC [ ? 1042 $ p   DECRQM: report private mode 1042.
    // ESC [ c            DA1: primary device attributes (the end marker).
    const QUERY: &[u8] = b"\x1b[?1042$p\x1b[c";

    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    tty.write_all(QUERY)?;
    tty.flush()?;

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 256];
    while !probe_reply_complete(&buf) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let mut pfd = libc::pollfd {
            fd: tty.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is a valid, initialised pollfd that outlives the call,
        // and the count (1) matches the single struct passed.
        let ready = unsafe { libc::poll(&mut pfd, 1, remaining.as_millis() as i32) };
        if ready <= 0 {
            break; // timeout, or an error we treat the same way
        }
        let got = tty.read(&mut chunk)?;
        if got == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..got]);
    }
    Ok(parse_bell_urgency_reply(&buf))
}

#[cfg(not(unix))]
fn probe_bell_urgency_io() -> std::io::Result<BellUrgency> {
    Ok(BellUrgency::Unknown)
}

/// Whether the DA1 reply (`ESC [ ? <params> c`) has arrived, which marks the
/// end of the terminal's answers to the probe.
fn probe_reply_complete(buf: &[u8]) -> bool {
    csi_private_final(buf, b'c').is_some()
}

/// Extract the mode state from a DECRPM reply (`ESC [ ? 1042 ; Ps $ y`)
/// anywhere in `buf`. Ps: 1 or 3 = set, 2 or 4 = reset, anything else
/// (including 0 = not recognised, or no reply at all) = unknown.
fn parse_bell_urgency_reply(buf: &[u8]) -> BellUrgency {
    let mut rest = buf;
    while let Some((params, after)) = csi_private_final(rest, b'y') {
        rest = after;
        // params is e.g. b"1042;1$" — the `$` intermediate precedes `y`.
        let Some(params) = params.strip_suffix(b"$") else {
            continue;
        };
        let mut it = params.split(|&b| b == b';');
        if it.next() != Some(b"1042".as_slice()) {
            continue;
        }
        return match it.next() {
            Some(b"1") | Some(b"3") => BellUrgency::On,
            Some(b"2") | Some(b"4") => BellUrgency::Off,
            _ => BellUrgency::Unknown,
        };
    }
    BellUrgency::Unknown
}

/// Find the first `ESC [ ? <params> <final>` in `buf`, where `<params>` is
/// digits, `;` and `$`. Returns the params and the remainder after the
/// final byte.
fn csi_private_final(buf: &[u8], final_byte: u8) -> Option<(&[u8], &[u8])> {
    let mut start = 0;
    while let Some(pos) = buf[start..].windows(3).position(|w| w == b"\x1b[?") {
        let params_start = start + pos + 3;
        let params_end = buf[params_start..]
            .iter()
            .position(|b| !(b.is_ascii_digit() || *b == b';' || *b == b'$'))
            .map(|n| params_start + n);
        match params_end {
            Some(end) if buf[end] == final_byte => {
                return Some((&buf[params_start..end], &buf[end + 1..]));
            }
            Some(end) => start = end,
            None => return None,
        }
    }
    None
}

/// Assert every terminal mode the TUI depends on. Used both at startup and
/// when resuming after an external program had the terminal.
///
/// Does not touch raw mode: callers own that, because `enable_raw_mode`
/// operates on the process's real tty rather than on `w`.
pub fn enter_tui_modes<W: std::io::Write>(w: &mut W) -> Result<()> {
    crossterm::execute!(
        w,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        DisableBellUrgencyHints
    )?;
    Ok(())
}

/// Release the modes [`enter_tui_modes`] asserted, in reverse order. Used at
/// exit, in the panic hook, and before handing the terminal to an external
/// program.
///
/// `urgency` is what [`probe_bell_urgency`] found at startup (pass
/// [`bell_urgency_on_entry`]); bell urgency hints are re-enabled only if
/// they were on.
pub fn leave_tui_modes<W: std::io::Write>(w: &mut W, urgency: BellUrgency) -> Result<()> {
    if urgency == BellUrgency::On {
        crossterm::execute!(w, EnableBellUrgencyHints)?;
    }
    crossterm::execute!(
        w,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALT_SCREEN_ON: &[u8] = b"\x1b[?1049h";
    const ALT_SCREEN_OFF: &[u8] = b"\x1b[?1049l";
    const BRACKETED_PASTE_ON: &[u8] = b"\x1b[?2004h";
    const BRACKETED_PASTE_OFF: &[u8] = b"\x1b[?2004l";
    const MOUSE_ON: &[u8] = b"\x1b[?1002h";
    const MOUSE_OFF: &[u8] = b"\x1b[?1002l";
    const URGENCY_HINTS_ON: &[u8] = b"\x1b[?1042h";
    const URGENCY_HINTS_OFF: &[u8] = b"\x1b[?1042l";

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    fn position(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .position(|w| w == needle)
            .unwrap_or_else(|| panic!("expected {needle:?} in {haystack:?}"))
    }

    /// The regression guard for the paste bug: resuming the TUI after an
    /// external editor must re-enable bracketed paste. vim turns it off on
    /// exit, and without this the next paste arrives as key presses, so a
    /// pasted newline reaches the agent as Enter.
    #[test]
    fn enter_tui_modes_re_enables_bracketed_paste() {
        let mut out = Vec::new();
        enter_tui_modes(&mut out).unwrap();
        assert!(
            contains(&out, BRACKETED_PASTE_ON),
            "expected {BRACKETED_PASTE_ON:?} in {out:?}"
        );
    }

    /// vim also disables mouse reporting on exit, so the resume path has to
    /// re-assert it for the same reason.
    #[test]
    fn enter_tui_modes_re_enables_mouse_capture() {
        let mut out = Vec::new();
        enter_tui_modes(&mut out).unwrap();
        assert!(contains(&out, MOUSE_ON), "expected {MOUSE_ON:?} in {out:?}");
    }

    #[test]
    fn enter_tui_modes_enters_the_alternate_screen() {
        let mut out = Vec::new();
        enter_tui_modes(&mut out).unwrap();
        assert!(contains(&out, ALT_SCREEN_ON));
    }

    /// The bell must stay a bell. Alacritty (and xterm) turn BEL into a
    /// window-manager urgency request while the window is unfocused, and on
    /// Wayland that request is xdg-activation, which Hyprland with
    /// `misc:focus_on_activate = true` (Omarchy's default) answers by
    /// switching the OS workspace to the terminal. Private mode 1042 is the
    /// terminal's opt-out: the audible/visual bell still fires, only the
    /// urgency side effect is suppressed.
    #[test]
    fn enter_tui_modes_disables_bell_urgency_hints() {
        let mut out = Vec::new();
        enter_tui_modes(&mut out).unwrap();
        assert!(
            contains(&out, URGENCY_HINTS_OFF),
            "expected {URGENCY_HINTS_OFF:?} in {out:?}"
        );
    }

    #[test]
    fn leave_tui_modes_releases_every_mode_enter_asserted() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out, BellUrgency::On).unwrap();
        assert!(contains(&out, BRACKETED_PASTE_OFF));
        assert!(contains(&out, MOUSE_OFF));
        assert!(contains(&out, URGENCY_HINTS_ON));
        assert!(contains(&out, ALT_SCREEN_OFF));
    }

    /// Urgency hints are NOT a universal default: Alacritty ships with them
    /// on, xterm ships with `bellIsUrgent` off, and either can be changed
    /// before wsx starts. Restore only what the startup probe saw.
    #[test]
    fn leave_tui_modes_restores_urgency_hints_when_they_were_on() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out, BellUrgency::On).unwrap();
        assert!(contains(&out, URGENCY_HINTS_ON));
        assert!(!contains(&out, URGENCY_HINTS_OFF));
    }

    #[test]
    fn leave_tui_modes_leaves_urgency_hints_off_when_they_were_off() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out, BellUrgency::Off).unwrap();
        assert!(
            !contains(&out, URGENCY_HINTS_ON),
            "must not enable: {out:?}"
        );
        assert!(!contains(&out, URGENCY_HINTS_OFF));
    }

    /// No answer to the probe means the terminal does not implement 1042
    /// (or a multiplexer swallowed the query). Turning the mode on blind is
    /// the failure mode this whole feature exists to prevent, so do nothing.
    #[test]
    fn leave_tui_modes_leaves_urgency_hints_alone_when_unknown() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out, BellUrgency::Unknown).unwrap();
        assert!(
            !contains(&out, URGENCY_HINTS_ON),
            "must not enable: {out:?}"
        );
        assert!(!contains(&out, URGENCY_HINTS_OFF));
    }

    /// Restoring after the screen swap would write the sequence onto the
    /// restored shell screen, like the paste/mouse resets below.
    #[test]
    fn leave_tui_modes_restores_urgency_hints_before_leaving_alt_screen() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out, BellUrgency::On).unwrap();
        assert!(position(&out, URGENCY_HINTS_ON) < position(&out, ALT_SCREEN_OFF));
    }

    /// The alternate screen must be left last: releasing paste/mouse after the
    /// screen has already been swapped back writes those resets to the
    /// restored screen, which is what leaves stray escape output behind.
    #[test]
    fn leave_tui_modes_leaves_the_alternate_screen_last() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out, BellUrgency::On).unwrap();
        assert!(position(&out, BRACKETED_PASTE_OFF) < position(&out, ALT_SCREEN_OFF));
    }

    // --- probe reply parsing ------------------------------------------------
    //
    // The probe sends DECRQM for 1042 followed by a primary device
    // attributes query. The DECRPM reply is `CSI ? 1042 ; Ps $ y` with
    // Ps = 0 not recognised, 1 set, 2 reset, 3 permanently set,
    // 4 permanently reset. The DA1 reply (`CSI ? … c`) marks the end of the
    // terminal's answers, whether or not it recognised 1042.

    #[test]
    fn probe_reply_set_means_on() {
        assert_eq!(
            parse_bell_urgency_reply(b"\x1b[?1042;1$y\x1b[?6c"),
            BellUrgency::On
        );
        assert_eq!(
            parse_bell_urgency_reply(b"\x1b[?1042;3$y\x1b[?6c"),
            BellUrgency::On
        );
    }

    #[test]
    fn probe_reply_reset_means_off() {
        assert_eq!(
            parse_bell_urgency_reply(b"\x1b[?1042;2$y\x1b[?6c"),
            BellUrgency::Off
        );
        assert_eq!(
            parse_bell_urgency_reply(b"\x1b[?1042;4$y\x1b[?6c"),
            BellUrgency::Off
        );
    }

    #[test]
    fn probe_reply_not_recognised_means_unknown() {
        assert_eq!(
            parse_bell_urgency_reply(b"\x1b[?1042;0$y\x1b[?6c"),
            BellUrgency::Unknown
        );
    }

    /// kitty/foot/tmux answer DA1 but not DECRQM for 1042 (or answer
    /// nothing at all).
    #[test]
    fn probe_reply_without_decrpm_means_unknown() {
        assert_eq!(
            parse_bell_urgency_reply(b"\x1b[?62;22c"),
            BellUrgency::Unknown
        );
        assert_eq!(parse_bell_urgency_reply(b""), BellUrgency::Unknown);
    }

    /// Typeahead can precede the reply; a different mode's report must not
    /// be mistaken for ours.
    #[test]
    fn probe_reply_ignores_noise_and_other_modes() {
        assert_eq!(
            parse_bell_urgency_reply(b"jk\x1b[?2004;1$y\x1b[?1042;2$y\x1b[?6c"),
            BellUrgency::Off
        );
        assert_eq!(
            parse_bell_urgency_reply(b"\x1b[?2004;1$y\x1b[?6c"),
            BellUrgency::Unknown
        );
    }

    #[test]
    fn probe_reply_is_complete_once_da1_arrives() {
        assert!(!probe_reply_complete(b""));
        assert!(!probe_reply_complete(b"\x1b[?1042;1$y"));
        assert!(!probe_reply_complete(b"\x1b[?1042;1$y\x1b[?6"));
        assert!(probe_reply_complete(b"\x1b[?1042;1$y\x1b[?6c"));
        assert!(probe_reply_complete(b"\x1b[?62;22c"));
    }

    /// A stray `c` keystroke is not a DA1 reply.
    #[test]
    fn probe_reply_completion_needs_the_csi_prefix() {
        assert!(!probe_reply_complete(b"c"));
        assert!(!probe_reply_complete(b"\x1b[?1042;1$yc"));
    }
}
