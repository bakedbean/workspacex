//! Writes from the detail-bar layout editor (`b` on the dashboard).
//!
//! The editor changes one `containers` list: the selected repo's own
//! override when it has one, else the global `detail_bar_config`. Every
//! edit is saved immediately, so the bar behind the drawer follows along.
//!
//! Only the `containers` key is rewritten: other fields, including any
//! this version doesn't know, are kept as stored.
//!
//! The dragged column widths (`detail_bar_widths`) are one global
//! setting whichever repo's bar is showing, as with dragging itself, so
//! container edits reshape them in either scope.

use super::App;
use crate::config::detail_bar_config::{DetailBarConfig, DetailBarOverride};
use crate::config::detail_bar_layout::Containers;
use crate::data::store::RepoId;
use crate::error::{Error, Result};
use serde_json::{Map, Value};

/// Which stored layout the editor writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutScope {
    Global,
    /// A repo whose `detail_bar_config` override sets `containers`.
    Repo(RepoId),
}

/// How a container edit reshapes the dragged column widths, so a
/// column keeps its size when its neighbours change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthsEdit {
    Keep,
    /// A container was inserted at this index.
    Insert(usize),
    /// The container at this index was removed.
    Remove(usize),
    /// These two containers traded places.
    Swap(usize, usize),
    /// The layout was replaced wholesale (reset): forget the widths.
    Clear,
}

/// The layout the editor should open on for `repo` (the selection's
/// repo, if any), and where it is stored.
pub(crate) fn layout_for(app: &App, repo: Option<RepoId>) -> (LayoutScope, Containers) {
    if let Some(repo) = repo.and_then(|id| app.repos.iter().find(|r| r.id == id))
        && repo_override(repo.detail_bar_config.as_deref())
            .containers
            .is_some()
    {
        let cfg = crate::config::detail_bar_config::resolve(repo, &app.store);
        return (LayoutScope::Repo(repo.id), cfg.containers);
    }
    let cfg = crate::config::detail_bar_config::resolve_global_only(&app.store);
    (LayoutScope::Global, cfg.containers)
}

/// The layout stored for `scope` now, as the bar resolves it. The
/// editor re-reads this after a failed save, so it never shows a layout
/// that differs from what is stored.
pub(crate) fn stored_layout(app: &App, scope: LayoutScope) -> Containers {
    match scope {
        // Read the repo from the store, not `app.repos`: the save that
        // failed may have written the repo and then failed to reload it.
        LayoutScope::Repo(id) => {
            let fresh = app.store.repos().ok();
            let repos = fresh.as_ref().unwrap_or(&app.repos);
            match repos.iter().find(|r| r.id == id) {
                Some(repo) => {
                    crate::config::detail_bar_config::resolve(repo, &app.store).containers
                }
                None => {
                    crate::config::detail_bar_config::resolve_global_only(&app.store).containers
                }
            }
        }
        LayoutScope::Global => {
            crate::config::detail_bar_config::resolve_global_only(&app.store).containers
        }
    }
}

/// Store `containers` in `scope`, and reshape the saved column widths
/// by `widths`. Refuses to overwrite stored JSON that doesn't parse as
/// an object, rather than replacing a hand edit with defaults.
pub(crate) fn save_layout(
    app: &mut App,
    scope: LayoutScope,
    containers: &Containers,
    widths: WidthsEdit,
) -> Result<()> {
    let mut containers = containers.clone();
    containers.truncate(crate::config::detail_bar_layout::MAX_CONTAINERS);
    let value = serde_json::to_value(&containers)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: {e}")))?;
    match scope {
        LayoutScope::Global => {
            let stored = app.store.get_setting("detail_bar_config")?;
            let raw = with_containers::<DetailBarConfig>(
                stored.as_deref(),
                value,
                "wsx config edit detail_bar_config",
            )?;
            app.store.set_setting("detail_bar_config", &raw)?;
        }
        LayoutScope::Repo(id) => {
            let stored = app
                .repos
                .iter()
                .find(|r| r.id == id)
                .and_then(|r| r.detail_bar_config.clone());
            let raw = with_containers::<DetailBarOverride>(
                stored.as_deref(),
                value,
                "the repo's detail_bar_config (s on the dashboard)",
            )?;
            app.store.set_repo_detail_bar_config(id, Some(&raw))?;
            app.repos = app.store.repos()?;
        }
    }
    reshape_widths(app, widths, containers.len())
}

/// `stored` (a JSON object, or nothing) with its `containers` key set to
/// `containers`, pretty-printed. `fix` names where to repair JSON that
/// isn't an object.
///
/// `T` is the typed config the renderer reads this JSON as; the result
/// must parse as it, or the bar would ignore the edit (a bad known field
/// elsewhere in the object).
fn with_containers<T: serde::de::DeserializeOwned>(
    stored: Option<&str>,
    containers: Value,
    fix: &str,
) -> Result<String> {
    let mut obj = match stored.map(str::trim).filter(|s| !s.is_empty()) {
        None => Map::new(),
        Some(raw) => match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(obj)) => obj,
            _ => {
                return Err(Error::UserInput(format!(
                    "the stored detail_bar_config isn't a valid JSON object; fix it with {fix}"
                )));
            }
        },
    };
    obj.insert("containers".into(), containers);
    let value = Value::Object(obj);
    if let Err(e) = serde_json::from_value::<T>(value.clone()) {
        return Err(Error::UserInput(format!(
            "the stored detail_bar_config is invalid ({e}); fix it with {fix}"
        )));
    }
    serde_json::to_string_pretty(&value)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: {e}")))
}

