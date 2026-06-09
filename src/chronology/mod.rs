//! wsx-side chronology layer.
//!
//! Re-exports the `sessionx` parsing/timeline core under one namespace and
//! owns the ratatui rendering (absorbed from chronox's `render.rs`). `sessionx`
//! is framework-agnostic, so the UI mapping lives here in the consumer rather
//! than in a shared crate. wsx code refers to the core via `crate::chronology`.

pub use sessionx::*;

mod render;
pub use render::{
    change_detail_lines_styled, clip_line_to_width, entry_lines, hhmm, relative_display,
    should_auto_hide, side_cell_to_line,
};
