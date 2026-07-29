pub mod activity;
pub mod agent;
pub mod app;
pub mod cli;
pub mod commands;
pub mod config;
pub mod data;
pub mod detail_modules;
pub mod error;
pub mod git;
pub(crate) mod install_common;
#[cfg(target_os = "macos")]
pub mod menubar;
pub mod names;
pub mod pty;
#[doc(hidden)]
pub mod test_support;
/// Internal wall-clock helpers; not part of the public API.
pub(crate) mod time;
#[cfg(unix)]
pub mod tui_ipc;
pub mod ui;
#[cfg(target_os = "linux")]
pub mod waybar;
pub mod workspace_rows;
