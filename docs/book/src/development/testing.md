# Testing

Run the same checks CI runs before you open a pull request:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc --all-features
```

The suite runs without any coding agent installed. Tests point `WSX_CLAUDE_BIN` and the other `WSX_*_BIN` variables at stand-ins such as `cat`.

## Toolchain

`rust-toolchain.toml` pins the Rust version, so rustup installs the same `rustfmt` and `clippy` CI uses. Formatting can differ between Rust releases, so use the pinned toolchain for `cargo fmt`.

The Nix dev shell (`nix develop`) provides `cargo`, `clippy`, `rustfmt`, `git` and `gh` from nixpkgs and ignores `rust-toolchain.toml`. If `cargo fmt --check` passes there but fails in CI, run it with rustup's toolchain instead.

## End-to-end checks

`harness/` runs a locally built wsx against a throwaway sandbox in `/tmp/wsx-test`:

```bash
bash harness/smoke.sh          # prints SMOKE PASS
harness/harness.sh up          # build wsx and provision the sandbox
harness/harness.sh down        # remove it
```

See `harness/README.md` for snapshots and state queries.
