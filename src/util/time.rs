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

/// Human-readable age from a millisecond delta: `45s`, `12m`, `3h`.
/// Negative deltas (clock skew) clamp to `0s`.
pub fn format_age(delta_ms: i64) -> String {
    let secs = (delta_ms / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_age_buckets_by_magnitude() {
        assert_eq!(format_age(0), "0s");
        assert_eq!(format_age(59_999), "59s");
        assert_eq!(format_age(60_000), "1m");
        assert_eq!(format_age(3_599_000), "59m");
        assert_eq!(format_age(3_600_000), "1h");
        assert_eq!(format_age(-500), "0s"); // negative delta clamps
    }
}
