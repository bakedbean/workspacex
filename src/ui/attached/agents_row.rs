//! Extracted from ui/attached.rs.

/// Switch keys for the chip row's agent pills, drawn from a reserved-safe pool so
/// they never collide with the `^x` leader follow-ups already bound in the
/// attached view:
///   - `a` → agents panel
///   - `u` → updates panel
///   - `d` → detach / close-pane
///   - `x` → send literal Ctrl-x
///   - `e` → open editor
///   - `t` → open terminal
///   - `v` → open diff
///   - `g` → open lazygit
///   - `c` → open chronox
///   - `k` → process list
///   - `1-9` (digits) → pinned commands
///
/// This is the single source of truth: both the renderer and (in a later
/// task) the input dispatcher call this with the same count so the
/// displayed key always equals the bound key.
///
/// Returns AT MOST `POOL.len()` (10) keys: a workspace with more than 10
/// agents (only reachable via many same-kind duplicates) exhausts the pool.
/// Callers must NOT `zip` agents against the result in a way that silently
/// drops the overflow — agents past the pool should still render (just
/// without a keyboard switch key; they remain clickable).
pub fn agent_switch_keys(count: usize) -> Vec<char> {
    // Pool excludes every letter the attached `^x` leader already binds
    // (d, x, u, a, e, t, v, g, c, k) plus all digits (pinned chips 1-9).
    const POOL: &[char] = &['q', 'w', 'r', 'y', 'i', 'o', 'p', 's', 'h', 'j'];
    POOL.iter().copied().take(count).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_keys_skip_reserved_and_are_unique() {
        // Request the whole pool so the exclusion check covers every key.
        let keys = agent_switch_keys(64);
        // No reserved `^x`-leader letter may appear anywhere in the pool.
        for reserved in ['d', 'x', 'u', 'a', 'e', 't', 'v', 'g', 'c', 'k'] {
            assert!(
                !keys.contains(&reserved),
                "pool must not contain reserved '{reserved}'"
            );
        }
        assert!(keys.iter().all(|c| !c.is_ascii_digit())); // digits are pinned chips
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }
}
