//! Capture a coding agent's cold boot off a real PTY and replay it through the
//! same `vt100` parser the delivery path reads, so `ready_for_input`'s per-agent
//! signals can be derived from what an agent actually paints instead of guessed.
//!
//! This is the tool that produced `tests/fixtures/agent-boot`. Reach for it when
//! adding a signal for an agent that has none (hermes, as of writing), or when
//! an agent's TUI changes and its signal needs re-deriving.
//!
//! ```text
//! cargo run --example capture_agent_boot -- <out-prefix> <secs> <cwd> <cmd> [args...]
//! ```
//!
//! It writes `<out-prefix>.bin` (the raw byte stream, which is what a fixture is
//! cut from) and `<out-prefix>.timing` (`<ms-since-spawn> <byte-count>` per read,
//! which is what says where a quiet window opens). Then it prints the screen at
//! every point where output went quiet for at least `DELIVERY_QUIET_MS`, since
//! those are exactly the moments an injection can land — a screen in that list
//! with no composer on it is a message the agent will silently eat.

use std::io::Read;
use std::sync::{Arc, Mutex};

/// Mirrors `app::messaging::DELIVERY_QUIET_MS`.
const QUIET_MS: u128 = 400;

/// One `(ms since spawn, bytes)` entry per PTY read, shared with the reader thread.
type Reads = Arc<Mutex<Vec<(u128, Vec<u8>)>>>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!(
            "usage: capture_agent_boot <out-prefix> <secs> <cwd> <cmd> [args...]\n\
             example: capture_agent_boot /tmp/codex 20 . codex"
        );
        std::process::exit(2);
    }
    let prefix = &args[0];
    let secs: u64 = args[1].parse().expect("secs must be a number");
    let cwd = &args[2];
    let argv = &args[3..];

    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut cmd = portable_pty::CommandBuilder::new(&argv[0]);
    for arg in &argv[1..] {
        cmd.arg(arg);
    }
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    cmd.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn");
    drop(pair.slave);

    // (ms since spawn, bytes) per read, which is the granularity the delivery
    // path sees: `activity_ms` is stamped once per read in `spawn_command_session`.
    let chunks: Reads = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&chunks);
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let start = std::time::Instant::now();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => sink
                    .lock()
                    .unwrap()
                    .push((start.elapsed().as_millis(), buf[..n].to_vec())),
            }
        }
    });

    std::thread::sleep(std::time::Duration::from_secs(secs));
    let _ = child.kill();

    let chunks = chunks.lock().unwrap().clone();
    let mut raw = Vec::new();
    let mut timing = String::new();
    for (ms, bytes) in &chunks {
        timing.push_str(&format!("{ms} {}\n", bytes.len()));
        raw.extend_from_slice(bytes);
    }
    std::fs::write(format!("{prefix}.bin"), &raw).expect("write .bin");
    std::fs::write(format!("{prefix}.timing"), &timing).expect("write .timing");
    println!(
        "captured {} reads / {} bytes to {prefix}.bin",
        chunks.len(),
        raw.len()
    );

    let mut parser = vt100::Parser::new(24, 80, 0);
    for (i, (ms, bytes)) in chunks.iter().enumerate() {
        parser.process(bytes);
        // The window between this read and the next is how long the stream is
        // quiet; the last read is quiet forever.
        let quiet = match chunks.get(i + 1) {
            Some((next, _)) => next - ms,
            None => u128::MAX,
        };
        if quiet < QUIET_MS {
            continue;
        }
        let screen = parser.screen();
        println!(
            "\n=== injectable at t={ms}ms (quiet for {}) read {i}/{} \
             alt={} cursor_hidden={} title={:?} ===",
            if quiet == u128::MAX {
                "the rest of the capture".to_string()
            } else {
                format!("{quiet}ms")
            },
            chunks.len(),
            screen.alternate_screen(),
            screen.hide_cursor(),
            screen.title()
        );
        for row in 0..24 {
            let line = screen.contents_between(row, 0, row, 80);
            if !line.trim().is_empty() {
                println!("{row:>2}| {line}");
            }
        }
    }
}
