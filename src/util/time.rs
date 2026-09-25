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

/// An epoch-millisecond timestamp as UTC ISO-8601 to the second:
/// `2026-09-25T14:03:11Z`. Stable and sortable, for CLI output an agent may
/// parse.
pub fn format_utc_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
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
    fn format_utc_ms_renders_iso_8601() {
        assert_eq!(format_utc_ms(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc_ms(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc_ms(1_790_342_591_999), "2026-09-25T13:23:11Z");
        assert_eq!(format_utc_ms(-1_000), "1969-12-31T23:59:59Z");
    }

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
