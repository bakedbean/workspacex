//! `wsx menubar refresh`: detached throttled sweep refreshing git facts
//! and PR state into scm_cache. Silent by contract.

use crate::data::store::Store;
use crate::error::Result;

pub async fn run_refresh(store: &Store) -> Result<()> {
    crate::desktop::rows::refresh_git_facts(store).await?;
    crate::desktop::rows::run_refresh_prs(store).await
}
