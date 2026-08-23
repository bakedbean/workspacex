//! Which per-repo setting a text edit is targeting, and applying it.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSettingField {
    RepoName,
    BranchPrefix,
    BaseBranch,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
    PinnedCommands,
    RelatedRepos,
    DetailBarConfig,
}

impl RepoSettingField {
    pub const ALL: [Self; 9] = [
        Self::RepoName,
        Self::BranchPrefix,
        Self::BaseBranch,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
        Self::RelatedRepos,
        Self::DetailBarConfig,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RepoName => "name",
            Self::BranchPrefix => "branch_prefix",
            Self::BaseBranch => "base_branch",
            Self::CustomInstructions => "custom_instructions",
            Self::SetupScript => "setup_script",
            Self::ArchiveScript => "archive_script",
            Self::PinnedCommands => "pinned_commands",
            Self::RelatedRepos => "related_repos",
            Self::DetailBarConfig => "detail_bar_config",
        }
    }
}

pub(crate) fn apply_repo_setting(
    app: &mut App,
    repo_id: crate::data::store::RepoId,
    field: RepoSettingField,
    value: &str,
) -> Result<()> {
    let trimmed = value.trim();
    let opt = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    match field {
        RepoSettingField::RepoName => {
            app.store.set_repo_name(repo_id, trimmed)?;
            Ok(())
        }
        RepoSettingField::BranchPrefix => app.store.set_repo_branch_prefix(repo_id, trimmed),
        RepoSettingField::BaseBranch => app.store.set_repo_base_branch(repo_id, opt),
        RepoSettingField::CustomInstructions => {
            app.store.set_repo_custom_instructions(repo_id, opt)
        }
        RepoSettingField::SetupScript => app.store.set_repo_setup_script(repo_id, opt),
        RepoSettingField::ArchiveScript => app.store.set_repo_archive_script(repo_id, opt),
        RepoSettingField::PinnedCommands => app.store.set_repo_pinned_commands(repo_id, opt),
        RepoSettingField::RelatedRepos => app.store.set_repo_related_repos(repo_id, opt),
        RepoSettingField::DetailBarConfig => {
            if trimmed.is_empty() {
                app.store.set_repo_detail_bar_config(repo_id, None)
            } else {
                // Validate. Use DetailBarOverride (not DetailBarConfig)
                // because per-repo entries are partial overrides.
                match serde_json::from_str::<crate::config::detail_bar_config::DetailBarOverride>(
                    trimmed,
                ) {
                    Ok(_) => app.store.set_repo_detail_bar_config(repo_id, Some(trimmed)),
                    Err(e) => Err(crate::error::Error::UserInput(format!(
                        "detail_bar_config is not valid JSON: {e}"
                    ))),
                }
            }
        }
    }
}
