# HarmonyOS frontend

Native ArkTS / ArkUI client for Sujiu, built as a chat-first app.

HarmonyOS owns ArkUI navigation, text input, window/safe-area behavior,
animations, credential storage and platform file/share capabilities.
Character/world-book/prompt behavior stays in Rust.

Interaction model matches the other frontends: the conversation canvas is the
home surface, history lives in a sheet (permanent sidebar on wide layouts),
model and character selection are lightweight sheets, tool and thinking activity
is collapsed by default, and settings follow the platform settings pattern. See
[`docs/UI_ARCHITECTURE.md`](../../docs/UI_ARCHITECTURE.md).

## Layers

```text
pages/       ArkUI pages (pages/Index is the chat shell and the router host)
components/  reusable views: conversation, composer, history, selector sheets
presentation/ ChatController + Navigator (page state, no I/O)
bridge/      SujiuBridge contract + preview implementation (→ sujiu-ffi)
platform/    clipboard, system appearance, platform info capabilities
app/         composition root; the only place that picks implementations
```

ArkUI views never call a provider, a tool or a platform API directly. They read
`ChatController` state and call its methods.

`SujiuBridge` currently has a preview implementation (`InMemorySujiuBridge`)
because `sujiu-ffi` does not expose the conversation surface yet; switching to
the real bridge is a one-line change in `app/Controller.ets`.

Signing material is machine local and secret and is never committed. Generate
it once per machine (see below).

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

`devecocli check lint` without a path argument reports `Files checked: 0`;
pass the source directory so codelinter actually processes the ArkTS files.

The debug build produces
`entry/build/default/outputs/default/entry-default-signed.hap`.

## Debug signing

`build-profile.json5` is committed without signing material: `signingConfigs`
is empty on purpose, so no key path, alias or password ever reaches Git.
Generate the local debug signature once per machine:

```bash
cd apps/harmony
devecocli signature generate --product default
```

This writes `.p12` / `.csr` / `.cer` / `.p7b` material under `~/.ohos/config/`
and fills `app.signingConfigs` plus the product's `signingConfig` reference
locally. Those local edits stay uncommitted; keep `signingConfigs: []` in the
committed file. Add `--force` only to rebuild an already-existing Sujiu
signature.
