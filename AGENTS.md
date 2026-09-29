# AGENTS.md

This file defines the working rules for coding agents contributing to **Sujiu**.

Sujiu is a multi-platform AI role-playing client. Its architecture is intentionally centered on a shared Rust runtime with thin native frontends. Keep that boundary intact.

## 1. Project priorities

When deciding what to implement first, use this order unless an issue explicitly says otherwise:

1. Rust AI conversation/runtime correctness.
2. Provider adapters and tool-calling behavior.
3. Context retrieval, memory, lore, and session semantics.
4. Compatibility codecs such as SillyTavern / Character Card formats.
5. FFI integration.
6. Platform-native UI and interaction.

Do not build UI features around missing or unstable Rust behavior when the capability clearly belongs in the shared runtime.

## 2. Repository structure

The main architectural boundaries are:

```text
apps/
  desktop/       Qt/QML frontend
  android/       Kotlin/Android frontend
  harmony/       ArkTS/ArkUI frontend

crates/
  sujiu-core/    Provider-neutral domain data and prompt compilation
  sujiu-ai/      AI providers, conversation loop, tools, retrieval
  sujiu-codec/   External format compatibility
  sujiu-ffi/     Cross-language boundary

docs/
  ARCHITECTURE.md
  TOOLS.md
```

Read `docs/ARCHITECTURE.md` and `docs/TOOLS.md` before changing runtime, prompt, memory, retrieval, provider, or tool behavior.

## 3. Rust owns conversation semantics

The Rust side owns:

- AI conversation orchestration.
- Multi-round tool calling.
- Provider-neutral model messages.
- Provider adapters.
- Prompt compilation.
- Character and session data.
- World-book logic.
- Context retrieval.
- Tool registration and execution.
- Long-term memory / plot-history semantics.
- Compatibility-independent domain models.

Platform frontends must **not** reimplement model/tool behavior independently.

Qt, Android, and HarmonyOS should receive stable Rust results through FFI and focus on presentation.

## 4. Platform frontends own UI state

Platform code owns:

- navigation
- widgets/components
- text input and IME behavior
- native lifecycle
- rendering
- animations
- window state
- native file/share pickers
- secure credential storage
- platform-specific permissions

Do not move cursor state, animation state, navigation state, or platform lifecycle into Rust.

The core may expose data and events. It must not become a cross-platform UI state machine.

## 5. Provider boundary

`AgentRuntime` must remain provider-neutral.

Provider-specific wire formats belong behind `AiProvider` implementations.

Examples:

- OpenAI Chat Completions
- OpenAI Responses
- Anthropic Messages
- Gemini GenerateContent / function calling

Do not let provider-specific response blocks, field names, tool-call envelopes, or HTTP details leak into the agent loop or platform UI.

Provider adapters translate their wire format into Sujiu's shared:

- `ModelMessage`
- `ToolDefinition`
- `ToolCall`
- `ToolResult`
- `AssistantTurn`

## 6. Tool protocol

Sujiu's internal tool model is intentionally MCP-shaped.

A tool definition should use the shared contract built around:

- `name`
- `title`
- `description`
- `input_schema`
- optional `output_schema`
- annotations
- local discovery metadata

Tool results use:

- typed content blocks
- optional `structured_content`
- `is_error`

Do not invent separate incompatible result shapes for individual tools.

Provider adapters may flatten or transform this data when a provider supports less than the internal representation.

## 7. Do not expose every tool every turn

A large installed tool/plugin catalog must not be dumped into every model request.

Sujiu uses:

1. cheap local discovery for likely-relevant tools
2. a small initial tool set
3. deferred tool discovery when needed

`sujiu_search_tools` exists to expose additional capabilities only when the model needs them.

When adding a tool:

- provide a precise description
- provide valid JSON Schema
- add useful local discovery metadata
- mark whether it must always be available
- avoid broad keywords that cause unrelated tool matches

Do not solve discovery problems by making all tools always visible.

## 8. Standard context protocol

Readable role-play information should normally use the shared context layer instead of adding a new model-visible tool.

Current standard context kinds include:

- `world_lore`
- `story_event`
- `character_memory`
- `chat_history`
- `persona`
- `note`
- `other`

The shared model is:

- `ContextSource`
- `ContextRecord`
- stable context URI
- scope
- metadata
- optional keywords, tags, timestamp, and priority

The standard model-visible tools are:

- `list_context_sources`
- `search_context`
- `read_context`

Before adding a new read tool, ask:

> Can this data be represented as another ContextSource / ContextRecord behind the existing list/search/read protocol?

If yes, reuse the context protocol.

New model-visible tools should represent genuinely new actions or capabilities, not merely another category of readable information.

## 9. Search first, read second

Large context collections must not be returned wholesale.

Use the normal pattern:

```text
search_context
  -> ranked snippets + stable URIs
  -> read_context for selected records
```

This applies to:

- world books
- old story events
- long-term character memory
- old chat history
- persona information
- notes
- future context sources

Storage and ranking may later use SQLite FTS, BM25, embeddings, vectors, or hybrid retrieval. Those backend changes must not require changing the model-visible tool schema.

## 10. Deterministic context injection is still valid

Not all relevant context needs a tool call.

For cheap and obvious matches, such as a direct world-book keyword hit, deterministic prompt injection is preferred.

Use tools when retrieval is:

- ambiguous
- large
- distant in history
- expensive
- multi-source
- model-dependent

