# Installation

Every method gives you the same `wsx` binary. Pick one.

wsx needs `git` on your PATH. Install the
[GitHub CLI](https://cli.github.com) as well if you want pull request numbers
and review marks on the dashboard.

## Homebrew

macOS and Linux, on both Intel and ARM:

```bash
brew tap bakedbean/workspacex https://github.com/bakedbean/workspacex
brew install bakedbean/workspacex/wsx
```

Two details, both deliberate:

- The tap URL is explicit because the formula lives in the main repository
  rather than in a separate `homebrew-` repository.
- The install name is fully qualified. Homebrew 6 refuses to load a formula
  from a tap you have not trusted, and installing by the full name trusts it
  for you. A bare `brew install wsx` stops with a trust error instead.

To upgrade:

```bash
brew update && brew upgrade bakedbean/workspacex/wsx
```

## cargo-binstall

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) downloads the
same prebuilt binary that Homebrew uses, without a Rust build:

```bash
cargo binstall --git https://github.com/bakedbean/workspacex wsx
```

The `--git` flag is necessary because wsx is not published on crates.io yet.

## Nix

The repository is a flake. Run wsx once without installing it:

```bash
nix run github:bakedbean/workspacex
```

Install it into your profile:

```bash
nix profile add github:bakedbean/workspacex
```

Nix older than 2.34 calls that subcommand `install` instead of `add`.

Add it to a NixOS or home-manager configuration:

```nix
{
  inputs.wsx.url = "github:bakedbean/workspacex";

  # Then, in your package list:
  #   inputs.wsx.packages.${pkgs.stdenv.hostPlatform.system}.default
}
```

`nix develop` gives you a shell with Rust, `git`, and `gh` ready for work on
wsx itself. Note that it uses the Rust version in nixpkgs, not the version
pinned in `rust-toolchain.toml`, so run `cargo fmt` through rustup if you
need to match CI exactly.

## Prebuilt binaries

Download a tarball from the
[releases page](https://github.com/bakedbean/workspacex/releases), then:

```bash
tar xzf wsx-<version>-<target>.tar.gz
sudo install -m 755 wsx-<version>-<target>/wsx /usr/local/bin/wsx
```

Each tarball ships with a `.sha256` file. Check it before you install:

```bash
f=wsx-<version>-<target>.tar.gz
echo "$(cat "$f.sha256")  $f" | shasum -a 256 -c -
```

The releases cover these targets:

| Platform | Target |
| --- | --- |
| macOS, Apple silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |
| Linux, ARM64 | `aarch64-unknown-linux-gnu` |
| Linux, x86-64 | `x86_64-unknown-linux-gnu` |

The Linux binaries are built against glibc 2.35.

## From source

You need Rust 1.85 or later, because wsx uses edition 2024.

```bash
git clone https://github.com/bakedbean/workspacex
cd workspacex
cargo build --release
./target/release/wsx
```

`cargo install --path .` puts the binary on your PATH.
