//! Raise a running TUI's terminal window after a jump has selected its
//! workspace: hyprctl on Hyprland, a KWin script over D-Bus on KDE Plasma.
//! Best-effort throughout, since the selection has already happened.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

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
    if plasma_session(
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

/// Whether this is a KDE Plasma session, so KWin is the compositor:
/// `$XDG_CURRENT_DESKTOP`, a colon-separated list, names `KDE`, and Hyprland
/// isn't running, since some Hyprland setups export `KDE` there too.
pub(crate) fn plasma_session(hyprland: bool, current_desktop: Option<&str>) -> bool {
    !hyprland
        && current_desktop.is_some_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("kde")))
}

/// Whether `program` is in one of `path`'s directories.
pub(crate) fn on_path(program: &str, path: Option<&std::ffi::OsStr>) -> bool {
    path.is_some_and(|path| std::env::split_paths(path).any(|dir| dir.join(program).is_file()))
}

/// Each jump's KWin script is named `wsx-focus-<id>` and loaded from
/// `kwin-focus-<id>.js` in the socket directory. The id is unique per jump,
/// so a quick second jump never shares a name or file with the first: each
/// script unloads only itself, and KWin can't read one jump's chain for
/// another's.
const KWIN_SCRIPT_PREFIX: &str = "wsx-focus-";
const KWIN_FILE_PREFIX: &str = "kwin-focus-";
const KWIN_FILE_SUFFIX: &str = ".js";

/// A script file older than this was read by KWin long ago. KWin reads each
/// one asynchronously after `run`, so a jump can't remove its own file and
/// leaves it for a later jump to clear.
const KWIN_FILE_MAX_AGE: Duration = Duration::from_secs(60);

/// `<pid>-<nanos>`: unique to this jump.
fn kwin_script_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

/// Script files in `dir` older than [`KWIN_FILE_MAX_AGE`] at `now`, with the
/// name each one's script was loaded under. Newer files may belong to a jump
/// still in flight, so they're left alone.
fn stale_kwin_scripts(dir: &Path, now: SystemTime) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().into_string().ok()?;
            let id = file
                .strip_prefix(KWIN_FILE_PREFIX)?
                .strip_suffix(KWIN_FILE_SUFFIX)?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            let age = now.duration_since(modified).ok()?;
            (age > KWIN_FILE_MAX_AGE).then(|| (entry.path(), format!("{KWIN_SCRIPT_PREFIX}{id}")))
        })
        .collect()
}

/// A KWin script that activates the window of the first pid in `chain` that
/// owns one (the same rule as [`client_pid_for_chain`]), then unloads itself.
/// Only normal windows count, as `hyprctl clients` lists only those: the
/// chain usually passes through plasmashell, whose panels and desktop are
/// windows too.
/// Activating from a script sidesteps KWin's focus-stealing prevention,
/// which would otherwise refuse a request from a background process.
fn kwin_focus_script(chain: &[u32], name: &str) -> String {
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
callDBus("org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "unloadScript", "{name}");
"#
    )
}

/// A command-line D-Bus client to reach KWin through. `dbus-send` isn't
/// always installed (Fedora packages it separately, in dbus-tools), while
/// Plasma ships `qdbus6`, and GLib's `gdbus` is nearly everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DbusClient {
    DbusSend,
    Qdbus(&'static str),
    Gdbus,
}

/// Tried in order; the first one on `PATH` is used.
const DBUS_CLIENTS: [DbusClient; 4] = [
    DbusClient::DbusSend,
    DbusClient::Qdbus("qdbus6"),
    DbusClient::Qdbus("qdbus"),
    DbusClient::Gdbus,
];

