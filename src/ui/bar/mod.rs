//! The themeable bar engine: a starship-style format grammar, segment
//! providers that carry click hits, and one evaluator shared by the
//! dashboard header and footer, the attached view's top and bottom bars,
//! and the dashboard detail pane's pinned-chip row.
//!
//! See `docs/superpowers/specs/2026-09-13-bar-theming-design.md`.

pub mod bars;
pub mod format;
pub mod providers;
pub mod registry;
pub mod render;
pub mod segment;
pub mod style;
#[cfg(test)]
pub mod test_util;
#[cfg(test)]
mod tests;

pub(crate) use bars::{AttachedInputs, attached_bars, attention_width_budget};
pub use bars::{
    DashboardFooterInputs, DashboardHeaderInputs, cfg, dashboard_detail, dashboard_footer,
    dashboard_header,
};
