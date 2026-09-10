# Releasing

A release is driven by a git tag. Everything after the tag is automatic.

## Cut a release

1. Set the new version in `Cargo.toml` and run `cargo check` so `Cargo.lock`
   picks it up. Commit both files.
2. Tag the commit and push the tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

The tag must match the version in `Cargo.toml`. The Release workflow compares
them and stops if they disagree, because a mismatch would ship binaries whose
`wsx --version` contradicts the release name.

## What the workflow does

Pushing the tag runs three jobs in order: `build`, then `release`, then
`homebrew`.

The `build` job compiles four targets and packages each one as
`wsx-<version>-<target>.tar.gz` with a matching `.sha256` file:

| Runner | Target |
| --- | --- |
| `macos-latest` | `aarch64-apple-darwin` |
| `macos-latest` | `x86_64-apple-darwin` |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` |
| `ubuntu-22.04-arm` | `aarch64-unknown-linux-gnu` |

Both macOS targets build on the same ARM runner. The Apple SDK is universal,
so the bundled SQLite in `rusqlite` cross-compiles for x86-64 from there. This
avoids the deprecated Intel runners.

Linux binaries are built on 22.04 rather than the current `ubuntu-latest`, so
they link against glibc 2.35 and stay usable on older distributions.

The `release` job creates the GitHub release and uploads every tarball. The
`homebrew` job then rewrites `Formula/wsx.rb` and opens a pull request with
the new version and checksums.

## Rebuild an existing tag

Run the Release workflow by hand from the Actions tab and give it the tag
name. It replaces the assets on the existing release instead of failing.

## Update the formula by hand

The Homebrew job calls a script you can also run locally. Download the release
tarballs and their `.sha256` files into a directory, then:

```bash
scripts/update-homebrew-formula.sh 0.2.0 ./dist
```

The script only touches the version, the urls, and the checksums. Caveats,
dependencies, and the test block survive a bump.

## Nix

`nix/package.nix` pins its own `version` and the hash of the `sessionx` git
dependency. Bump the version there in the same commit as `Cargo.toml`. If the
`sessionx` revision in `Cargo.toml` changes, refresh the hash:

```bash
nix-prefetch-git --url https://github.com/bakedbean/sessionx --rev <rev>
```

Copy the `hash` field into `outputHashes` in `nix/package.nix`.

## crates.io

wsx is not published on crates.io. Two things block it:

- `publish = false` in `Cargo.toml`.
- The `sessionx` git dependency. crates.io rejects a crate that depends on a
  git revision, so `sessionx` has to be published first.

Once both are resolved, `cargo binstall wsx` starts working without the
`--git` flag, and the binstall metadata already in `Cargo.toml` needs no
change.
