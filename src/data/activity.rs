//! Activity-bucket telemetry persistence (the `activity_buckets` table):
//! hourly max-live-session counts powering the usage sparkline.

use crate::data::store::Store;
use crate::error::Result;

impl Store {
    pub fn set_activity_bucket(&self, hour_epoch: u64, max_live: u32) -> Result<()> {
        self.conn().execute(
            "INSERT INTO activity_buckets (hour_epoch, max_live) VALUES (?1, ?2)
             ON CONFLICT(hour_epoch) DO UPDATE SET max_live = excluded.max_live",
            rusqlite::params![hour_epoch as i64, max_live as i64],
        )?;
        Ok(())
    }

    /// Return up to `limit` most-recent buckets in ascending hour order.
    pub fn recent_activity_buckets(&self, limit: usize) -> Result<Vec<(u64, u32)>> {
        let mut stmt = self.conn().prepare(
            "SELECT hour_epoch, max_live FROM activity_buckets
             ORDER BY hour_epoch DESC LIMIT ?1",
        )?;
        let mut rows: Vec<(u64, u32)> = stmt
            .query_map(rusqlite::params![limit as i64], |r| {
                let h: i64 = r.get(0)?;
                let m: i64 = r.get(1)?;
                Ok((h as u64, m as u32))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Delete buckets with hour_epoch strictly less than `cutoff`.
    pub fn prune_activity_buckets_before(&self, cutoff: u64) -> Result<()> {
        self.conn().execute(
            "DELETE FROM activity_buckets WHERE hour_epoch < ?1",
            rusqlite::params![cutoff as i64],
        )?;
        Ok(())
    }
}
