//! Crate-wide primitives with no dependency on wsx domain types.
//!
//! Modules here are leaves: they may be used from anywhere, and they use
//! nothing from the rest of the crate. Anything that reaches back into
//! `data`, `app`, or `ui` belongs in the subsystem it serves, not here.

pub mod names;
pub(crate) mod time;
