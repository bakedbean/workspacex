# Installation

Every method installs the same `wsx` binary. Pick one.

Prerequisites:

- `git` on your PATH.
- The [GitHub CLI](https://cli.github.com) (`gh`), optional, for pull request numbers and review marks on the dashboard.

## Homebrew

macOS and Linux, Intel and ARM:

```bash
brew tap bakedbean/workspacex https://github.com/bakedbean/workspacex
brew install bakedbean/workspacex/wsx
```

Use the fully qualified name. A bare `brew install wsx` stops with a tap trust error.

Upgrade:

```bash
brew update && brew upgrade bakedbean/workspacex/wsx
```

## cargo-binstall

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) downloads the
prebuilt binary, with no Rust build:

```bash
cargo binstall --git https://github.com/bakedbean/workspacex wsx
```

## Nix

Run once without installing:

```bash
nix run github:bakedbean/workspacex
```

Install into your profile (Nix before 2.34 uses `install` instead of `add`):

```bash
nix profile add github:bakedbean/workspacex
```

Add to a NixOS or home-manager flake:

```nix
{
  inputs.wsx.url = "github:bakedbean/workspacex";
  # package: inputs.wsx.packages.${pkgs.stdenv.hostPlatform.system}.default
}
```

## Prebuilt binaries

Download a tarball from the
[releases page](https://github.com/bakedbean/workspacex/releases), verify it
against its `.sha256` file, and install it:

```bash
f=wsx-<version>-<target>.tar.gz
echo "$(cat "$f.sha256")  $f" | shasum -a 256 -c -
tar xzf "$f"
sudo install -m 755 wsx-<version>-<target>/wsx /usr/local/bin/wsx
```

| Platform | Target |
| --- | --- |
| macOS, Apple silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |
| Linux, ARM64 | `aarch64-unknown-linux-gnu` |
| Linux, x86-64 | `x86_64-unknown-linux-gnu` |

The Linux binaries need glibc 2.35 or later.

## From source

Requires Rust 1.88 or later.

```bash
git clone https://github.com/bakedbean/workspacex
cd workspacex
cargo install --path .
```