/// A repo's stored override; malformed JSON reads as empty, as the
/// renderer treats it.
fn repo_override(raw: Option<&str>) -> DetailBarOverride {
    raw.and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

/// Apply `edit` to the dragged widths when they still describe the
/// layout before it; widths that already don't match are left alone
/// (the bar shares evenly for them anyway).
fn reshape_widths(app: &mut App, edit: WidthsEdit, new_len: usize) -> Result<()> {
    if edit == WidthsEdit::Clear {
        app.store.delete_setting(DetailBarConfig::WIDTHS_SETTING)?;
        app.dashboard.detail_widths = None;
        return Ok(());
    }
    let Some(mut widths) = app.dashboard.detail_widths.clone() else {
        return Ok(());
    };
    let old_len = match edit {
        WidthsEdit::Keep | WidthsEdit::Swap(..) | WidthsEdit::Clear => new_len,
        WidthsEdit::Insert(_) => new_len - 1,
        WidthsEdit::Remove(_) => new_len + 1,
    };
    if widths.len() != old_len {
        return Ok(());
    }
    match edit {
        WidthsEdit::Keep | WidthsEdit::Clear => return Ok(()),
        WidthsEdit::Insert(at) => {
            let avg =
                widths.iter().map(|&w| u32::from(w)).sum::<u32>() / widths.len().max(1) as u32;
            widths.insert(at.min(widths.len()), (avg as u16).max(1));
        }
        WidthsEdit::Remove(at) => {
            widths.remove(at);
        }
        WidthsEdit::Swap(a, b) => widths.swap(a, b),
    }
    let setting = DetailBarConfig::WIDTHS_SETTING;
    if widths.is_empty() {
        app.store.delete_setting(setting)?;
        app.dashboard.detail_widths = None;
    } else {
        app.store
            .set_setting(setting, &DetailBarConfig::format_widths(&widths))?;
        app.dashboard.detail_widths = Some(widths);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::Store;
    use std::path::PathBuf;

    fn app() -> (App, RepoId) {
        let store = Store::open_in_memory().unwrap();
        let rid = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        (
            App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap(),
            rid,
        )
    }

    fn layout(spec: &[&[&str]]) -> Containers {
        spec.iter()
            .map(|c| c.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn opens_on_global_default_without_overrides() {
        let (app, rid) = app();
        let (scope, containers) = layout_for(&app, Some(rid));
        assert_eq!(scope, LayoutScope::Global);
        assert_eq!(containers, DetailBarConfig::default().containers);
    }

    #[test]
    fn repo_override_without_containers_still_edits_global() {
        let (mut app, rid) = app();
        app.store
            .set_repo_detail_bar_config(rid, Some(r#"{"visible": false}"#))
            .unwrap();
        app.refresh().unwrap();
        assert_eq!(layout_for(&app, Some(rid)).0, LayoutScope::Global);
    }

    #[test]
    fn global_save_keeps_other_fields() {
        let (mut app, _) = app();
        app.store
            .set_setting("detail_bar_config", r#"{"bottom_rule": false}"#)
            .unwrap();
        let l = layout(&[&["agents"], &[]]);
        save_layout(&mut app, LayoutScope::Global, &l, WidthsEdit::Keep).unwrap();
        let cfg = crate::config::detail_bar_config::resolve_global_only(&app.store);
        assert_eq!(cfg.containers, l);
        assert!(!cfg.bottom_rule);
    }

    #[test]
    fn repo_save_writes_only_the_override() {
        let (mut app, rid) = app();
        app.store
            .set_repo_detail_bar_config(
                rid,
                Some(r#"{"containers": [["processes"]], "visible": false}"#),
            )
            .unwrap();
        app.refresh().unwrap();
        let (scope, containers) = layout_for(&app, Some(rid));
        assert_eq!(scope, LayoutScope::Repo(rid));
        assert_eq!(containers, layout(&[&["processes"]]));

        let l = layout(&[&["processes", "agents"]]);
        save_layout(&mut app, scope, &l, WidthsEdit::Keep).unwrap();
        let repo = app.repos.iter().find(|r| r.id == rid).unwrap();
        let ovr: DetailBarOverride =
            serde_json::from_str(repo.detail_bar_config.as_deref().unwrap()).unwrap();
        assert_eq!(ovr.containers, Some(l));
        assert_eq!(ovr.visible, Some(false));
        assert!(
            app.store
                .get_setting("detail_bar_config")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn container_edits_reshape_matching_widths() {
        let (mut app, _) = app();
        app.dashboard.detail_widths = Some(vec![30, 10, 20]);
        let four = layout(&[&[], &[], &[], &[]]);
        save_layout(&mut app, LayoutScope::Global, &four, WidthsEdit::Insert(1)).unwrap();
        assert_eq!(app.dashboard.detail_widths, Some(vec![30, 20, 10, 20]));

        let mut three = four.clone();
        three.pop();
        save_layout(&mut app, LayoutScope::Global, &three, WidthsEdit::Remove(0)).unwrap();
        assert_eq!(app.dashboard.detail_widths, Some(vec![20, 10, 20]));

        save_layout(
            &mut app,
            LayoutScope::Global,
            &three,
            WidthsEdit::Swap(0, 1),
        )
        .unwrap();
        assert_eq!(app.dashboard.detail_widths, Some(vec![10, 20, 20]));
        assert_eq!(
            app.store
                .get_setting(DetailBarConfig::WIDTHS_SETTING)
                .unwrap()
                .as_deref(),
            Some("10,20,20")
        );
    }

    #[test]
    fn save_keeps_fields_it_does_not_know() {
        let (mut app, _) = app();
        app.store
            .set_setting(
                "detail_bar_config",
                r#"{"bottom_rule": false, "future_field": {"x": 1}}"#,
            )
            .unwrap();
        let l = layout(&[&["agents"]]);
        save_layout(&mut app, LayoutScope::Global, &l, WidthsEdit::Keep).unwrap();
        let raw = app.store.get_setting("detail_bar_config").unwrap().unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["future_field"]["x"], 1);
        assert_eq!(v["bottom_rule"], false);
        assert_eq!(v["containers"], serde_json::json!([["agents"]]));
    }

    #[test]
    fn malformed_stored_json_is_not_overwritten() {
        let (mut app, rid) = app();
        app.store
            .set_setting("detail_bar_config", "{not json")
            .unwrap();
        let l = layout(&[&["agents"]]);
        let err = save_layout(&mut app, LayoutScope::Global, &l, WidthsEdit::Keep).unwrap_err();
        assert!(err.to_string().contains("wsx config edit"), "{err}");
        assert_eq!(
            app.store
                .get_setting("detail_bar_config")
                .unwrap()
                .as_deref(),
            Some("{not json")
        );
        // What the editor falls back to is what the bar shows: defaults.
        assert_eq!(
            stored_layout(&app, LayoutScope::Global),
            DetailBarConfig::default().containers
        );

        app.store
            .set_repo_detail_bar_config(rid, Some("[1, 2]"))
            .unwrap();
        app.refresh().unwrap();
        assert!(save_layout(&mut app, LayoutScope::Repo(rid), &l, WidthsEdit::Keep).is_err());
    }

    #[test]
    fn invalid_known_fields_are_not_saved_over() {
        let (mut app, rid) = app();
        let l = layout(&[&["agents"]]);
        app.store
            .set_setting("detail_bar_config", r#"{"visible": "bad"}"#)
            .unwrap();
        let err = save_layout(&mut app, LayoutScope::Global, &l, WidthsEdit::Keep).unwrap_err();
        assert!(err.to_string().contains("invalid"), "{err}");
        assert_eq!(
            app.store
                .get_setting("detail_bar_config")
                .unwrap()
                .as_deref(),
            Some(r#"{"visible": "bad"}"#)
        );

        let bad = r#"{"containers": [["processes"]], "visible": "bad"}"#;
        app.store
            .set_repo_detail_bar_config(rid, Some(bad))
            .unwrap();
        app.refresh().unwrap();
        assert!(save_layout(&mut app, LayoutScope::Repo(rid), &l, WidthsEdit::Keep).is_err());
        let repo = app
            .store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == rid)
            .unwrap();
        assert_eq!(repo.detail_bar_config.as_deref(), Some(bad));
    }

    #[test]
    fn stored_layout_reads_the_repo_from_the_store() {
        let (mut app, rid) = app();
        app.refresh().unwrap();
        // The store has moved on; the cached `app.repos` hasn't.
        app.store
            .set_repo_detail_bar_config(rid, Some(r#"{"containers": [["agents"]]}"#))
            .unwrap();
        assert_eq!(
            stored_layout(&app, LayoutScope::Repo(rid)),
            layout(&[&["agents"]])
        );
    }

    #[test]
    fn clear_forgets_the_widths() {
        let (mut app, _) = app();
        app.store
            .set_setting(DetailBarConfig::WIDTHS_SETTING, "1,2")
            .unwrap();
        app.dashboard.detail_widths = Some(vec![1, 2]);
        let l = DetailBarConfig::default().containers;
        save_layout(&mut app, LayoutScope::Global, &l, WidthsEdit::Clear).unwrap();
        assert_eq!(app.dashboard.detail_widths, None);
        assert!(
            app.store
                .get_setting(DetailBarConfig::WIDTHS_SETTING)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn stale_widths_are_left_alone() {
        let (mut app, _) = app();
        app.dashboard.detail_widths = Some(vec![1, 2]);
        let l = layout(&[&[], &[], &[], &[]]);
        save_layout(&mut app, LayoutScope::Global, &l, WidthsEdit::Insert(0)).unwrap();
        assert_eq!(app.dashboard.detail_widths, Some(vec![1, 2]));
    }
}
