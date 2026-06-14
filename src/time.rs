//! Wall-clock helpers.
//!
//! Centralizes the `SystemTime` → epoch-millis boilerplate that was otherwise
//! duplicated across the app, render, and background loops. Both functions
//! saturate to `0` if the system clock is somehow before the Unix epoch, which
//! matches the previous inline behavior (`.unwrap_or(0)`).

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch as `i64` — the type used by sqlite
/// timestamps and most in-memory app state.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch as `u64`, for comparison against the
/// unsigned activity timestamps kept in atomics (e.g. `Session::activity_ms`).
pub fn now_ms_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whole seconds since the Unix epoch as `u64`. Used by the usage-graph
/// hour-bucketing (`now_secs - now_secs % 3600`).
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
