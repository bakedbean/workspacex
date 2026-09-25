//! How the built binary reports failures: usage errors exit 2, runtime
//! errors exit 1, and both print `error: <msg>` rather than Rust's Debug form.

use std::process::{Command, Output};

fn wsx(args: &[&str]) -> Output {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wsx"));
    cmd.args(args)
        .env("HOME", tmp.path())
        .env("XDG_STATE_HOME", tmp.path().join("state"))
        .env("XDG_CONFIG_HOME", tmp.path().join("config"));
    // Run as a plain shell, not as an agent of the developer's workspace.
    for (k, _) in std::env::vars() {
        if k.starts_with("WSX_") {
            cmd.env_remove(k);
        }
    }
    cmd.output().unwrap()
}

#[test]
fn runtime_errors_print_display_and_exit_1() {
    let out = wsx(&["status", "--workspace", "nope/nope"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(stderr.contains("no repo named 'nope'"), "{stderr}");
    assert!(
        !stderr.contains("UserInput("),
        "Debug form leaked: {stderr}"
    );
}

#[test]
fn usage_errors_exit_2() {
    let out = wsx(&["status", "--bogus"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.starts_with("error: unexpected argument: --bogus"),
        "{stderr}"
    );
}
