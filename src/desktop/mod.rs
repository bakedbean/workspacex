//! Desktop-shell integrations: the Linux waybar module and KDE Plasma
//! applet, and the macOS menubar (SwiftBar) plugin.
//!
//! All three surface the same thing — a live list of workspaces the user
//! can jump to from outside the TUI — against different host shells. The
//! platform-specific halves are gated here so the rest of the crate can
//! refer to `desktop::waybar` / `desktop::plasma` / `desktop::menubar`
//! without repeating `cfg(target_os = ...)` at every use site.
//!
//! Shared:
//!   - [`rows`] — the platform-neutral workspace row model they render
//!   - [`status`] — the desktop-neutral summary (count, most urgent state,
//!     tooltip, rows) behind `wsx desktop status` and the waybar payload
//!   - [`install_support`] — helpers for the `wsx setup` installers
//!   - `jump` and `focus` (Linux) — select a workspace in a running TUI and
//!     raise its window, for both the waybar module and the Plasma applet
//!
//! Jump requests travel back to a running TUI over the socket in
//! `crate::app::ipc`; this subsystem is that socket's client, never its owner.

pub mod rows;

pub mod status;

pub(crate) mod install_support;

#[cfg(target_os = "linux")]
pub(crate) mod focus;

#[cfg(target_os = "linux")]
pub mod jump;

#[cfg(target_os = "macos")]
pub mod menubar;

#[cfg(target_os = "linux")]
pub mod plasma;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod terminal;

#[cfg(target_os = "linux")]
pub mod waybar;
