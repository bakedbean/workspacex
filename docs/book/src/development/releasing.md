# Releasing

A release is driven by a git tag. Merging a version bump to main creates the
tag, and everything after that is automatic.

## Cut a release

1. In a pull request, set the new version in `Cargo.toml`, run `cargo check`
   so `Cargo.lock` picks it up, and set the same version in
   `nix/package.nix`.
2. Merge the pull request.

The Tag release workflow (`tag-release.yml`) runs on every push to main that
touches `Cargo.toml`. It reads the version, and if no tag exists for it yet,
tags the merge commit `v<version>` and starts the Release workflow for that
tag. A change to `Cargo.toml` that leaves the version alone finds the tag
already there and does nothing.

The workflow starts Release explicitly instead of relying on the tag push.
A push made with the workflow's `GITHUB_TOKEN` does not start other
workflows, so Release's `push: tags` trigger never fires for it. A manual
run (`workflow_dispatch`) is exempt from that rule.

### Tag by hand

If the Tag release workflow did not run or failed, tag the merge commit
yourself and push the tag. A tag pushed with your own credentials starts
the Release workflow directly:

```bash
git tag v0.2.0 <merge-commit>
git push origin v0.2.0
```

If the tag already exists but no release was published, run the Release
workflow by hand instead (see [Rebuild an existing tag](#rebuild-an-existing-tag)).

The tag must match the version in `Cargo.toml`. The Release workflow compares
them and stops if they disagree, because a mismatch would ship binaries whose
`wsx --version` contradicts the release name.

## What the workflow does

The Release workflow runs four jobs in order: `test`, `build`, `release`, then
`homebrew`.

The `test` job runs the same commands as `ci.yml` on Linux and macOS. It is
here because `ci.yml` runs only on pushes to main and on pull requests, so a
tag starts no tests of its own. Nothing is published if it fails.

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

The packaging step reads each binary with `file` and fails if the
architecture does not match the target it was built for. This means a change
to the architecture a runner label points at stops the release instead of
shipping the wrong artifact.

The `release` job creates the GitHub release and uploads every tarball. The
`homebrew` job then rewrites `Formula/wsx.rb` and opens a pull request with
the new version and checksums.

## Rebuild an existing tag

Run the Release workflow by hand from the Actions tab and give it the tag
name. It publishes the release if there is none yet, or replaces the assets
on the existing one instead of failing.

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
