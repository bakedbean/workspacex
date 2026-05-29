//! Launching user-configured external tools and commands.
//!
//! `external` resolves and spawns the configured editor/terminal/lazygit/
//! difftool; `remotes` runs named remote shell commands; `pinned` parses
//! the pinned-command chips shown in the attached view.

pub mod external;
pub mod pinned;
pub mod remotes;
