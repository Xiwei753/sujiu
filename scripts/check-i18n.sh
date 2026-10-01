#!/usr/bin/env bash
# Fail when the HarmonyOS translation falls behind the source strings.
#
# The HarmonyOS resource system resolves `base` and then `zh_CN` by locale, and a
# key that exists only in `base` is not an error: it quietly renders in English.
# That is exactly what happened. Issue #5 added sixty-two strings, every one of
# them English-only, and the library, the editors and the binding screen all came
# up in English on a Chinese device without a single build or lint warning.
#
# So the check is not "are the translations good" — it cannot judge that. It is
# "is every key the UI can ask for present in every locale we claim to ship". A
# missing key is a build failure, because it is invisible everywhere else.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
resources="$repo_root/apps/harmony/entry/src/main/resources"
locales=(zh_CN)

status=0

for locale in "${locales[@]}"; do
  dir="$resources/$locale/element"
  if [ ! -d "$dir" ]; then
    echo "No $locale resources at $dir." >&2
    echo "A locale listed in scripts/check-i18n.sh has to exist; an absent one is" >&2
    echo "not a fallback, it is a claim that the app speaks that language." >&2
    status=1
    continue
  fi
  if ! python3 "$repo_root/scripts/check-i18n.py" "$resources/base/element/string.json" \
      "$dir/string.json" "$locale"; then
    status=1
  fi
done

if [ "$status" -ne 0 ]; then
  exit 1
fi

echo "Every string in base has a $locales translation, and no translation is an orphan."