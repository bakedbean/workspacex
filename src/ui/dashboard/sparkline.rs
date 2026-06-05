//! Render a sequence of u32 samples as Unicode block characters.
//! Used by the dashboard footer's 24h activity strip.

pub const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render `samples` to a string of length `len`. If `samples.len() < len`,
/// missing samples render as the lowest block (`▁`); if longer, only the
/// last `len` samples are kept. Range scales by the max sample (floor 1).
pub fn render(samples: &[u32], len: usize) -> String {
    let start = samples.len().saturating_sub(len);
    let tail = &samples[start..];
    let max = (*tail.iter().max().unwrap_or(&0)).max(1);
    let pad = len.saturating_sub(tail.len());
    let mut out = String::with_capacity(len * 3);
    for _ in 0..pad {
        out.push(BLOCKS[0]);
    }
    for &v in tail {
        let idx = ((v as u64 * 7) / max as u64) as usize;
        let idx = idx.min(7);
        out.push(BLOCKS[idx]);
    }
    out
}

/// Collapse hourly `(hour_epoch, max_live)` buckets into `bars` samples covering
/// the most recent `window_hours`, ending at the end of `now_hour`. Each output
/// bar spans `window_hours / bars` hours and takes the MAX of the buckets whose
/// `hour_epoch` falls inside it; spans with no buckets yield 0. Output length is
/// always `bars` (so `bars == 0` yields an empty vec). Bar 0 is oldest, bar
/// `bars-1` is most recent.
pub fn aggregate_buckets(
    buckets: &[(u64, u32)],
    now_hour: u64,
    window_hours: u64,
    bars: usize,
) -> Vec<u32> {
    if bars == 0 {
        return Vec::new();
    }
    let span_hours = (window_hours / bars as u64).max(1);
    let span_secs = span_hours * 3600;
    let total_secs = span_secs * bars as u64;
    // Window ends at the end of the current hour so "now" lands in the last bar.
    let window_end = now_hour.saturating_add(3600);
    let window_start = window_end.saturating_sub(total_secs);

    let mut out = vec![0u32; bars];
    for &(hour, value) in buckets {
        if hour < window_start || hour >= window_end {
            continue;
        }
        let idx = ((hour - window_start) / span_secs) as usize;
        if idx < bars && value > out[idx] {
            out[idx] = value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_zero_is_all_lowest() {
        assert_eq!(render(&[0, 0, 0], 3), "▁▁▁");
    }

    #[test]
    fn short_input_left_pads_with_lowest() {
        let out = render(&[1, 1], 5);
        // 3 missing samples then 2 maxed (since max=1, all render as full)
        assert_eq!(out.chars().count(), 5);
        assert_eq!(&out.chars().collect::<String>()[..], "▁▁▁██");
    }

    #[test]
    fn long_input_keeps_tail() {
        // 10 samples, render last 3.
        let out = render(&[0, 0, 0, 0, 0, 0, 0, 1, 2, 3], 3);
        assert_eq!(out.chars().count(), 3);
        // last 3 are 1,2,3 with max=3 → ⌊7/3⌋=2 (▃), ⌊14/3⌋=4 (▅), 7 (█)
        let chars: Vec<char> = out.chars().collect();
        assert_eq!(chars, vec!['▃', '▅', '█']);
    }

    #[test]
    fn output_length_always_matches_requested() {
        assert_eq!(render(&[], 24).chars().count(), 24);
        assert_eq!(render(&[5; 100], 24).chars().count(), 24);
    }

    // Helper: an hour-aligned "now" for deterministic bucket math.
    fn now_hour() -> u64 {
        1_000_000 - (1_000_000 % 3600)
    }

    #[test]
    fn day_window_maps_one_bucket_per_bar() {
        let now = now_hour();
        // 24 buckets, one per hour, oldest first, values 1..=24.
        let buckets: Vec<(u64, u32)> = (0..24)
            .map(|i| (now - (23 - i) as u64 * 3600, (i + 1) as u32))
            .collect();
        let out = aggregate_buckets(&buckets, now, 24, 24);
        assert_eq!(out, (1..=24).collect::<Vec<u32>>());
    }

    #[test]
    fn week_window_places_recent_bucket_in_last_bar() {
        let now = now_hour();
        let out = aggregate_buckets(&[(now, 5)], now, 168, 24);
        assert_eq!(out.len(), 24);
        assert_eq!(out[23], 5);
        assert!(out[..23].iter().all(|&v| v == 0));
    }

    #[test]
    fn aggregation_takes_max_within_a_span() {
        let now = now_hour();
        // Two buckets within the last 7h span of a 1-week window.
        let out = aggregate_buckets(&[(now, 3), (now - 3600, 7)], now, 168, 24);
        assert_eq!(out[23], 7);
    }

    #[test]
    fn buckets_older_than_window_are_ignored() {
        let now = now_hour();
        let out = aggregate_buckets(&[(now - 200 * 3600, 9)], now, 168, 24);
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn output_length_is_always_bars() {
        let now = now_hour();
        assert_eq!(aggregate_buckets(&[], now, 24, 24).len(), 24);
        assert_eq!(aggregate_buckets(&[], now, 168, 24).len(), 24);
        assert_eq!(aggregate_buckets(&[], now, 720, 24).len(), 24);
    }

    #[test]
    fn zero_bars_yields_empty() {
        let now = now_hour();
        assert!(aggregate_buckets(&[(now, 5)], now, 24, 0).is_empty());
    }
}
