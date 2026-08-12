//! Generic key/value settings persistence (the `settings` table).
//!
//! Reads are memoized. `get_setting` is called several times per rendered
//! frame (nerd fonts, notifications, dashboard column widths, pinned
//! commands), and each call was a SQLite round trip inside the render path.
//! The table is tiny and changes rarely, so reads are served from a cache that
//! is dropped on any local write and on any externally-detected commit.
//!
//! All settings SQL lives in this file, so the cache cannot be bypassed by a
//! raw write elsewhere.

use crate::data::store::Store;
use crate::error::Result;
use rusqlite::OptionalExtension;

impl Store {
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        if let Ok(cache) = self.settings_cache.read() {
            if let Some(hit) = cache.get(key) {
                return Ok(hit.clone());
            }
        }
        let value = self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        // Misses are cached too: several render-path keys are usually unset,
        // and without this they would query on every frame forever.
        if let Ok(mut cache) = self.settings_cache.write() {
            cache.insert(key.to_string(), value.clone());
        }
        Ok(value)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        self.invalidate_settings_cache();
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM settings WHERE key = ?1", [key])?;
        self.invalidate_settings_cache();
        Ok(())
    }

    pub fn list_settings(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Drop every memoized setting.
    ///
    /// Local writes call this themselves. Callers only need it for changes made
    /// through *another* connection — the TUI does so when `data_version`
    /// reports a sibling `wsx` process committed. Until then a cached read can
    /// be one poll interval stale, which is the deliberate trade for keeping
    /// SQLite out of the render path.
    pub fn invalidate_settings_cache(&self) {
        if let Ok(mut cache) = self.settings_cache.write() {
            cache.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::store::Store;

    #[test]
    fn reads_back_a_written_value() {
        let s = Store::open_in_memory().unwrap();
        s.set_setting("theme", "dark").unwrap();
        assert_eq!(s.get_setting("theme").unwrap().as_deref(), Some("dark"));
    }

    #[test]
    fn an_unset_key_reads_as_none() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get_setting("nope").unwrap(), None);
        // Second read comes from the cached miss; must still be None, not a hit
        // on some other key's value.
        assert_eq!(s.get_setting("nope").unwrap(), None);
    }

    #[test]
    fn a_local_overwrite_is_visible_immediately() {
        // The cache must not outlive its own writes — this is what would break
        // the settings modal if invalidation were missed.
        let s = Store::open_in_memory().unwrap();
        s.set_setting("theme", "dark").unwrap();
        assert_eq!(s.get_setting("theme").unwrap().as_deref(), Some("dark"));
        s.set_setting("theme", "light").unwrap();
        assert_eq!(s.get_setting("theme").unwrap().as_deref(), Some("light"));
    }

    #[test]
    fn a_delete_is_visible_immediately() {
        let s = Store::open_in_memory().unwrap();
        s.set_setting("theme", "dark").unwrap();
        assert_eq!(s.get_setting("theme").unwrap().as_deref(), Some("dark"));
        s.delete_setting("theme").unwrap();
        assert_eq!(s.get_setting("theme").unwrap(), None);
    }

    #[test]
    fn caching_a_miss_does_not_hide_a_later_write() {
        // Order matters: read (caches None) -> write -> read.
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get_setting("pinned_commands").unwrap(), None);
        s.set_setting("pinned_commands", "a,b").unwrap();
        assert_eq!(
            s.get_setting("pinned_commands").unwrap().as_deref(),
            Some("a,b")
        );
    }

    #[test]
    fn an_external_write_is_visible_after_invalidation() {
        // The sibling-CLI case the TUI drives off `data_version`. Two
        // connections to one file: B writes, A must see it once invalidated.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let a = Store::open(&path).unwrap();
        let b = Store::open(&path).unwrap();

        a.set_setting("theme", "dark").unwrap();
        assert_eq!(a.get_setting("theme").unwrap().as_deref(), Some("dark"));

        b.set_setting("theme", "light").unwrap();
        a.invalidate_settings_cache();
        assert_eq!(a.get_setting("theme").unwrap().as_deref(), Some("light"));
    }

    #[test]
    fn list_settings_always_reads_through() {
        // `list_settings` is not on the render path and bypasses the cache, so
        // it must reflect an external write with no invalidation at all.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let a = Store::open(&path).unwrap();
        let b = Store::open(&path).unwrap();

        a.get_setting("theme").unwrap();
        b.set_setting("theme", "light").unwrap();
        assert_eq!(
            a.list_settings().unwrap(),
            vec![("theme".to_string(), "light".to_string())]
        );
    }
}
