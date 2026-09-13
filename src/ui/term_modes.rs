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

use crate::error::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

/// `DECRST 1042`: stop the terminal from raising a window-manager urgency
/// hint on BEL. Honoured by Alacritty and xterm; a no-op elsewhere.
struct DisableBellUrgencyHints;

impl crossterm::Command for DisableBellUrgencyHints {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1042l")
    }
}

/// `DECSET 1042`: restore the terminal's default of raising an urgency hint
/// on BEL, for the shell (and anything it runs) after wsx exits.
struct EnableBellUrgencyHints;

impl crossterm::Command for EnableBellUrgencyHints {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1042h")
    }
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
pub fn leave_tui_modes<W: std::io::Write>(w: &mut W) -> Result<()> {
    crossterm::execute!(
        w,
        EnableBellUrgencyHints,
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
        leave_tui_modes(&mut out).unwrap();
        assert!(contains(&out, BRACKETED_PASTE_OFF));
        assert!(contains(&out, MOUSE_OFF));
        assert!(contains(&out, URGENCY_HINTS_ON));
        assert!(contains(&out, ALT_SCREEN_OFF));
    }

    /// Urgency hints are the terminal's default; the shell we hand back
    /// (and any program run from it) expects them on.
    #[test]
    fn leave_tui_modes_restores_urgency_hints_before_leaving_alt_screen() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out).unwrap();
        let alt = out
            .windows(ALT_SCREEN_OFF.len())
            .position(|w| w == ALT_SCREEN_OFF)
            .unwrap();
        let urgency = out
            .windows(URGENCY_HINTS_ON.len())
            .position(|w| w == URGENCY_HINTS_ON)
            .unwrap();
        assert!(
            urgency < alt,
            "urgency restore must precede leaving alt screen"
        );
    }

    /// The alternate screen must be left last: releasing paste/mouse after the
    /// screen has already been swapped back writes those resets to the
    /// restored screen, which is what leaves stray escape output behind.
    #[test]
    fn leave_tui_modes_leaves_the_alternate_screen_last() {
        let mut out = Vec::new();
        leave_tui_modes(&mut out).unwrap();
        let alt = out
            .windows(ALT_SCREEN_OFF.len())
            .position(|w| w == ALT_SCREEN_OFF)
            .unwrap();
        let paste = out
            .windows(BRACKETED_PASTE_OFF.len())
            .position(|w| w == BRACKETED_PASTE_OFF)
            .unwrap();
        assert!(paste < alt, "paste reset must precede leaving alt screen");
    }
}
