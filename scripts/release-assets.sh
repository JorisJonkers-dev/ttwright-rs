#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Gather the release assets into one directory:
#   ttwright-x86_64-unknown-linux-gnu        the Linux binary
#   ttwright-x86_64-pc-windows-msvc.exe      the Windows binary
#   ttwright-universal-apple-darwin          the macOS binary (x86_64 and arm64)
#   ttwright-<contract>.v<N>.json            the Contract schemas (copied from contracts/, checked against the binary)
#   SHA256SUMS                               the SHA-256 of every file above
# The binaries come from the build jobs of release.yml (in <binaries-dir>); the attestation is made by the workflow
# over the output directory. Usage: scripts/release-assets.sh <binaries-dir> <out-dir>
set -euo pipefail

binaries="${1:?usage: scripts/release-assets.sh <binaries-dir> <out-dir>}"
out="${2:?usage: scripts/release-assets.sh <binaries-dir> <out-dir>}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ -e "$out" ] && [ -n "$(ls -A "$out")" ]; then
  echo "release-assets: $out exists and is not empty" >&2
  exit 1
fi
mkdir -p "$out"

for name in ttwright-x86_64-unknown-linux-gnu ttwright-x86_64-pc-windows-msvc.exe ttwright-universal-apple-darwin; do
  if [ ! -f "$binaries/$name" ]; then
    echo "release-assets: $binaries/$name is missing" >&2
    exit 1
  fi
  cp "$binaries/$name" "$out/$name"
done
linux="$out/ttwright-x86_64-unknown-linux-gnu"
chmod +x "$linux" "$out/ttwright-universal-apple-darwin"

# The published schemas are the committed ones, and the binary must print exactly the same text.
shopt -s nullglob
schemas=("$root"/contracts/ttwright-*.v*.json)
if [ "${#schemas[@]}" -eq 0 ]; then
  echo "release-assets: no schema under contracts/" >&2
  exit 1
fi
for file in "${schemas[@]}"; do
  name="$(basename "$file")"
  kind="${name#ttwright-}"; kind="${kind%%.*}"
  version="${name#*.v}"; version="${version%.json}"
  if ! "$linux" schema "$kind" --schema-version "$version" | cmp -s - "$file"; then
    echo "release-assets: $file differs from 'ttwright schema $kind --schema-version $version'" >&2
    exit 1
  fi
  cp "$file" "$out/$name"
done

(cd "$out" && sha256sum -- * > ../SHA256SUMS.tmp)
mv "$out/../SHA256SUMS.tmp" "$out/SHA256SUMS"
ls -l "$out"
