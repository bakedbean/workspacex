#!/usr/bin/env bash
#
# Rewrite Formula/wsx.rb for a new release.
#
# Usage: scripts/update-homebrew-formula.sh <version> <dist-dir>
#
#   <version>   the release version without the leading `v`, e.g. 0.2.0
#   <dist-dir>  a directory that holds the release tarballs and their
#               `.sha256` files, as produced by the Release workflow
#
# The script edits the `version` line, the version inside each `url`, and the
# `sha256` that follows each `url`. It leaves every other line alone, so hand
# written parts of the formula (caveats, dependencies, the test block) survive
# a version bump.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  sed -n '3,12p' "$0" >&2
  exit 2
fi

version="$1"
dist="$2"
formula="$(dirname "$0")/../Formula/wsx.rb"

# Every target the formula names. Keep this in step with the build matrix in
# .github/workflows/release.yml.
targets=(
  aarch64-apple-darwin
  x86_64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-unknown-linux-gnu
)

# Collect the checksums first so a missing artifact fails before the formula
# is touched. A half-rewritten formula is worse than an untouched one.
declare -a pairs=()
for target in "${targets[@]}"; do
  file="$dist/wsx-$version-$target.tar.gz.sha256"
  if [ ! -f "$file" ]; then
    echo "missing checksum: $file" >&2
    exit 1
  fi
  sha="$(tr -d '[:space:]' < "$file")"
  if [ "${#sha}" -ne 64 ]; then
    echo "$file does not hold a bare 64 character sha256" >&2
    exit 1
  fi
  pairs+=("$target=$sha")
done

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

awk -v version="$version" -v spec="${pairs[*]}" '
  BEGIN {
    # Turn "target=sha target=sha ..." into a lookup table.
    n = split(spec, entries, " ")
    for (i = 1; i <= n; i++) {
      split(entries[i], kv, "=")
      sha[kv[1]] = kv[2]
    }
  }

  # The standalone version line.
  /^[[:space:]]*version "/ {
    sub(/version "[^"]*"/, "version \"" version "\"")
    print
    next
  }

  # A url line names its target in the file name. Remember which target this
  # is so the sha256 on a following line can be matched to it, and rewrite
  # both the tag and the file name to the new version.
  /^[[:space:]]*url "/ {
    for (target in sha) {
      if (index($0, "-" target ".tar.gz") > 0) {
        pending = target
        gsub(/\/download\/v[^\/]*\//, "/download/v" version "/")
        # The regex has to be built as a string: awk would read a
        # /literal/ next to `target` as a match result, not a pattern.
        sub("wsx-[^\"]*-" target, "wsx-" version "-" target)
        break
      }
    }
    print
    next
  }

  # The first sha256 after a url line belongs to that url.
  /^[[:space:]]*sha256 "/ && pending != "" {
    sub(/sha256 "[^"]*"/, "sha256 \"" sha[pending] "\"")
    pending = ""
    print
    next
  }

  { print }
' "$formula" > "$tmp"

# Guard against a silent no-op: every placeholder must be gone.
if grep -q 'sha256 "0\{64\}"' "$tmp"; then
  echo "formula still holds a placeholder checksum after the rewrite" >&2
  exit 1
fi

mv "$tmp" "$formula"
trap - EXIT
echo "updated $formula to $version"
