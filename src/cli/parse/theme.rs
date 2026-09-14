//! `wsx theme` — the bar theme file.

use super::Args;
use crate::cli::action::CliAction;
use crate::error::{Error, Result};
use std::path::PathBuf;

pub(in crate::cli) fn parse_theme(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("check") => Ok(CliAction::ThemeCheck {
            path: it.next().map(PathBuf::from),
        }),
        Some("path") => Ok(CliAction::ThemePath),
        Some("init") => Ok(CliAction::ThemeInit),
        Some(other) => Err(Error::Usage {
            group: None,
            msg: format!("unknown theme command: {other}"),
        }),
        None => Err(Error::Usage {
            group: None,
            msg: "usage: wsx theme <check [path] | path | init>".into(),
        }),
    }
}
