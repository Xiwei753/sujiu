#!/usr/bin/env bash
# Environment for cross compiling the Rust runtime for HarmonyOS.
#
# Sourced, not executed. Building for aarch64-unknown-linux-ohos needs a linker,
# a C compiler for the assembler, an archiver, and one extra link argument, and
# every one of them has to agree. A second script that assembles this list on its
# own is a second place for the list to be wrong in: a missing CC makes ring
# compile its assembly for the host, which fails much later with "is incompatible
# with aarch64linux" and says nothing about the real cause.
#
# Usage:
#   source "$(dirname "$0")/harmony-cross-env.sh"
#
# Environment:
#   DEVECO_CLI_CLT_PATH   HarmonyOS Command Line Tools root. Required.

if [ -z "${DEVECO_CLI_CLT_PATH:-}" ]; then
  echo "DEVECO_CLI_CLT_PATH is not set." >&2
  echo "Point it at the HarmonyOS Command Line Tools, for example:" >&2
  echo "  export DEVECO_CLI_CLT_PATH=\$HOME/.harmony-cli" >&2
  return 1 2>/dev/null || exit 1
fi

_harmony_ndk="$DEVECO_CLI_CLT_PATH/sdk/default/openharmony/native"
if [ ! -x "$_harmony_ndk/llvm/bin/aarch64-unknown-linux-ohos-clang" ]; then
  echo "The OHOS NDK was not found at $_harmony_ndk" >&2
  return 1 2>/dev/null || exit 1
fi

export PATH="$_harmony_ndk/llvm/bin:$PATH"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$_harmony_ndk/llvm/bin/aarch64-unknown-linux-ohos-clang"
export CC_aarch64_unknown_linux_ohos="$_harmony_ndk/llvm/bin/aarch64-unknown-linux-ohos-clang"
export CXX_aarch64_unknown_linux_ohos="$_harmony_ndk/llvm/bin/aarch64-unknown-linux-ohos-clang++"
export AR_aarch64_unknown_linux_ohos="$_harmony_ndk/llvm/bin/llvm-ar"

# The napi symbols are not in libc. The ArkTS engine resolves them from
# libace_napi.z.so, which the NDK declares as the ace_napi system capability.
# Without this dependency the built module only needs libc.so, and the device
# refuses to relocate it at load time with
# "relocating failed: symbol not found ... s=napi_create_object",
# which surfaces as a blank screen rather than as a build error.
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-lace_napi.z"

unset _harmony_ndk