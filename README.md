# Sujiu

Sujiu is a multi-platform AI role-playing client built around a shared Rust conversation/runtime layer and thin native frontends.

## Goals

- Native frontends for desktop, Android and HarmonyOS.
- Rust owns AI conversation, tool calling, character/world-book logic and prompt compilation.
- Provider-specific HTTP formats stay behind Rust adapters instead of leaking into platform UI.
- Do not send every tool or every lore entry on every turn: use deterministic keyword selection first, then model-driven deferred tool discovery when needed.
- SillyTavern ecosystem compatibility is implemented as import/export codecs, not as the internal data model.
- Platform UI state, rendering and animation remain platform-owned.

## Repository layout

```text
apps/
  desktop/       Qt/QML shell
  android/       Kotlin/Android shell
  harmony/       ArkTS/ArkUI shell
crates/
  sujiu-core/    Pure Rust domain model and prompt compiler
  sujiu-ai/      Provider adapters, conversation loop and tool runtime
  sujiu-codec/   SillyTavern / external format compatibility
  sujiu-runtime/     Stable cross-language boundary
  sujiu-napi/    NAPI module so ArkTS can call the boundary directly
docs/
  ARCHITECTURE.md
  TOOLS.md
  UI_ARCHITECTURE.md
```

## Rust AI runtime

`sujiu-ai` already contains:

- a provider-neutral asynchronous conversation loop
- OpenAI-compatible Chat Completions transport
- zero/one/multiple function calls per model turn
- local tool execution and tool-result feedback
- a hard maximum number of tool rounds
- keyword-based initial tool selection
- a deferred `sujiu_search_tools` meta-tool so the model can load a capability without receiving the entire tool catalog
- standard `list_context_sources` / `search_context` / `read_context` tools shared by lore, plot history, durable memory, old chats, persona data and notes

The provider-neutral loop is intentionally separate from provider wire formats so Anthropic, Gemini and newer OpenAI transports can reuse the same tool runtime.

See [docs/TOOLS.md](docs/TOOLS.md) for the stable tool/context protocol.

## Configuring an AI endpoint

Configuration follows the order the work actually happens in:

```text
1. interface address   what to talk to
2. API key             who to talk as
3. probe               which protocols answer, and which models exist
4. model               the user picks one, or types one
```

There is no vendor field and no protocol field. Sujiu asks the endpoint what it
speaks rather than reading a hostname or a model name, so a user supplies a
standard address and a key and nothing else. The model is the only choice a
machine cannot make for them.

A failed probe never blocks configuring the endpoint. A server that will not
list its models can still be used, so manual model entry stays available in every
state, and the settings screen distinguishes a rejected key from a wrong path,
a network failure, a rate limit, a service error and "this build speaks none of
the protocols here" rather than calling all of them a probe failure.

## The diagnostic log

`sujiu-ai` keeps a bounded, persistable log of what it asked, what came back and
what it concluded, split into a `discovery` half (probing and model listing) and
a `chat` half (turns, tool calls, continuations). A user who says "it does not
work" cannot be asked to reproduce anything, so the settings screen can copy the
log for them to paste into a bug report.

API keys and `Authorization` headers never reach it in plaintext. Every message
and field passes through redaction first: a bare key is masked to `sk-****cdef`,
a named field keeps its name and loses its value, and an auth header is dropped
to the end of the line. Response bodies are truncated rather than stored whole.

It is read through the same bridge as everything else, so Android and Desktop
get it by implementing four methods.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the redaction rules and the
stage names.

## Platform frontends

Sujiu is a modern AI chat client first: the conversation canvas is the home
surface, history lives in a drawer/sidebar, model and character selection are
lightweight sheets, tool/thinking activity is collapsed by default, and wide
screens promote the same layout into columns. Every frontend follows the same
four layers — `UI → Presentation → Application Bridge + Platform Services → OS`
— so page layout and platform capabilities stay independent:

```text
apps/desktop/   qml/ + src/{bridge,presentation,platform}
apps/android/   ui/ + {bridge,presentation,platform}
apps/harmony/   pages/ + components/ + {bridge,presentation,platform}
```

See [docs/UI_ARCHITECTURE.md](docs/UI_ARCHITECTURE.md) for the interaction spec,
the layer rules, the turn state machine and the FFI surface.

## Core checks

```bash
cargo fmt --all -- --check
cargo test --workspace
```

The HarmonyOS frontend drives the real runtime today: `sujiu-napi` wraps
`sujiu-runtime` as a NAPI module, and `SujiuNativeBridge` implements the shared
bridge contract on top of it. The native library is a committed build output, so
it has to be rebuilt whenever Rust changes:

```bash
export PATH="$HOME/.local/npm-global/bin:$PATH"     # devecocli is not on PATH
export DEVECO_CLI_CLT_PATH="$HOME/.harmony-cli"
scripts/build-harmony-runtime.sh                    # cross-compile + stage
scripts/check-harmony-runtime.sh                    # fails if the stage is stale
```

The check is not optional busywork: ArkTS imports the library by name, so a Rust
change that was never staged ships an old runtime behind a new bridge, and a new
NAPI method the bridge calls is simply absent at runtime.

To build and install the app:

```bash
export PATH="$HOME/.local/npm-global/bin:$DEVECO_CLI_CLT_PATH/bin:$PATH"
cd apps/harmony
devecocli build --modules entry --build-mode debug
devecocli device list
devecocli run --module entry
```

See [apps/harmony/README.md](apps/harmony/README.md) for the full toolchain
notes, and AGENTS.md §15 for the rules an agent is expected to follow.

Android and Desktop still drive a preview bridge behind the same contract, so
switching them over is a change in their composition root only.

## License

AGPL-3.0. See [LICENSE](LICENSE).
