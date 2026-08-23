//! Desktop-shell integrations: the Linux waybar module and the macOS
//! menubar (SwiftBar) plugin.
//!
//! Both surface the same thing — a live list of workspaces the user can
//! jump to from outside the TUI — against two different host shells. The
//! platform-specific halves are gated here so the rest of the crate can
//! refer to `desktop::waybar` / `desktop::menubar` without repeating
//! `cfg(target_os = ...)` at every use site.
//!
//! Shared by both:
//!   - [`rows`] — the platform-neutral workspace row model they render
//!   - [`install_support`] — helpers for `wsx setup waybar` / `wsx setup menubar`
//!
//! Jump requests travel back to a running TUI over the socket in
//! `crate::app::ipc`; this subsystem is that socket's client, never its owner.

pub mod rows;

pub(crate) mod install_support;

#[cfg(target_os = "macos")]
pub mod menubar;

#[cfg(target_os = "linux")]
pub mod waybar;
