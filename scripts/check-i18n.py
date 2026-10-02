#!/usr/bin/env python3
"""Compare a base HarmonyOS string resource against one locale.

Missing keys are the failure this existed for. HarmonyOS falls back to `base`
without warning, so an untranslated string ships as English inside an otherwise
Chinese screen and nothing in the build mentions it.

The same failure has a quieter form, and it is the one that actually reached
users. Commit a154337c rewrote `zh_CN` values back into their English source
text. Every key was present, every format specifier matched, and every check
below passed — while the app rendered "Settings", "Writing…" and "The reply could
not be completed." on a Chinese screen. A key that exists is not a translated
key. So a locale value byte-identical to the base value is reported too, unless
the key is listed in `ALLOWED_UNCHANGED` with the reason it is genuinely the
same word in both languages.

That allowlist is deliberately per-key and per-locale. A blanket rule like
"proper nouns are fine" cannot be checked by a script and would swallow every
future whole-English sentence, because a sentence and a product name are the
same kind of string to any rule that is not allowed to name the string.

Two more checks survive because they are the same failure wearing a different
hat: a translation whose format specifiers do not match the source (which
throws at format time, in one language only), and a key present in the locale
but not in base (which is a leftover from a rename, invisible forever).

Usage: check-i18n.py <base.json> <locale.json> <locale-name>
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

# %1$s, %d, %2$d … The positional index is part of the specifier: a translation
# that reordered them would still render, just wrongly, so the indices are kept.
_SPECIFIER = re.compile(r"%(?:(\d+)\$)?[sd]")

# Values that are legitimately identical in both languages, with the reason.
# Everything else must actually be translated. Adding a row here is a claim
# that the English text is already the right text for this locale, so a row
# should be a proper noun, a bare placeholder, or a term the product keeps
# untranslated on purpose — never "this sentence is long, leave it".
ALLOWED_UNCHANGED: dict[str, dict[str, str]] = {
    "zh_CN": {
        "app_name": "product name",
        "entry_ability_desc": "product name",
        "binding_persona": "kept untranslated: Persona is a resource type name",
        "context_kind_persona": "kept untranslated: Persona is a resource type name",
        "provider_models_count_short": "bare format placeholder",
    },
}


def load(path: Path) -> dict[str, str]:
    document = json.loads(path.read_text(encoding="utf-8"))
    return {entry["name"]: entry["value"] for entry in document["string"]}


def specifiers(text: str) -> list[str]:
    """Format specifiers in order, with the positional index attached."""
    return [
        f"{m.group(1) or ''}:s" if m.group(0).endswith("s") else f"{m.group(1) or ''}:d"
        for m in _SPECIFIER.finditer(text)
    ]


def untranslated(locale_name: str, name: str, source: str) -> str | None:
    """The complaint for a locale value that is just the English source again."""
    if locale_name not in ALLOWED_UNCHANGED:
        return (
            f"{name!r} is untranslated: {locale_name} still holds the base value "
            f"{source[:60]!r}. Either translate it, or add it to ALLOWED_UNCHANGED "
            f"if the same text is correct in this language."
        )
    reason = ALLOWED_UNCHANGED[locale_name].get(name)
    if reason is None:
        return (
            f"{name!r} is untranslated: {locale_name} still holds the base value "
            f"{source[:60]!r}, and it is not in ALLOWED_UNCHANGED[{locale_name!r}]."
        )
    return None


def main() -> None:
    base_path, locale_path = Path(sys.argv[1]), Path(sys.argv[2])
    locale_name = sys.argv[3]
    base, locale = load(base_path), load(locale_path)

    problems: list[str] = []

    for name, source in base.items():
        if name not in locale:
            problems.append(f"{name!r} has no {locale_name} translation: {source[:60]!r}")
            continue
        expected, actual = specifiers(source), specifiers(locale[name])
        if expected != actual:
            problems.append(
                f"{name!r} formats differently: base has {expected}, "
                f"{locale_name} has {actual}"
            )
            continue
        if locale[name] == source:
            complaint = untranslated(locale_name, name, source)
            if complaint is not None:
                problems.append(complaint)

    for name in locale:
        if name not in base:
            problems.append(f"{name!r} exists only in {locale_name}; nothing can render it")

    if problems:
        print(
            f"The {locale_name} strings do not cover the source strings.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()