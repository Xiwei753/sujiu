#!/usr/bin/env bash
# Generate every cross-language binding from the Rust app-facing API.
#
# The Rust exports are the only interface source. Android's Kotlin and
# HarmonyOS's ArkTS declarations are outputs of this script, so a Rust DTO that
# changes without the platform following is a stale binding rather than a second
# contract someone has to remember to update.
#
# Two targets, deliberately not one command, because they answer different
# questions and fail differently:
#
#   kotlin  generated at Android build time, never committed
#   arkts   committed under apps/harmony, and checked by check-bindings.sh
#
# Usage:
#   scripts/generate-bindings.sh [kotlin|arkts|all]
#
# Environment:
#   DEVECO_CLI_CLT_PATH   Required for `arkts`. HarmonyOS Command Line Tools
#                         root, which contains the NDK used to cross compile.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${1:-all}"

# The napi CLI the HarmonyOS declarations come from.
#
# Pinned, because the generated text is committed and a newer CLI would rewrite
# it on an unrelated Rust change, turning "my DTO changed" into "the toolchain
# moved". Raise it deliberately, with the regenerated file in the same commit.
napi_cli="@napi-rs/cli@3.10.5"

# The directory the ArkTS declaration is checked into. It lives under cpp/types
# because that is where the C++ language server looks for a native module's
# declaration; the ArkTS compiler does not read it, which is exactly why it has
# to be generated rather than trusted.
#
# SUJIU_ARKTS_OUT redirects the output, so check-bindings.sh can generate into a
# scratch directory. Generating over the committed file and comparing afterwards
# would make the check repair what it is checking.
arkts_out="${SUJIU_ARKTS_OUT:-$repo_root/apps/harmony/entry/src/main/cpp/types/libsujiu_napi/index.d.ts}"

# A generated declaration that is empty is worse than a stale one: it looks
# maintained and describes nothing. This has happened for real, because napi-rs
# emits the declaration from a build script hook and a cargo no-op leaves the
# hook uncalled, which produces a zero-byte file with no error anywhere.
require_non_empty() {
  local file="$1"
  if [ ! -s "$file" ]; then
    echo "Generated $(basename "$file") is empty." >&2
    echo "napi-rs only writes declarations when it recompiles the crate, so a" >&2
    echo "cached build yields an empty file rather than an error. Force one:" >&2
    echo "  touch crates/sujiu-napi/src/lib.rs && $0 $target" >&2
    exit 1
  fi
}

generate_kotlin() {
  echo "Generating Kotlin bindings from the UniFFI exports"
  local out="$repo_root/apps/android/app/build/generated/uniffi"
  rm -rf "$out"
  cargo build -q -p sujiu-uniffi --features bindgen
  cargo run -q -p sujiu-uniffi --features bindgen --bin uniffi-bindgen -- \
    generate --library target/debug/libsujiu_uniffi.so \
    --language kotlin --out-dir "$out"
  require_non_empty "$out/uniffi/sujiu/sujiu.kt"
  echo "Wrote $out/uniffi/sujiu/sujiu.kt"
}

generate_arkts() {
  echo "Generating the ArkTS declaration from the napi exports"
  if [ -z "${DEVECO_CLI_CLT_PATH:-}" ]; then
    echo "DEVECO_CLI_CLT_PATH is not set." >&2
    echo "Point it at the HarmonyOS Command Line Tools, for example:" >&2
    echo "  export DEVECO_CLI_CLT_PATH=\$HOME/.harmony-cli" >&2
    exit 1
  fi
  # shellcheck source=scripts/harmony-cross-env.sh
  source "$repo_root/scripts/harmony-cross-env.sh"

  local staging="$repo_root/target/napi-bindings"
  rm -rf "$staging"
  mkdir -p "$staging"

  # Touching the entry point makes cargo recompile, which is what actually makes
  # napi-rs run its declaration hook. Without this the command below succeeds
  # and writes nothing.
  touch "$repo_root/crates/sujiu-napi/src/lib.rs"
  (
    cd "$repo_root/crates/sujiu-napi"
    npx --yes --package "$napi_cli" napi build \
      --target aarch64-unknown-linux-ohos \
      --release \
      --dts index.d.ts \
      --output-dir "$staging" \
      >/dev/null
  )

  mkdir -p "$(dirname "$arkts_out")"
  cp "$staging/index.d.ts" "$arkts_out"
  require_non_empty "$arkts_out"
  echo "Wrote $arkts_out"
}

case "$target" in
  kotlin) generate_kotlin ;;
  arkts) generate_arkts ;;
  all)
    generate_kotlin
    generate_arkts
    ;;
  *)
    echo "Unknown target '$target'. Use kotlin, arkts or all." >&2
    exit 1
    ;;
esac