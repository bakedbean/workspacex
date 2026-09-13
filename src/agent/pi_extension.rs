//! The pi extension wsx loads on every pi spawn so pi reports which session
//! it is in.
//!
//! pi has no lifecycle hooks and no notify program, but it loads extension
//! files given as `-e <path>`, and an extension sees `session_start` — fired
//! on startup and again after `/new`, `/resume` and forks — with the session
//! manager in hand. This one shells out to `wsx status from-notify --agent
//! pi` with the session id, the same channel Codex's notify uses, so
//! `app::spawn::recorded_resume_id` can resume exactly that session later.
//! The wsx binary path rides in `$WSX_BIN`, set by `build_pi_command`.
//!
//! Written under the wsx app dir and rewritten on drift, like the omp config
//! overlay (`agent::omp_config`).

use crate::agent::skill::install_content_to;
use crate::error::Result;
use std::path::{Path, PathBuf};

pub const EXTENSION_FILE_NAME: &str = "pi-session-report.ts";

/// The env var carrying the wsx binary path into pi (and so into the
/// extension's `pi.exec`).
pub const WSX_BIN_ENV: &str = "WSX_BIN";

pub const EXTENSION_CONTENT: &str = r#"// Written by wsx before every pi launch and loaded with `pi -e <file>`.
// Reports the session pi is in to wsx on every session start (startup,
// /new, /resume, fork) so wsx can resume exactly this session after a
// restart. Do not edit: wsx rewrites it on drift.
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
	pi.on("session_start", async (event, ctx) => {
		const bin = process.env.WSX_BIN;
		if (!bin) return;
		const sm = ctx.sessionManager as { getSessionId?: () => string; getSessionFile?: () => string | undefined };
		let id = sm.getSessionId?.();
		if (!id) {
			// Fall back to the file name: <timestamp>_<id>.jsonl
			const file = sm.getSessionFile?.();
			const stem = file?.split("/").pop()?.replace(/\.jsonl$/, "");
			id = stem?.slice(stem.lastIndexOf("_") + 1);
		}
		if (!id) return;
		const payload = JSON.stringify({ type: "session_start", reason: event.reason, session_id: id });
		try {
			await pi.exec(bin, ["status", "from-notify", "--agent", "pi", payload]);
		} catch {
			// Never let reporting break the session.
		}
	});
}
"#;

pub fn extension_path(app_dir: &Path) -> PathBuf {
    app_dir.join(EXTENSION_FILE_NAME)
}

pub fn ensure_extension(app_dir: &Path) -> Result<PathBuf> {
    let path = extension_path(app_dir);
    install_content_to(&path, EXTENSION_CONTENT)?;
    Ok(path)
}

/// Write (or refresh) the extension under the discovered app dir. `None`
/// when it cannot be written: pi then launches without it and the instance
/// keeps only its minted pin.
pub fn ensure_extension_default() -> Option<PathBuf> {
    let app_dir = crate::config::Dirs::discover().app_dir();
    match ensure_extension(&app_dir) {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::warn!(
                "could not write the pi session-report extension under {}: {e}; \
                 pi sessions will not be tracked across /new",
                app_dir.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_is_written_and_rewritten_on_drift() {
        let dir = tempfile::tempdir().unwrap();
        let path = ensure_extension(dir.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXTENSION_CONTENT);
        std::fs::write(&path, "tampered").unwrap();
        ensure_extension(dir.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXTENSION_CONTENT);
    }

    #[test]
    fn extension_reports_through_from_notify_with_the_session_id() {
        assert!(EXTENSION_CONTENT.contains(r#"pi.on("session_start""#));
        assert!(EXTENSION_CONTENT.contains(r#""status", "from-notify", "--agent", "pi""#));
        assert!(EXTENSION_CONTENT.contains("process.env.WSX_BIN"));
    }
}
