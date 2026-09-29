# HarmonyOS frontend

Native ArkTS / ArkUI client for Sujiu, built as a chat-first app.

HarmonyOS owns ArkUI navigation, text input, window/safe-area behavior,
animations, credential storage and platform file/share capabilities.
Character/world-book/prompt behavior stays in Rust.

Interaction model matches the other frontends: the conversation canvas is the
home surface, history is a drawer (pinned sidebar on wide layouts), model and
character selection are lightweight sheets, tool and thinking activity is
collapsed by default, and settings follow the platform settings pattern. See
[`docs/UI_ARCHITECTURE.md`](../../docs/UI_ARCHITECTURE.md).

## Layers

```text
pages/        ArkUI pages; pages/Index hosts the chat shell and ArkUI Navigation
components/   reusable views: conversation, composer, history drawer, selector sheets
presentation/ ChatController: page state, turn state, no I/O and no routing
bridge/       SujiuBridge contract + preview implementation (-> sujiu-ffi)
platform/     clipboard, system appearance, platform info capabilities
app/          composition root; the only place that picks implementations
ui/theme/     AppTheme: the UI-layer color tokens
```

ArkUI views never call a provider, a tool or a platform API directly. They read
`ChatController` state and call its methods.

Routing is the platform's, not the runtime's: `pages/Index` uses ArkUI
`Navigation` with a `NavPathStack`, so the system back key, the back gesture and
page lifecycle are native. There is no custom navigator in the presentation
layer.

History is an ArkUI `SideBarContainer`: an overlay drawer on narrow windows and
a pinned column on expanded ones, decided by one width breakpoint rather than by
two different screens.

`SujiuBridge` currently has a preview implementation (`InMemorySujiuBridge`)
because `sujiu-ffi` does not expose the conversation surface yet; switching to
the real bridge is a one-line change in `app/Controller.ets`.

## Toolchain

HarmonyOS is built with the official DevEco CLI plus the HarmonyOS Command
Line Tools, both of which live outside this repository:

```bash
export PATH="$HOME/.local/npm-global/bin:$PATH"
export DEVECO_CLI_CLT_PATH="$HOME/.harmony-cli"
devecocli --version
```

`DEVECO_CLI_CLT_PATH` must point at a Command Line Tools installation;
otherwise `devecocli` reports that DevEco Studio is unavailable on Linux.

## Lint, build, run

All commands run from `apps/harmony`:

```bash
devecocli check lint entry/src/main/ets
devecocli build --modules entry --build-mode debug
devecocli device list
devecocli run --module entry
```

`devecocli check lint` reports `Files checked: 0` even with a path argument: it
shells out to codelinter without a target. To lint the sources for real, run
codelinter directly:

```bash
"$DEVECO_CLI_CLT_PATH/bin/codelinter" -c code-linter.json5 entry/src/main/ets
```

The debug build produces
`entry/build/default/outputs/default/entry-default-signed.hap`.

## Debug signing

`build-profile.json5` is committed with the `default` signing config and the
product's `signingConfig` reference in place, but with **empty material
placeholders**. The structure is committed; the key path, key alias, passwords
and profile path are machine local and never reach Git.

Because the product always resolves a signing config, `devecocli build` needs
generated material. On a fresh clone, generate the local debug signature first:

```bash
cd apps/harmony
devecocli signature generate --product default
devecocli build --modules entry --build-mode debug
```

`devecocli signature generate` fills the existing `default` entry in place: it
writes `.p12` / `.csr` / `.cer` / `.p7b` material under `~/.ohos/config/` and
replaces the empty placeholders with real paths, alias and encrypted passwords.
Those local edits stay uncommitted; restore the empty placeholders before
committing `build-profile.json5`.

Add `--force` only to rebuild an already-existing Sujiu signature.
