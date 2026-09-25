//! Launching user-configured external tools and commands.
//!
//! `external` resolves and spawns the configured editor/terminal/lazygit/
//! difftool; `context` renders the workspace digest for `wsx context
//! show|write`; `inspect` builds the read-only views behind `status show`, `recap show`,
//! `agent list` and `workspace list`; `remotes` runs named remote shell commands; `pinned` parses
//! the pinned-command chips shown in the attached view; `shared` builds the
//! machine-readable inventory for `wsx shared list --json`; `shared_hosts`
//! holds the ssh destinations for browsing shared workspaces on remote hosts;
//! `tags` holds the prompt-tag names the attached view wraps a body in
//! (`<context>…</context>`) and their use counts.

pub mod context;
pub mod external;
pub mod inspect;
pub mod pinned;
pub mod remotes;
pub mod shared;
pub mod shared_hosts;
pub mod tags;
