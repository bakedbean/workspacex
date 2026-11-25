//! `wsx new` arguments.

use super::{Args, is_help_flag};
use crate::cli::action::{CliAction, HelpTopic};
use crate::error::{Error, Result};
use crate::pty::session::AgentKind;

fn usage(msg: impl Into<String>) -> Error {
    Error::Usage {
        group: None,
        msg: msg.into(),
    }
}

/// Flags are only read before the repo; everything after it is the task.
pub(super) fn parse_new(it: &mut Args) -> Result<CliAction> {
    let mut no_yolo = false;
    let mut agent: Option<String> = None;
    let mut repo: Option<String> = None;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            a if is_help_flag(a) => return Ok(CliAction::Help(HelpTopic::Group("new"))),
            "--no-yolo" => no_yolo = true,
            "--agent" => {
                let kinds = AgentKind::ALL.map(|k| k.display_name());
                let a = it
                    .next()
                    .ok_or_else(|| usage(format!("--agent needs value ({})", kinds.join(", "))))?;
                if !kinds.contains(&a.as_str()) {
                    return Err(usage(format!(
                        "--agent must be one of [{}], got '{a}'",
                        kinds.join(", ")
                    )));
                }
                agent = Some(a);
            }
            "--worker" => {
                let job = it
                    .next()
                    .ok_or_else(|| usage("--worker needs a job file"))?;
                return Ok(CliAction::NewWorker { job: job.into() });
            }
            a if a.starts_with("--") => return Err(usage(format!("unknown arg: {a}"))),
            _ => {
                repo = Some(arg);
                break;
            }
        }
    }
    let task = it.collect::<Vec<_>>().join(" ");
    Ok(CliAction::New {
        repo,
        task: (!task.trim().is_empty()).then_some(task),
        no_yolo,
        agent,
    })
}
