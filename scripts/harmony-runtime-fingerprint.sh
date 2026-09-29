#!/usr/bin/env bash
# Print the fingerprint of the Rust sources that determine the staged HarmonyOS
# runtime library.
#
# The library is a build output, so it is committed: ArkTS imports it by name
# and has to find it before the entry module compiles. A committed binary is
# only trustworthy if it cannot silently fall behind its own sources, so the
# build records which sources it was produced from and check-harmony-runtime.sh
# compares that record against the current tree.
#
# Only tracked files are hashed, so an untracked local build artifact cannot
# change the fingerprint. Cargo.lock is deliberately not an input: it is not
# committed, so its absence is the honest answer rather than a fingerprint that
# CI cannot reproduce. The result therefore covers the workspace manifests and
# every Rust source in the crates that end up in the library.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

{
  git ls-files -- \
    Cargo.toml \
    'crates/*/Cargo.toml' \
    'crates/**/*.rs' \
    'scripts/build-harmony-runtime.sh' \
    'scripts/harmony-runtime-fingerprint.sh'
} | LC_ALL=C sort | while read -r path; do
  [ -f "$path" ] || continue
  printf '%s  %s\n' "$(sha256sum "$path" | cut -d' ' -f1)" "$path"
done | sha256sum | cut -d' ' -f1
