//! `wsx menubar refresh`: detached throttled sweep refreshing git facts
//! and PR state into scm_cache. Silent by contract.

use crate::data::store::Store;
use crate::error::Result;

pub async fn run_refresh(store: &Store) -> Result<()> {
    crate::workspace_rows::refresh_git_facts(store).await?;
    crate::workspace_rows::run_refresh_prs(store).await
}
