//! Writes from the global settings drawer (`,` on the dashboard).

use super::App;
use crate::error::Result;

/// Store `value` for a global key, validated like `wsx config set`; an empty
/// value clears the key. Re-reads the dashboard prefs cached at startup so a
/// sort-mode or pin-age change shows without a restart.
pub(crate) fn apply_global_setting(app: &mut App, key: &str, value: &str) -> Result<()> {
    let value = value.trim_end_matches('\n');
    if value.trim().is_empty() {
        app.store.delete_setting(key)?;
    } else {
        let value = crate::config::settings::normalize(key, value)?;
        app.store.set_setting(key, &value)?;
    }
    app.dashboard.load_ordering_prefs(&app.store);
    Ok(())
}
