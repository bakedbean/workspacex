# Releasing

A git tag drives each release. Merging a version bump to `main` creates the tag, and the rest is automatic.

## Cut a release

1. In a pull request, set the new version in `Cargo.toml`, run `cargo check` to update `Cargo.lock`, and set the same version in `nix/package.nix`.
2. Merge the pull request.

The Tag release workflow (`tag-release.yml`) runs on each push to `main` that touches `Cargo.toml`. If no `v<version>` tag exists, it tags the merge commit and starts the Release workflow. A `Cargo.toml` change that keeps the version does nothing.

### Tag by hand

If Tag release didn't run or failed, tag the merge commit and push the tag. A tag you push starts Release directly:

```bash
git tag v0.2.0 <merge-commit>
git push origin v0.2.0
```

The tag must match the version in `Cargo.toml`, or Release stops.

### Rebuild an existing tag

If the tag exists but the release is missing or broken, run the Release workflow from the Actions tab with the tag name. It creates the release or replaces its assets.

## What Release does

| Job | Does |
| --- | --- |
| `test` | Runs the CI checks on Linux and macOS. Nothing publishes if they fail. |
| `build` | Builds four targets and packages each as `wsx-<version>-<target>.tar.gz` with a `.sha256`. |
| `release` | Creates the GitHub release and uploads the tarballs. |
| `homebrew` | Opens a pull request that updates `Formula/wsx.rb`. |

| Runner | Target |
| --- | --- |
| `macos-latest` | `aarch64-apple-darwin` |
| `macos-latest` | `x86_64-apple-darwin` |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` |
| `ubuntu-22.04-arm` | `aarch64-unknown-linux-gnu` |

Linux builds use Ubuntu 22.04 so the binaries run on glibc 2.35 and later. The build fails if a binary's architecture doesn't match its target.

## Update the formula by hand

Download the release tarballs and their `.sha256` files into a directory, then:

```bash
scripts/update-homebrew-formula.sh 0.2.0 ./dist
```

The script changes only the version, URLs and checksums.

## Nix

Bump `version` in `nix/package.nix` in the same commit as `Cargo.toml`. If the `sessionx` revision in `Cargo.toml` changes, refresh its hash and copy the `hash` field into `outputHashes` in `nix/package.nix`:

```bash
nix-prefetch-git --url https://github.com/bakedbean/sessionx --rev <rev>
```

## crates.io

wsx isn't on crates.io. `Cargo.toml` sets `publish = false`, and the `sessionx` git dependency must be published first. After both change, `cargo binstall wsx` works without `--git`.
