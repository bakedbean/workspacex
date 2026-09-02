//! Launching user-configured external tools and commands.
//!
//! `external` resolves and spawns the configured editor/terminal/lazygit/
//! difftool; `context` renders the workspace digest for `wsx context
//! show|write`; `remotes` runs named remote shell commands; `pinned` parses
//! the pinned-command chips shown in the attached view; `shared` builds the
//! machine-readable inventory for `wsx shared list --json`; `shared_hosts`
//! holds the ssh destinations for browsing shared workspaces on remote hosts.

pub mod context;
pub mod external;
pub mod pinned;
pub mod remotes;
pub mod shared;
pub mod shared_hosts;
