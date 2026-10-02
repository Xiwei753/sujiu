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
# "is every key the UI can ask for present in every locale we claim to ship", plus
# "is the locale value actually different from the English it copies". A missing
# key is a build failure, because it is invisible everywhere else, and so is a
# key whose zh_CN value is byte-identical to the English base value: commit
# a154337c reverse-translated sixty existing Chinese strings back to English and
# every key was present, so nothing failed. scripts/check-i18n.py owns that second
# rule, including the per-key allowlist of terms that are genuinely the same word
# in both languages.
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

echo "Every string in base has a $locales translation, none is still the English base value, and no translation is an orphan."