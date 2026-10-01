#!/usr/bin/env bash
# Compile the HarmonyOS app and confirm the ArkTS compiler is really reading the
# generated N-API declaration.
#
# This exists because of a failure that is invisible by construction. The SDK
# types an import from a native module as `any` unless the module is declared as
# a local folder package. Without that declaration the app still builds, every
# native call is unchecked, and the bridge can disagree with Rust in any way at
# all — the warning it prints is the only evidence, and it reads like a
# limitation of the SDK rather than a misconfigured project.
#
# So this does not merely build. It fails when the "not verified" warning
# appears, because that warning is the difference between a checked bridge and an
# unchecked one that happens to compile.
#
# CI cannot run this: the Command Line Tools are a 7 GB install. That is why
# `scripts/check-arkts-contract.py` checks the association statically in CI, and
# why this is not optional before changing the bridge.
#
# Usage:
#   scripts/check-harmony-types.sh
#
# Requires DEVECO_CLI_CLT_PATH to point at the HarmonyOS Command Line Tools, and
# devecocli on PATH:
#
#   export DEVECO_CLI_CLT_PATH="$HOME/.harmony-cli"
#   export PATH="$HOME/.local/npm-global/bin:$DEVECO_CLI_CLT_PATH/bin:$PATH"
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_dir="$repo_root/apps/harmony"

if [ -z "${DEVECO_CLI_CLT_PATH:-}" ] || [ ! -d "${DEVECO_CLI_CLT_PATH:-}" ]; then
  echo "DEVECO_CLI_CLT_PATH is not set to an existing directory." >&2
  echo "The HarmonyOS Command Line Tools are required to compile the app, and they" >&2
  echo "are not something this script can install." >&2
  exit 1
fi

if ! command -v devecocli > /dev/null 2>&1; then
  echo "devecocli is not on PATH." >&2
  echo "npm install -g @deveco/deveco-cli@stable, then add its bin directory to PATH." >&2
  exit 1
fi

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

echo "Compiling the HarmonyOS app…"
if ! (cd "$app_dir" && devecocli build --modules entry --build-mode debug) > "$log" 2>&1; then
  cat "$log" >&2
  echo >&2
  echo "The app does not compile." >&2
  exit 1
fi

# A successful build can still mean nothing was checked. The SDK warns once per
# import site of a module it cannot verify, and that warning is the only place
# the difference shows up, so it is matched explicitly rather than trusted to be
# noticed.
if grep -q "is not verified" "$log"; then
  grep -n "is not verified" "$log" >&2
  cat >&2 <<'MESSAGE'

The app compiled, but the ArkTS compiler did not read the generated N-API
declaration: the SDK could not verify the module, so every native call was
typed as `any` and nothing about the bridge was checked.

That means a build here proves less than it appears to. Check the association
that makes the declaration authoritative:

  apps/harmony/entry/src/main/cpp/types/libsujiu_napi/oh-package.json5
    { "name": "libsujiu_napi.so", "types": "./index.d.ts", "version": "" }

  apps/harmony/entry/oh-package.json5
    "dependencies": { "libsujiu_napi.so": "file:./src/main/cpp/types/libsujiu_napi" }

Both are required. Remove oh_modules/ and rebuild if the association was just
added, so ohpm reinstalls the package.
MESSAGE
  exit 1
fi

echo "The app compiles and the ArkTS compiler reads the generated declaration."