Do not force an extra model round for information the runtime can determine cheaply and reliably.

## 11. Memory writes are separate from memory reads

Do **not** give the role-play model an unrestricted `write_memory` tool by default.

Generated narration is not automatically canonical truth.

Durable memory should go through a separate persistence pipeline such as:

```text
conversation
  -> candidate extraction / scene summary
  -> classify
  -> deduplicate
  -> validate
  -> reconcile contradictions
  -> persist
```

Potential persistent outputs include:

- `story_event`
- `character_memory`

If an explicit user-authorized editing tool is added later, keep it separate from normal RP generation.

## 12. Long conversation handling

Do not solve long sessions by indefinitely appending raw history.

The intended split is:

- recent messages: remain directly in the prompt
- old raw messages: searchable as `chat_history`
- scene / plot summaries: stored as `story_event`
- durable facts and relationships: stored as `character_memory`

Compaction must preserve the ability to retrieve exact older information when needed.

## 13. SillyTavern compatibility

Compatibility belongs in `sujiu-codec`.

External formats should be converted into Sujiu's internal models.

Do not reshape the internal architecture to mirror SillyTavern implementation details.

Important rules:

- support external formats through adapters/codecs
- preserve unknown extension fields where practical
- allow import/export without making external schema the internal source of truth
- PNG/JSON character-card support belongs in the compatibility layer
- world-book imports should map into Sujiu context/domain data

The goal is ecosystem compatibility, not code-architecture compatibility.

## 14. FFI boundary

Keep FFI small and stable.

Prefer versioned, provider-neutral data structures.

Do not expose platform-specific UI state through FFI.

Do not require every frontend to understand provider-specific tool-call JSON.

Rust should normalize the runtime behavior first.

## 15. Error handling

Tool execution errors should normally become structured tool results that the model can observe and react to.

Do not crash the full agent loop for ordinary tool failures.

Reserve Rust errors for conditions where continuing the runtime is not valid.

Examples:

- malformed provider response
- transport failure
- invalid runtime configuration
- maximum tool rounds exceeded

Examples that should usually become tool results:

- unknown tool
- invalid tool arguments
- missing context record
- backend-specific lookup failure that the model can recover from

## 16. Tool-loop safety

Always keep a finite maximum number of tool rounds.

Do not create recursive or unbounded tool execution paths.

Multiple tool calls in one assistant turn must be supported where the provider allows them.

Tool results must be returned to the model before the next assistant continuation.

## 17. Testing requirements

For Rust changes, the required baseline is:

```bash
cargo fmt --all -- --check
cargo test --workspace
```

Do not claim Rust work is complete if these checks are failing.

When changing:

- tool discovery: add tests for false positives and correct exposure
- agent loop: test multi-round tool calls
- context search: test filtering, ranking, search/read separation
- provider adapters: test request/response translation
- memory behavior: test persistence boundaries and contradiction handling
- codecs: test round-tripping and unknown-field preservation

Prefer tests that validate behavior rather than implementation details.

## 18. CI status matters

The repository has Core CI.

If CI fails:

1. inspect the failing job
2. distinguish formatting failures from compilation/test failures
3. fix the actual cause
4. do not weaken tests merely to make CI green

A failed behavior test is evidence to investigate, not an invitation to change the expected result without justification.

## 19. Keep changes focused

Avoid large unrelated refactors while implementing one feature.

Do not modify the same cluster of files through multiple concurrent issues/branches when the work can be done sequentially.

Preferred workflow:

```text
one issue / one focused change
  -> implement
  -> test
  -> review
  -> merge/finish
  -> next issue
```

This is especially important for:

- prompt runtime
- editor/runtime state
- shared Rust APIs
- FFI
- platform integration

Avoid unnecessary Git conflict surfaces.

## 20. Do not over-engineer early

Prefer a small correct abstraction over a large speculative framework.

Good examples:

- one provider-neutral `AiProvider` trait
- one standard context retrieval protocol
- one tool registry
- one stable FFI boundary

Avoid:

- provider-specific logic in UI
- one tool per database/table/content type
- duplicate implementations on every platform
- speculative plugin systems before the core behavior exists
- unnecessary abstraction layers with no current caller

## 21. Backward compatibility and schema evolution

For internal serialized data:

- version schemas where persistence depends on them
- add fields compatibly when possible
- use explicit migrations for incompatible persisted-data changes
- preserve unknown external extension fields when importing third-party formats

For model-visible tools:

- prefer adding optional fields over renaming/removing existing fields
- keep stable tool names when semantics remain the same
- avoid unnecessary schema churn because prompts and provider behavior may depend on it

## 22. Documentation

Update documentation when changing architectural contracts.

At minimum:

- `docs/ARCHITECTURE.md` for ownership/layer changes
- `docs/TOOLS.md` for tool/context protocol changes
- `README.md` for user/developer-visible repository structure changes
- `AGENTS.md` when agent rules themselves change

Do not leave the code and documented architecture contradicting each other.

## 23. Completion standard

Before declaring a task complete, verify:

- the change belongs in the correct layer
- provider-specific details did not leak upward
- platform UI logic did not leak into Rust
- new readable data reused the Context protocol where appropriate
- tool schemas are not unnecessarily exposed every turn
- no unrestricted RP memory write path was introduced
- relevant tests were added or updated
- Rust formatting passes
- Rust workspace tests pass
- documentation matches the implementation

If any of these are knowingly unresolved, state that clearly instead of calling the work finished.
