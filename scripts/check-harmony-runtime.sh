#!/usr/bin/env bash
# Fail when the committed HarmonyOS runtime library no longer matches the Rust
# sources it was built from.
#
# scripts/build-harmony-runtime.sh records the source fingerprint next to the
# library it stages. This check recomputes the fingerprint and compares it. It
# needs nothing but a shell and sha256sum, so it runs in CI without the
# HarmonyOS NDK, and it catches the failure that matters: a Rust change that
# was never rebuilt, which would otherwise reach a device as an old runtime
# behind a new bridge.
#
# Usage:
#   scripts/check-harmony-runtime.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

libs_dir="apps/harmony/entry/libs/arm64-v8a"
library="$libs_dir/libsujiu_napi.so"
record="$libs_dir/libsujiu_napi.sources.sha256"

if [ ! -f "$library" ]; then
  echo "The staged runtime library is missing: $library" >&2
  echo "Run scripts/build-harmony-runtime.sh to build and stage it." >&2
  exit 1
fi

if [ ! -f "$record" ]; then
  echo "The runtime library has no source record: $record" >&2
  echo "Run scripts/build-harmony-runtime.sh to build and stage it." >&2
  exit 1
fi

expected="$(cut -d' ' -f1 < "$record")"
actual="$("$(dirname "${BASH_SOURCE[0]}")/harmony-runtime-fingerprint.sh")"

if [ "$expected" != "$actual" ]; then
  echo "The staged runtime library is stale." >&2
  echo "  recorded: $expected" >&2
  echo "  current:  $actual" >&2
  echo "A Rust source changed without rebuilding the HarmonyOS library." >&2
  echo "Run scripts/build-harmony-runtime.sh and commit the result." >&2
  exit 1
fi

echo "The staged runtime library matches its sources ($actual)."
