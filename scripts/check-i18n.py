#!/usr/bin/env python3
"""Compare a base HarmonyOS string resource against one locale.

Missing keys are the failure this exists for. HarmonyOS falls back to `base`
without warning, so an untranslated string ships as English inside an otherwise
Chinese screen and nothing in the build mentions it.

Two more things are checked because they are the same failure wearing a
different hat: a translation whose format specifiers do not match the source
(which throws at format time, in one language only), and a key present in the
locale but not in base (which is a leftover from a rename, invisible forever).

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


def load(path: Path) -> dict[str, str]:
    document = json.loads(path.read_text(encoding="utf-8"))
    return {entry["name"]: entry["value"] for entry in document["string"]}


def specifiers(text: str) -> list[str]:
    """Format specifiers in order, with the positional index attached."""
    return [
        f"{m.group(1) or ''}:s" if m.group(0).endswith("s") else f"{m.group(1) or ''}:d"
        for m in _SPECIFIER.finditer(text)
    ]


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