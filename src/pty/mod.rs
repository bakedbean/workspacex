// Internal leaf modules; their public surface is funneled through the
// re-exports below (and `pty::session::*`) to keep the `pty` API stable.
mod agent_kind;
pub mod render;
pub mod session;
mod session_detect;
pub use agent_kind::AgentKind;
pub use session::{Session, SessionManager, SessionStatus};
