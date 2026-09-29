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
bridge/       SujiuBridge contract + the sujiu-ffi backed implementation
platform/     clipboard, system appearance, platform info capabilities
app/          composition root; the only place that picks implementations
ui/           AppTheme color tokens and Copy, the only place that produces text
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

`SujiuBridge` is implemented by `SujiuNativeBridge`, which loads the sujiu
native module and returns whatever the shared Rust runtime returns. Sessions,
characters, models, context sources, conversation state and the whole turn
lifecycle come from Rust; the bridge only normalizes Rust turn events into the
shared event contract so the UI never sees a provider shape.

Provider credentials are not part of this slice. `SujiuNativeBridge` takes the
api key through `setApiKey` from whoever supplies it (a platform credential
service) and never persists it. Until such a service exists, a turn without a
key ends in a structured `missing_credential` failure that the UI shows as a
normal error, not a crash.

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

## Building the native runtime

The bridge talks to Rust through a NAPI module, so the runtime is cross
compiled for `aarch64-unknown-linux-ohos` and staged into the entry module:

```bash
export DEVECO_CLI_CLT_PATH="$HOME/.harmony-cli"
scripts/build-harmony-runtime.sh              # release, stripped
scripts/build-harmony-runtime.sh --debug      # keeps symbols
```

The script needs the HarmonyOS Command Line Tools, which provide the OHOS NDK
clang that `ring` needs (not just the linker), plus the Rust target:

```bash
rustup target add aarch64-unknown-linux-ohos
```

ArkTS imports the library by name, so a staged copy has to exist before the
entry module compiles. The staged `entry/libs/arm64-v8a/libsujiu_napi.so` is
therefore committed; rerun the script whenever the Rust side changes.

`entry/src/main/cpp/types/libsujiu_napi/index.d.ts` is the hand-maintained
shape of that module. API 26 does not type check napi imports yet and reports
"Currently module for 'libsujiu_napi.so' is not verified", so keep the file in
step with `crates/sujiu-napi/src/bridge.rs` by hand.

## Copy and language

All user-visible text lives in the UI layer's string tables:

```text
entry/src/main/resources/base/element/string.json     English
entry/src/main/resources/zh_CN/element/string.json     Chinese
AppScope/resources/<locale>/element/string.json        app label
```

Presentation exposes states and codes, not sentences, so nothing above the UI
layer needs translating. A tool status is `ToolCallStatus.Running`, a failure is
`errorCode` plus a detail, and `Copy.ets` maps both onto resources. To add a
language, add one table per locale; there is no other switch to flip.

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

The debug build produces a HAP in
`entry/build/default/outputs/default/`. It is `entry-default-signed.hap` when a
local signature exists, and `entry-default-unsigned.hap` otherwise.

## Debug signing

`build-profile.json5` is committed with **no** `signingConfigs` and no product
`signingConfig` reference. A signing config whose material is present but empty
is not a usable default: the HAP signing task fails with `Invalid storeFile
value` instead of falling back to an unsigned HAP.

To get a signed HAP, generate the local debug signature once per machine:

```bash
cd apps/harmony
devecocli signature generate --product default
devecocli build --modules entry --build-mode debug
```

`devecocli signature generate` writes `.p12` / `.csr` / `.cer` / `.p7b` material
under `~/.ohos/config/` and adds the signing config plus the product reference to
`build-profile.json5`. The key paths, key alias and encrypted passwords are
machine local and secret, so those local edits never reach Git: restore the file
before committing it.

Add `--force` only to rebuild an already-existing Sujiu signature.
