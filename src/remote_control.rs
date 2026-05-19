//! Settings + helper struct controlling whether claude is launched with
//! `--remote-control` (claude.ai/code + mobile relay). See
//! `docs/superpowers/specs/2026-05-16-remote-control-by-default-design.md`.

#[derive(Debug, Clone, Copy)]
pub struct RemoteOpts {
    pub enabled: bool,
    pub sandbox: bool,
}

impl RemoteOpts {
    pub fn from_store(store: &crate::store::Store) -> Self {
        Self {
            enabled: enabled(store),
            sandbox: sandbox_enabled(store),
        }
    }

    /// Convenience for tests / call sites that explicitly don't want the
    /// flag (e.g. spawning `cat` instead of claude).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            sandbox: false,
        }
    }
}

/// Defaults ON. Off-values: `false` / `off` / `0` / `no`.
pub fn enabled(store: &crate::store::Store) -> bool {
    !matches!(
        store
            .get_setting("remote_control")
            .ok()
            .flatten()
            .as_deref(),
        Some("false" | "off" | "0" | "no")
    )
}

/// Defaults OFF. On-values: `true` / `on` / `1` / `yes`.
pub fn sandbox_enabled(store: &crate::store::Store) -> bool {
    matches!(
        store
            .get_setting("remote_control_sandbox")
            .ok()
            .flatten()
            .as_deref(),
        Some("true" | "on" | "1" | "yes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_defaults_true_when_unset() {
        let store = crate::store::Store::open_in_memory().unwrap();
        assert!(enabled(&store));
    }

    #[test]
    fn enabled_false_for_off_values() {
        let store = crate::store::Store::open_in_memory().unwrap();
        for v in ["false", "off", "0", "no"] {
            store.set_setting("remote_control", v).unwrap();
            assert!(!enabled(&store), "expected disabled for {v:?}");
        }
    }

    #[test]
    fn enabled_true_for_other_values() {
        let store = crate::store::Store::open_in_memory().unwrap();
        for v in ["true", "yes", "on", "1", "anything"] {
            store.set_setting("remote_control", v).unwrap();
            assert!(enabled(&store), "expected enabled for {v:?}");
        }
    }

    #[test]
    fn sandbox_defaults_false_when_unset() {
        let store = crate::store::Store::open_in_memory().unwrap();
        assert!(!sandbox_enabled(&store));
    }

    #[test]
    fn sandbox_true_for_on_values() {
        let store = crate::store::Store::open_in_memory().unwrap();
        for v in ["true", "on", "1", "yes"] {
            store.set_setting("remote_control_sandbox", v).unwrap();
            assert!(sandbox_enabled(&store), "expected enabled for {v:?}");
        }
    }

    #[test]
    fn from_store_combines_both_settings() {
        let store = crate::store::Store::open_in_memory().unwrap();
        store.set_setting("remote_control", "false").unwrap();
        store.set_setting("remote_control_sandbox", "on").unwrap();
        let opts = RemoteOpts::from_store(&store);
        assert!(!opts.enabled);
        assert!(opts.sandbox);
    }

    #[test]
    fn disabled_constructor_is_off() {
        let opts = RemoteOpts::disabled();
        assert!(!opts.enabled);
        assert!(!opts.sandbox);
    }
}
