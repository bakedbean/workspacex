//! Raise a running TUI's terminal window after a jump has selected its
//! workspace: hyprctl on Hyprland, a KWin script over D-Bus on KDE Plasma.
//! Best-effort throughout, since the selection has already happened.

use std::process::Command;

fn ppid_from_stat(stat: &str) -> Option<u32> {
    // comm is parenthesized and may itself contain ')' — split on the LAST ')'.
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().nth(1)?.parse().ok() // state, ppid, ...
}

fn ancestor_pids(pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut current = pid;
    while chain.len() < 32 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{current}/stat")) else {
            break;
        };
        let Some(ppid) = ppid_from_stat(&stat) else {
            break;
        };
        if ppid <= 1 {
            break;
        }
        chain.push(ppid);
        current = ppid;
    }
    chain
}

fn client_pid_for_chain(clients_json: &str, chain: &[u32]) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(clients_json).ok()?;
    let clients = v.as_array()?;
    // chain is self→ancestors; the first chain pid with a window wins.
    chain.iter().copied().find(|pid| {
        clients
            .iter()
            .any(|c| c.get("pid").and_then(|p| p.as_u64()) == Some(u64::from(*pid)))
    })
}

pub(crate) fn focus_window_of(tui_pid: u32) {
    if use_kwin(
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
    ) {
        focus_kwin_window(&ancestor_pids(tui_pid));
        return;
    }
    let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
        return; // not Hyprland — selection still happened
    };
    let chain = ancestor_pids(tui_pid);
    if let Some(pid) = client_pid_for_chain(&String::from_utf8_lossy(&out.stdout), &chain) {
        let _ = Command::new("hyprctl")
            .args(["dispatch", "focuswindow", &format!("pid:{pid}")])
            .status();
    }
}

/// Whether to focus through KWin rather than hyprctl: a KDE session per
/// `$XDG_CURRENT_DESKTOP` (a colon-separated list; Plasma sets `KDE`), unless
/// Hyprland is running, since some Hyprland setups export `KDE` there too.
fn use_kwin(hyprland: bool, current_desktop: Option<&str>) -> bool {
    !hyprland
        && current_desktop.is_some_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("kde")))
}

/// The KWin script's name, which is also how it unloads itself.
const KWIN_SCRIPT_NAME: &str = "wsx-focus";

/// A KWin script that activates the window of the first pid in `chain` that
/// owns one (the same rule as [`client_pid_for_chain`]), then unloads itself.
/// Only normal windows count, as `hyprctl clients` lists only those: the
/// chain usually passes through plasmashell, whose panels and desktop are
/// windows too.
/// Activating from a script sidesteps KWin's focus-stealing prevention,
/// which would otherwise refuse a request from a background process.
fn kwin_focus_script(chain: &[u32]) -> String {
    let pids = chain
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"const chain = [{pids}];
const windows = workspace.windowList();
for (const pid of chain) {{
    const w = windows.find((w) => w.normalWindow && w.pid === pid);
    if (w) {{
        workspace.activeWindow = w;
        break;
    }}
}}
callDBus("org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "unloadScript", "{KWIN_SCRIPT_NAME}");
"#
    )
}

/// One `dbus-send` call to KWin; the reply on success.
fn kwin_call(path: &str, method: &str, args: &[String]) -> Option<String> {
    let out = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.kde.KWin",
            path,
            method,
        ])
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The script id in a `loadScript` reply (`   int32 7`). KWin answers -1
/// when it refuses the script, which fails the unsigned parse.
fn parse_script_id(reply: &str) -> Option<u32> {
    reply.split("int32").nth(1)?.trim().parse().ok()
}

/// Focus the TUI's window under KWin (Plasma), where there is no hyprctl:
/// load [`kwin_focus_script`] over D-Bus and run it. Best-effort like the
/// Hyprland path — the selection already happened.
fn focus_kwin_window(chain: &[u32]) {
    // KWin reads the file asynchronously after `run` returns, so it can't be
    // removed here; each jump replaces it atomically, so a quick second jump
    // can't hand KWin a half-written file.
    let path = crate::app::ipc::socket_dir().join("kwin-focus.js");
    if crate::desktop::install_support::write_atomic(&path, &kwin_focus_script(chain)).is_err() {
        return;
    }
    let name = format!("string:{KWIN_SCRIPT_NAME}");
    // A copy that failed to unload itself would block loading this one.
    let _ = kwin_call(
        "/Scripting",
        "org.kde.kwin.Scripting.unloadScript",
        std::slice::from_ref(&name),
    );
    let Some(id) = kwin_call(
        "/Scripting",
        "org.kde.kwin.Scripting.loadScript",
        &[format!("string:{}", path.display()), name],
    )
    .and_then(|reply| parse_script_id(&reply)) else {
        return;
    };
    let _ = kwin_call(
        &format!("/Scripting/Script{id}"),
        "org.kde.kwin.Script.run",
        &[],
    );
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    #[test]
    fn client_pid_prefers_closest_ancestor() {
        let clients = r#"[
            {"address":"0x1","pid":900,"class":"Alacritty"},
            {"address":"0x2","pid":300,"class":"ghostty"}
        ]"#;
        // chain is ordered self → parent → grandparent
        assert_eq!(client_pid_for_chain(clients, &[100, 300, 900]), Some(300));
        assert_eq!(client_pid_for_chain(clients, &[100, 200]), None);
        assert_eq!(client_pid_for_chain("not json", &[100]), None);
        assert_eq!(client_pid_for_chain("[]", &[100]), None);
    }

    #[test]
    fn ancestor_pids_walks_proc() {
        let chain = ancestor_pids(std::process::id());
        assert_eq!(chain.first(), Some(&std::process::id()));
        assert!(
            chain.len() >= 2,
            "expected at least self + parent, got {chain:?}"
        );
        assert!(chain.len() <= 32);
    }

    #[test]
    fn kwin_focus_script_walks_chain_in_order_then_unloads() {
        let s = kwin_focus_script(&[100, 300, 900]);
        assert!(s.contains("const chain = [100, 300, 900];"), "{s}");
        assert!(s.contains("w.normalWindow && w.pid === pid"), "{s}");
        assert!(s.contains("workspace.activeWindow = w;"), "{s}");
        assert!(s.contains(r#""unloadScript", "wsx-focus");"#), "{s}");
    }

    #[test]
    fn parses_kwin_script_id() {
        let reply = "method return time=1.2 sender=:1.5 -> destination=:1.9 serial=4 \
                     reply_serial=2\n   int32 7\n";
        assert_eq!(parse_script_id(reply), Some(7));
        assert_eq!(parse_script_id("method return\n   int32 -1\n"), None);
        assert_eq!(parse_script_id(""), None);
    }

    #[test]
    fn kwin_only_in_kde_sessions_without_hyprland() {
        assert!(use_kwin(false, Some("KDE")));
        assert!(use_kwin(false, Some("ubuntu:KDE")));
        assert!(!use_kwin(false, Some("Hyprland")));
        assert!(!use_kwin(false, None));
        // Hyprland exporting KDE for Qt theming keeps the hyprctl path.
        assert!(!use_kwin(true, Some("Hyprland:KDE")));
    }

    #[test]
    fn stat_ppid_parses_despite_parens_in_comm() {
        assert_eq!(ppid_from_stat("123 (weird) name) S 77 123 123 0"), Some(77));
        assert_eq!(ppid_from_stat("123 (simple) S 1 123"), Some(1));
        assert_eq!(ppid_from_stat("garbage"), None);
    }
}
