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
docs/
  ARCHITECTURE.md
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

## Core checks

```bash
cargo fmt --all -- --check
cargo test --workspace
```

The platform projects are intentionally thin at this stage. Their build-system glue and generated SDK files will be added per-platform after the Rust conversation boundary is stable.

## License

AGPL-3.0. See [LICENSE](LICENSE).
