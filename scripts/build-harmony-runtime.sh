#!/usr/bin/env bash
# Build the Sujiu Rust runtime as a HarmonyOS native library and stage it into
# the entry module, so the HAP ships the real runtime instead of a preview one.
#
# The library is a NAPI module, so ArkTS can import it directly and the
# application bridge can talk to sujiu-ffi without any C glue in ArkTS.
#
# ArkTS imports the library by name (`libsujiu_napi.so`), so a staged copy has
# to exist before `devecocli build` can compile the entry module. The staged
# copy is committed; run this script whenever the Rust side changes.
#
# Usage:
#   scripts/build-harmony-runtime.sh [--debug]
#
# Environment:
#   DEVECO_CLI_CLT_PATH   HarmonyOS Command Line Tools root, which contains the
#                         NDK used to cross compile. Required.
set -euo pipefail

profile="release"
if [ "${1:-}" = "--debug" ]; then
  profile="debug"
fi

if [ -z "${DEVECO_CLI_CLT_PATH:-}" ]; then
  echo "DEVECO_CLI_CLT_PATH is not set." >&2
  echo "Point it at the HarmonyOS Command Line Tools, for example:" >&2
  echo "  export DEVECO_CLI_CLT_PATH=\$HOME/harmony/command-line-tools" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ndk="$DEVECO_CLI_CLT_PATH/sdk/default/openharmony/native"

if [ ! -x "$ndk/llvm/bin/aarch64-unknown-linux-ohos-clang" ]; then
  echo "The OHOS NDK was not found at $ndk" >&2
  exit 1
fi

# rustls links C and assembly, so the cross compiler is needed as well as the
# linker. Without these the link step fails with a target mismatch.
export PATH="$ndk/llvm/bin:$PATH"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$ndk/llvm/bin/aarch64-unknown-linux-ohos-clang"
export CC_aarch64_unknown_linux_ohos="$ndk/llvm/bin/aarch64-unknown-linux-ohos-clang"
export CXX_aarch64_unknown_linux_ohos="$ndk/llvm/bin/aarch64-unknown-linux-ohos-clang++"
export AR_aarch64_unknown_linux_ohos="$ndk/llvm/bin/llvm-ar"

echo "Building the Sujiu runtime for aarch64-unknown-linux-ohos ($profile)"
cargo build --manifest-path "$repo_root/Cargo.toml" \
  -p sujiu-napi --target aarch64-unknown-linux-ohos --"$profile"

built="$repo_root/target/aarch64-unknown-linux-ohos/$profile/libsujiu_napi.so"
staged="$repo_root/apps/harmony/entry/libs/arm64-v8a/libsujiu_napi.so"

mkdir -p "$(dirname "$staged")"
cp "$built" "$staged"

# Debug builds keep symbols so a crash is diagnosable. Release builds are
# stripped, because the library is committed and its size matters.
if [ "$profile" = "release" ]; then
  strip="$ndk/llvm/bin/llvm-strip"
  if [ -x "$strip" ]; then
    "$strip" --strip-unneeded "$staged"
  fi
fi

echo "Staged $staged ($(du -h "$staged" | cut -f1))"
