//! Wakeup signal from PTY reader threads to the TUI render loop.
//!
//! The reader thread in [`crate::pty::session::spawn_command_session`] parses
//! bytes into the shared `vt100::Parser` off-thread and never told anyone. The
//! render loop therefore only discovered new output when it happened to redraw,
//! which made the render tick double as the PTY poll rate — the reason that
//! tick was pinned at 16ms and burned CPU rebuilding unchanged frames forever.
//!
//! Signalling here decouples the two: the tick drops to animation speed while
//! attached panes still repaint as soon as bytes actually land.
//!
//! Only *visible* sessions signal. A backgrounded agent keeps running and keeps
//! parsing, but no frame can display it — `pty::render::render_screen` has a
//! single caller, in `ui::attached` — so signalling for it would hold the loop
//! at full redraw rate showing nothing new. The gate lives on
//! [`crate::pty::session::Session::visible`], which the render path maintains
//! each frame; a signal arriving here therefore means "something on screen
//! changed", not merely "some PTY produced bytes".

use tokio::sync::Notify;

/// A coalescing "something changed" edge between PTY readers and the renderer.
///
/// Deliberately carries no payload — the renderer re-reads whatever the parser
/// holds, so N reads landing between two frames are one repaint, not N.
#[derive(Debug, Default)]
pub struct OutputWake(Notify);

impl OutputWake {
    pub fn new() -> Self {
        Self(Notify::new())
    }

    /// Signal that a session produced output. Called on every PTY read, so it
    /// must stay cheap: `notify_one` is a single atomic in the uncontended case.
    pub fn notify(&self) {
        self.0.notify_one();
    }

    /// Wait for the next output. A signal raised while nobody is waiting is
    /// stored rather than dropped, so output landing mid-frame still forces the
    /// following frame — the renderer can never sleep on stale state.
    pub async fn wait(&self) {
        self.0.notified().await;
    }
}

/// Process-wide wake shared by every session's reader thread. A single TUI
/// process drives one render loop, so one edge is enough; keeping it here
/// avoids threading a channel through the whole spawn path.
pub fn output_wake() -> &'static OutputWake {
    static OUTPUT: std::sync::LazyLock<OutputWake> = std::sync::LazyLock::new(OutputWake::new);
    &OUTPUT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Tests use local `OutputWake`s, never `output_wake()`: the process-wide
    // instance is shared by every test in the binary, and a stored permit from
    // one would satisfy another's wait.

    #[tokio::test]
    async fn wait_returns_when_output_arrives_first() {
        // The mid-frame case: bytes land while the renderer is busy drawing.
        // The signal must survive until the loop comes back around to wait.
        let wake = OutputWake::new();
        wake.notify();
        tokio::time::timeout(Duration::from_secs(1), wake.wait())
            .await
            .expect("a signal raised before the wait must not be lost");
    }

    #[tokio::test]
    async fn wait_returns_when_output_arrives_later() {
        let wake = std::sync::Arc::new(OutputWake::new());
        let signaller = wake.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            signaller.notify();
        });
        tokio::time::timeout(Duration::from_secs(1), wake.wait())
            .await
            .expect("a waiting renderer must be woken by later output");
    }

    #[tokio::test]
    async fn bursts_collapse_into_a_single_wakeup() {
        // The property that makes this safe to call per-read: a streaming agent
        // emitting thousands of chunks costs one repaint, not thousands.
        let wake = OutputWake::new();
        wake.notify();
        wake.notify();
        wake.notify();

        wake.wait().await;
        let second = tokio::time::timeout(Duration::from_millis(50), wake.wait()).await;
        assert!(
            second.is_err(),
            "three reads between frames must coalesce into one repaint"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn losing_a_select_race_does_not_break_later_waits() {
        // The renderer awaits this inside a `select!`, so its wait future is
        // routinely polled (registering a waiter) and then dropped when another
        // arm wins. Registration must not leave the wake in a state where a
        // later signal is missed — otherwise an attached pane could silently
        // stop repainting.
        //
        // This pins the behaviour we depend on at the boundary we control. It
        // does not attempt to pin tokio's internal guarantee that a permit
        // handed to a future dropped mid-poll is passed on to the next waiter.
        let wake = OutputWake::new();

        tokio::select! {
            _ = wake.wait() => panic!("nothing has signalled yet"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }

        wake.notify();
        tokio::time::timeout(Duration::from_millis(1), wake.wait())
            .await
            .expect("a signal after a lost select race must still be observed");
    }

    #[tokio::test]
    async fn wait_blocks_while_there_is_no_output() {
        // Guards the whole point of the change: an idle attached pane must let
        // the render loop sleep instead of spinning it at the old 62.5Hz.
        let wake = OutputWake::new();
        let idle = tokio::time::timeout(Duration::from_millis(50), wake.wait()).await;
        assert!(idle.is_err(), "no output must not wake the renderer");
    }
}