impl DbusClient {
    fn program(self) -> &'static str {
        match self {
            DbusClient::DbusSend => "dbus-send",
            DbusClient::Qdbus(program) => program,
            DbusClient::Gdbus => "gdbus",
        }
    }

    /// Arguments calling `method` (`interface.member`) on KWin's object
    /// `path`, passing each of `args` as a string.
    fn argv(self, path: &str, method: &str, args: &[&str]) -> Vec<String> {
        let mut argv: Vec<String> = match self {
            DbusClient::DbusSend => vec![
                "--session".into(),
                "--print-reply".into(),
                "--dest=org.kde.KWin".into(),
                path.into(),
                method.into(),
            ],
            DbusClient::Qdbus(_) => vec!["org.kde.KWin".into(), path.into(), method.into()],
            DbusClient::Gdbus => vec![
                "call".into(),
                "--session".into(),
                "--dest".into(),
                "org.kde.KWin".into(),
                "--object-path".into(),
                path.into(),
                "--method".into(),
                method.into(),
            ],
        };
        argv.extend(args.iter().map(|arg| match self {
            DbusClient::DbusSend => format!("string:{arg}"),
            DbusClient::Qdbus(_) | DbusClient::Gdbus => (*arg).to_string(),
        }));
        argv
    }

    /// The script id in this client's `loadScript` reply: `   int32 7`
    /// (dbus-send), `7` (qdbus) or `(7,)` (gdbus). KWin answers -1 when it
    /// refuses the script, which fails the unsigned parse.
    fn parse_script_id(self, reply: &str) -> Option<u32> {
        let id = match self {
            DbusClient::DbusSend => reply.split("int32").nth(1)?,
            DbusClient::Qdbus(_) => reply,
            DbusClient::Gdbus => reply
                .trim()
                .strip_prefix('(')?
                .strip_suffix(')')?
                .trim_end_matches(','),
        };
        id.trim().parse().ok()
    }
}

/// The first of [`DBUS_CLIENTS`] with a program in one of `path`'s
/// directories.
fn find_dbus_client(path: Option<&std::ffi::OsStr>) -> Option<DbusClient> {
    DBUS_CLIENTS
        .into_iter()
        .find(|client| on_path(client.program(), path))
}

