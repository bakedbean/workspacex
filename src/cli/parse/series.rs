//! `wsx series …`: show or title the current workspace's series.

use super::{Args, parse_read_flags};
use crate::cli::action::CliAction;
use crate::error::{Error, Result};

const TITLE_USAGE: &str = "usage: wsx series title <text> [--workspace <repo>/<slug>]";

pub(in crate::cli) fn parse_series(it: &mut Args) -> Result<CliAction> {
    let first = it.next();
    match first.as_deref() {
        Some("title") => {
            let mut workspace = None;
            let mut words: Vec<String> = Vec::new();
            while let Some(arg) = it.next() {
                if arg == "--workspace" {
                    workspace = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: TITLE_USAGE.into(),
                    })?);
                } else {
                    words.push(arg);
                }
            }
            if words.is_empty() {
                return Err(Error::Usage {
                    group: None,
                    msg: TITLE_USAGE.into(),
                });
            }
            Ok(CliAction::SeriesTitle {
                title: words.join(" "),
                workspace,
            })
        }
        None | Some("show") => show(None, it),
        Some(flag) if flag.starts_with('-') => show(first, it),
        Some(other) => Err(Error::Usage {
            group: None,
            msg: format!("unknown series subcommand: {other}"),
        }),
    }
}

fn show(first: Option<String>, it: &mut Args) -> Result<CliAction> {
    let rest: Vec<String> = first.into_iter().chain(it).collect();
    let f = parse_read_flags(
        &mut rest.into_iter(),
        "series [show] [--workspace <repo>/<slug>] [--json]",
        true,
    )?;
    Ok(CliAction::SeriesShow {
        workspace: f.workspace,
        json: f.json,
    })
}
