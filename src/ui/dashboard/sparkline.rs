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
        let out = render(&[0,0,0,0,0,0,0,1,2,3], 3);
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
}