/// One call to KWin through `client`; the reply on success.
fn kwin_call(client: DbusClient, path: &str, method: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(client.program())
        .args(client.argv(path, method, args))
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Focus the TUI's window under KWin (Plasma), where there is no hyprctl:
/// load [`kwin_focus_script`] over D-Bus and run it. Best-effort like the
/// Hyprland path — the selection already happened.
fn focus_kwin_window(chain: &[u32]) {
    let Some(client) = find_dbus_client(std::env::var_os("PATH").as_deref()) else {
        let msg = "can't raise the TUI's window: no D-Bus client to reach KWin \
                   (install dbus-send, qdbus6, qdbus or gdbus)";
        tracing::warn!("{msg}");
        eprintln!("wsx: {msg}");
        return;
    };
    let dir = crate::app::ipc::socket_dir();
    // Clear earlier jumps' files, and unload any of their scripts that never
    // got to unload themselves.
    for (stale, stale_name) in stale_kwin_scripts(&dir, SystemTime::now()) {
        let _ = std::fs::remove_file(stale);
        let _ = kwin_call(
            client,
            "/Scripting",
            "org.kde.kwin.Scripting.unloadScript",
            &[&stale_name],
        );
    }
    let id = kwin_script_id();
    let name = format!("{KWIN_SCRIPT_PREFIX}{id}");
    let path = dir.join(format!("{KWIN_FILE_PREFIX}{id}{KWIN_FILE_SUFFIX}"));
    if crate::desktop::install_support::write_atomic(&path, &kwin_focus_script(chain, &name))
        .is_err()
    {
        return;
    }
    let Some(id) = kwin_call(
        client,
        "/Scripting",
        "org.kde.kwin.Scripting.loadScript",
        &[&path.display().to_string(), &name],
    )
    .and_then(|reply| client.parse_script_id(&reply)) else {
        // Never loaded, so KWin won't read it.
        let _ = std::fs::remove_file(&path);
        return;
    };
    let _ = kwin_call(
        client,
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
    fn kwin_focus_script_walks_chain_in_order_then_unloads_itself() {
        let s = kwin_focus_script(&[100, 300, 900], "wsx-focus-42-7");
        assert!(s.contains("const chain = [100, 300, 900];"), "{s}");
        assert!(s.contains("w.normalWindow && w.pid === pid"), "{s}");
        assert!(s.contains("workspace.activeWindow = w;"), "{s}");
        // Its own name only, so it can't unload a later jump's script.
        assert!(s.contains(r#""unloadScript", "wsx-focus-42-7");"#), "{s}");
    }

    #[test]
    fn each_jump_gets_its_own_script_id() {
        assert_ne!(kwin_script_id(), kwin_script_id());
        assert!(kwin_script_id().starts_with(&format!("{}-", std::process::id())));
    }

    #[test]
    fn only_old_kwin_script_files_count_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let old = now - KWIN_FILE_MAX_AGE - Duration::from_secs(1);
        for (file, modified) in [
            ("kwin-focus-1-100.js", old),
            // A concurrent jump's file: KWin may not have read it yet.
            ("kwin-focus-2-200.js", now),
            // Not a jump's file at all.
            ("tui-77.sock", old),
            ("kwin-focus-3-300.txt", old),
        ] {
            let path = dir.path().join(file);
            std::fs::write(&path, "x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }
        assert_eq!(
            stale_kwin_scripts(dir.path(), now),
            vec![(
                dir.path().join("kwin-focus-1-100.js"),
                "wsx-focus-1-100".to_string()
            )]
        );
        // A missing directory has nothing stale in it.
        assert!(stale_kwin_scripts(&dir.path().join("gone"), now).is_empty());
    }

    #[test]
    fn parses_each_clients_script_id() {
        let dbus_send = "method return time=1.2 sender=:1.5 -> destination=:1.9 serial=4 \
                         reply_serial=2\n   int32 7\n";
        assert_eq!(DbusClient::DbusSend.parse_script_id(dbus_send), Some(7));
        assert_eq!(
            DbusClient::DbusSend.parse_script_id("method return\n   int32 -1\n"),
            None
        );
        assert_eq!(DbusClient::Qdbus("qdbus6").parse_script_id("7\n"), Some(7));
        assert_eq!(DbusClient::Qdbus("qdbus6").parse_script_id("-1\n"), None);
        assert_eq!(DbusClient::Gdbus.parse_script_id("(7,)\n"), Some(7));
        assert_eq!(DbusClient::Gdbus.parse_script_id("(-1,)\n"), None);
        for client in DBUS_CLIENTS {
            assert_eq!(client.parse_script_id(""), None, "{client:?}");
        }
    }

    #[test]
    fn each_client_passes_strings_its_own_way() {
        let args = ["/run/w/kwin-focus-1-2.js", "wsx-focus-1-2"];
        assert_eq!(
            DbusClient::DbusSend.argv("/Scripting", "org.kde.kwin.Scripting.loadScript", &args),
            [
                "--session",
                "--print-reply",
                "--dest=org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                "string:/run/w/kwin-focus-1-2.js",
                "string:wsx-focus-1-2",
            ]
        );
        assert_eq!(
            DbusClient::Qdbus("qdbus6").argv(
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                &args
            ),
            [
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                "/run/w/kwin-focus-1-2.js",
                "wsx-focus-1-2",
            ]
        );
        assert_eq!(
            DbusClient::Gdbus.argv("/Scripting/Script3", "org.kde.kwin.Script.run", &[]),
            [
                "call",
                "--session",
                "--dest",
                "org.kde.KWin",
                "--object-path",
                "/Scripting/Script3",
                "--method",
                "org.kde.kwin.Script.run",
            ]
        );
    }

    #[test]
    fn picks_the_first_dbus_client_on_path() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(find_dbus_client(Some(&path)), None);
        assert_eq!(find_dbus_client(None), None);
        std::fs::write(b.path().join("gdbus"), "").unwrap();
        assert_eq!(find_dbus_client(Some(&path)), Some(DbusClient::Gdbus));
        std::fs::write(a.path().join("qdbus6"), "").unwrap();
        assert_eq!(
            find_dbus_client(Some(&path)),
            Some(DbusClient::Qdbus("qdbus6"))
        );
        std::fs::write(b.path().join("dbus-send"), "").unwrap();
        assert_eq!(find_dbus_client(Some(&path)), Some(DbusClient::DbusSend));
    }

    #[test]
    fn plasma_session_means_kde_without_hyprland() {
        assert!(plasma_session(false, Some("KDE")));
        assert!(plasma_session(false, Some("ubuntu:KDE")));
        assert!(!plasma_session(false, Some("Hyprland")));
        assert!(!plasma_session(false, None));
        // Hyprland exporting KDE for Qt theming keeps the hyprctl path.
        assert!(!plasma_session(true, Some("Hyprland:KDE")));
    }

    #[test]
    fn stat_ppid_parses_despite_parens_in_comm() {
        assert_eq!(ppid_from_stat("123 (weird) name) S 77 123 123 0"), Some(77));
        assert_eq!(ppid_from_stat("123 (simple) S 1 123"), Some(1));
        assert_eq!(ppid_from_stat("garbage"), None);
    }
}
