#!/usr/bin/env python3
"""Fail when the HarmonyOS bridge stops agreeing with the generated declaration.

CI cannot run the ArkTS compiler: the HarmonyOS Command Line Tools are a 7 GB
install that a GitHub-hosted runner does not have, and a check that only runs on
one contributor's machine is not a check. So this covers the failures that do not
need a compiler, and `scripts/check-harmony-types.sh` covers the rest locally.

Three things are checked:

1. The native module is still declared as a local folder package. This is the
   one that matters most: without it the SDK silently types every import from the
   module as `any`, the compiler stops checking anything, and the whole
   arrangement fails quietly rather than loudly. That is not a hypothetical —
   it is exactly what this repository shipped until the association was added.

2. Every native method the bridge calls exists in the generated declaration.
   A Rust export that is renamed or removed is a broken bridge, and the generated
   file is what the rename has to reach.

3. No hand-written native contract has come back. A second copy of the surface
   cannot drift if it does not exist, so its return is worth a line of CI.

4. No editor sends an empty world-book list. The runtime replaces
   `worldbook_ids` wholesale on save, so an editor that does not carry them back
   is not leaving them alone — it is unbinding them, silently, and the lore
   stops reaching the model. The compiler cannot see this: `[]` is a perfectly
   well-typed `string[]`, so the shape agrees with the declaration and the
   meaning is lost. This one shipped and was caught by reading, not by a tool.

Argument counts, parameter types and return types are deliberately not checked
here. They were wrong twice (`useDataDirectory` declared as a promise, and
`rememberedEndpoint` as returning `undefined`), and both are now compile errors
in the local build, because the association in (1) makes the compiler read the
generated types. Reimplementing a type checker in this script would be a worse
version of the thing that just been removed.

Exit status is 0 when everything agrees, 1 with an explanation otherwise.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MODULE_NAME = "libsujiu_napi.so"
# The directory is named after the module without its extension: ohpm and the
# SDK both look for types/libsujiu_napi/ next to libsujiu_napi.so, so the two
# names are not interchangeable and conflating them was the first bug here.
TYPES_DIR = REPO_ROOT / "apps/harmony/entry/src/main/cpp/types/libsujiu_napi"
MODULE_OH_PACKAGE = TYPES_DIR / "oh-package.json5"
ENTRY_OH_PACKAGE = REPO_ROOT / "apps/harmony/entry/oh-package.json5"
DECLARATION = TYPES_DIR / "index.d.ts"
BRIDGE = REPO_ROOT / "apps/harmony/entry/src/main/ets/bridge/SujiuNativeBridge.ets"

# A local folder package is written in JSON5, which is JSON plus comments and
# unquoted trailing-comma tolerance. Nothing here needs a JSON5 parser: the file
# is four keys written by this repository, and the one thing that must be read
# exactly is a path. Reading it with a JSON5 library would mean depending on one.
_OH_PACKAGE_STRING = re.compile(r'"([^"]+)"\s*:\s*"([^"]*)"')


def fail(problems: list[str]) -> None:
    print("The HarmonyOS bridge does not match the generated N-API declaration.", file=sys.stderr)
    for problem in problems:
        print(f"  - {problem}", file=sys.stderr)
    sys.exit(1)


def check_association() -> None:
    """The native module must be declared as a package that points at the types."""
    problems: list[str] = []

    if not MODULE_OH_PACKAGE.is_file():
        problems.append(
            f"{MODULE_OH_PACKAGE.relative_to(REPO_ROOT)} is missing. It is what tells ohpm "
            f"that {MODULE_NAME} ships a declaration."
        )
    else:
        fields = dict(_OH_PACKAGE_STRING.findall(MODULE_OH_PACKAGE.read_text(encoding="utf-8")))
        if fields.get("name") != MODULE_NAME:
            problems.append(
                f"{MODULE_OH_PACKAGE.relative_to(REPO_ROOT)} declares "
                f"name {fields.get('name')!r}, expected {MODULE_NAME!r}."
            )
        types = fields.get("types", "")
        if not types:
            problems.append(
                f"{MODULE_OH_PACKAGE.relative_to(REPO_ROOT)} has no \"types\" field. Without it "
                f"the module resolves to no declaration at all."
            )
        elif not (TYPES_DIR / types).is_file():
            problems.append(
                f"{MODULE_OH_PACKAGE.relative_to(REPO_ROOT)} points \"types\" at {types!r}, "
                f"which does not exist. A generator that stops writing it would otherwise "
                f"leave a package that installs and types nothing."
            )

    if not ENTRY_OH_PACKAGE.is_file():
        problems.append(f"{ENTRY_OH_PACKAGE.relative_to(REPO_ROOT)} is missing.")
    else:
        dependencies = ENTRY_OH_PACKAGE.read_text(encoding="utf-8")
        if f'"{MODULE_NAME}"' not in dependencies:
            problems.append(
                f"{ENTRY_OH_PACKAGE.relative_to(REPO_ROOT)} does not list {MODULE_NAME!r} under "
                f"dependencies. This is the half that makes the compiler read the declaration; "
                f"without it every native import is typed as `any` and nothing about the "
                f"bridge is checked."
            )

    if problems:
        fail(problems)


def declared_methods() -> set[str]:
    """Method names on the generated bridge class.

    Signatures wrap across lines, so the method name is taken from the line that
    opens the signature rather than by matching a whole declaration. A multi-line
    signature that the single-line pattern would miss is exactly the kind of
    method whose absence would be reported here as a broken bridge call.
    """
    text = DECLARATION.read_text(encoding="utf-8")
    start = text.find("export declare class SujiuRuntimeBridge {")
    if start < 0:
        fail([f"{DECLARATION.relative_to(REPO_ROOT)} declares no SujiuRuntimeBridge class."])
    end = text.find("\n}", start)
    body = text[start:end]
    return set(re.findall(r"^\s{2}(\w+)\(", body, re.MULTILINE))


def called_methods(source: str) -> dict[str, int]:
    """Native methods the bridge calls, with the line each call is on."""
    found: dict[str, int] = {}
    for match in re.finditer(r"this\.runtime\.(\w+)\(", source):
        found[match.group(1)] = source[: match.start()].count("\n") + 1
    return found


def check_calls() -> None:
    if not BRIDGE.is_file():
        fail([f"{BRIDGE.relative_to(REPO_ROOT)} is missing."])

    declared = declared_methods()
    source = BRIDGE.read_text(encoding="utf-8")
    problems = [
        f"{BRIDGE.relative_to(REPO_ROOT)}:{line} calls this.runtime.{name}(...), which the "
        f"generated declaration does not declare."
        for name, line in sorted(called_methods(source).items(), key=lambda item: item[1])
        if name not in declared
    ]
    if problems:
        fail(problems)


def check_no_handwritten_contract() -> None:
    """A second copy of the native surface is the regression this whole path removes."""
    source = BRIDGE.read_text(encoding="utf-8")
    problems: list[str] = []

    for match in re.finditer(r"^\s*interface\s+(Native\w*|SujiuNativeModule)\b", source, re.MULTILINE):
        line = source[: match.start()].count("\n") + 1
        problems.append(
            f"{BRIDGE.relative_to(REPO_ROOT)}:{line} declares {match.group(1)} again. The native "
            f"types are generated; a local interface cannot be checked against Rust."
        )

    # The same drift is reachable without the word Native, by restating the
    # method table under another name.
    if re.search(r"^\s*interface\s+\w+\s*\{[^}]*\b\w+\(\)\s*:\s*(void|string)\s*;", source, re.MULTILINE | re.DOTALL):
        problems.append(
            f"{BRIDGE.relative_to(REPO_ROOT)} declares an interface of no-argument methods, which "
            f"is the shape of a hand-copied runtime. Call the generated class directly."
        )

    if problems:
        fail(problems)


def check_no_dropped_bindings() -> None:
    """An editor must not send an empty world-book list.

    `worldbook_ids: []` is valid at every layer: it is a `string[]` in ArkTS, an
    `Option`/`Vec` on the wire, and a legal value in the Rust request. So the
    compiler and the generated declaration both accept it while it means
    "unbind everything", and the only thing that notices is a user who saved a
    character and later found its lore gone.

    A create has nothing to preserve and is written with an empty id, so the
    check only fires on a draft that is editing something that exists.
    """
    editors = REPO_ROOT / "apps/harmony/entry/src/main/ets/components/LibraryEditors.ets"
    if not editors.is_file():
        fail([f"{editors.relative_to(REPO_ROOT)} is missing."])

    source = editors.read_text(encoding="utf-8")
    problems: list[str] = []
    for match in re.finditer(r"worldbookIds:\s*\[\s*\]", source):
        line = source[: match.start()].count("\n") + 1
        problems.append(
            f"{editors.relative_to(REPO_ROOT)}:{line} saves worldbookIds as an empty list. "
            f"The runtime replaces the field wholesale, so this unbinds every world book the "
            f"resource carried and nothing reports it. Carry the loaded ids, or pick them."
        )
    if problems:
        fail(problems)


def main() -> None:
    check_association()
    check_calls()
    check_no_handwritten_contract()
    check_no_dropped_bindings()
    print(
        "The HarmonyOS bridge reads the generated declaration: the module is wired as a "
        "package, every call it makes exists, it declares no native contract of its own, "
        "and no editor drops a binding on save."
    )


if __name__ == "__main__":
    main()
