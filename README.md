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
  sujiu-ffi/     Stable cross-language boundary
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
`sujiu-ffi` as a NAPI module, and `SujiuNativeBridge` implements the shared
bridge contract on top of it. Rebuild the native library after Rust changes:

```bash
export DEVECO_CLI_CLT_PATH="$HOME/.harmony-cli"
scripts/build-harmony-runtime.sh
```

Android and Desktop still drive a preview bridge behind the same contract, so
switching them over is a change in their composition root only.

## License

AGPL-3.0. See [LICENSE](LICENSE).
