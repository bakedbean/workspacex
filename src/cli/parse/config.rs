//! `wsx config` — global settings.

use super::Args;
use crate::cli::action::{CliAction, ValueSource};
use crate::error::{Error, Result};

pub(in crate::cli) fn known_setting_key(k: &str) -> bool {
    crate::config::settings::is_known(k)
}

pub(in crate::cli) fn parse_config(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("get") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config get <key>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            Ok(CliAction::ConfigGet { key })
        }
        Some("set") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config set <key> <value-or-@file>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config set <key> <value-or-@file>".into(),
            })?;
            Ok(CliAction::ConfigSet {
                key,
                source: ValueSource::from_arg(value),
            })
        }
        Some("list") => Ok(CliAction::ConfigList),
        Some("edit") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config edit <key>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            Ok(CliAction::ConfigEdit { key })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown config command: {cmd}"),
                None => "missing config command".into(),
            },
        }),
    }
}
