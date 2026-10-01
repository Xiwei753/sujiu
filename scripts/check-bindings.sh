#!/usr/bin/env bash
# Fail when a committed binding declaration no longer matches the Rust exports,
# or when the HarmonyOS bridge stops reading that declaration.
#
# CI runs this so a Rust DTO change that was never regenerated is a failed job,
# not a device failure. The ArkTS compiler reads the committed declaration, but
# only on a machine that has the HarmonyOS Command Line Tools, which is a 7 GB
# install a CI runner does not have — so the part of the check that needs a
# compiler is `scripts/check-harmony-types.sh`, run locally.
#
# The Kotlin bindings are generated at build time and are deliberately not
# checked here; the compile that consumes them is the check.
#
# Usage:
#   scripts/check-bindings.sh
#
# No environment is required. The declaration is generated from the #[napi]
# attributes while the crate compiles, so regenerating it needs a Rust toolchain
# and Node, not the HarmonyOS NDK. A check that could only run on a machine with
# the Command Line Tools installed would not be a check.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
committed="$repo_root/apps/harmony/entry/src/main/cpp/types/libsujiu_napi/index.d.ts"
scratch="$repo_root/target/napi-bindings-check"
generated="$scratch/index.d.ts"

# Generate into a scratch copy and compare afterwards. Generating over the
# committed file and diffing that against itself would report success for a
# repository it had already rewritten, and would leave the tree modified by a
# check that is supposed to only look at it.
rm -rf "$scratch"
mkdir -p "$scratch"

# `diff` is not trusted for the verdict. The HarmonyOS Command Line Tools ship
# their own under sdk/.../toolchains, and on any machine that has that directory
# on PATH it shadows GNU diff, rejects `-u`, and exits 0 when it could not parse
# its options at all. A comparison that cannot fail is not a comparison, so the
# verdict comes from `cmp`, which that toolchain does not ship.
#
# The probe exists because the alternative is a check that reports success
# because its own tooling is broken, which is the exact failure this script was
# written to catch.
probe_a="$scratch/probe-a.d.ts"
probe_b="$scratch/probe-b.d.ts"
printf 'export declare const x: string;\n' > "$probe_a"
printf 'export declare const y: string;\n' > "$probe_b"
if cmp -s "$probe_a" "$probe_b"; then
  echo "cmp cannot tell two different files apart on this machine." >&2
  echo "The drift check would pass for any binding, so it is not run." >&2
  exit 1
fi
if ! cmp -s "$probe_a" "$probe_a"; then
  echo "cmp reports identical files as different on this machine." >&2
  echo "The drift check would fail for any binding, so it is not run." >&2
  exit 1
fi

if ! SUJIU_ARKTS_OUT="$generated" \
  "$repo_root/scripts/generate-bindings.sh" arkts; then
  echo "The ArkTS declaration could not be generated." >&2
  exit 1
fi

if [ ! -s "$generated" ]; then
  echo "The generated ArkTS declaration is empty." >&2
  echo "An empty declaration describes nothing and would match nothing." >&2
  exit 1
fi

if cmp -s "$generated" "$committed"; then
  echo "The committed ArkTS declaration matches the napi exports."
  # Only now. Comparing the bridge to a declaration that is already known to be
  # stale would report a mismatch against the wrong file, and the stale
  # declaration is the finding — it is fixed by regenerating, not by editing the
  # bridge to match it.
  "$repo_root/scripts/check-arkts-contract.py"
  exit 0
fi

echo "The committed ArkTS declaration no longer matches the napi exports." >&2
echo "Run scripts/generate-bindings.sh and commit the result." >&2
# Best effort only. If this machine's diff is the HarmonyOS one, the verdict above
# is still correct; it just has nothing readable to show.
diff -u "$committed" "$generated" 2>/dev/null | sed -n '1,40p' >&2 || true
exit 1