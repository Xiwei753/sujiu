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

### Current platform priority: HarmonyOS first

For platform work, **HarmonyOS is currently the primary frontend**.

Unless the task is a shared Rust/FFI contract that necessarily affects every platform:

1. implement and verify the HarmonyOS path first;
2. use HarmonyOS to prove the platform/presentation/bridge split and the real Rust FFI integration;
3. only then port the proven behavior to Android and Desktop.

Do not spend time chasing Android/Desktop feature parity while the equivalent HarmonyOS flow is still unbuildable, unverified, or using preview-only data.

Shared abstractions must still stay platform-neutral. "HarmonyOS first" means implementation/verification order, not moving HarmonyOS-specific behavior into Rust.

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

## 15. HarmonyOS development on Linux with DevEco CLI

HarmonyOS is the current platform priority, and Linux agents must use the official **DevEco CLI + HarmonyOS Command Line Tools** workflow instead of treating "DevEco Studio is unavailable on Linux" as a reason to skip compilation.

Huawei currently publishes DevEco CLI as the npm package:

```bash
npm install -g @deveco/deveco-cli@stable
```

The executable is:

```bash
devecocli
```

DevEco CLI is an orchestration layer over the HarmonyOS toolchain. On Linux, install the **HarmonyOS Command Line Tools** bundle as well. The Command Line Tools contain the SDK/build/device tools used by DevEco CLI, including Hvigor, ohpm and hdc.

### 15.1 Linux environment

Keep the Command Line Tools outside the repository, for example:

```text
~/harmony/command-line-tools/
```

Set the toolchain root explicitly. Do not assume Linux can auto-discover a DevEco Studio installation.

```bash
export DEVECO_CLI_CLT_PATH="$HOME/harmony/command-line-tools"
export PATH="$DEVECO_CLI_CLT_PATH/bin:$PATH"
```

If the local Command Line Tools layout exposes Node or hdc outside `bin`, add the corresponding installed directories to `PATH` rather than copying binaries into the repository.

Verify the installation before touching project code:

```bash
devecocli --version
devecocli --help
```

If those fail, fix the local toolchain first. Do not edit project source to compensate for a broken CLI installation.

### 15.2 Project root

All HarmonyOS CLI commands for Sujiu run from:

```bash
cd apps/harmony
```

A valid HarmonyOS project root must contain the normal non-secret project metadata expected by the toolchain, such as the project/module build profiles, package metadata and Hvigor entry files.

**Do not omit required project metadata merely because DevEco Studio generated it.** Generated build outputs and private signing material stay uncommitted; reproducible project configuration belongs in Git.

If `devecocli build` cannot recognize `apps/harmony` as a project because files such as `build-profile.json5`, `oh-package.json5`, `hvigorfile.ts`, module build profiles, or equivalent current-toolchain metadata are missing, fixing that project skeleton is the first HarmonyOS task.

Signing secrets remain local. Never commit private keys, certificates containing secrets, passwords, or machine-specific signing paths.

### 15.3 Lint before build

For ArkTS/TS changes, run lint first when possible:

```bash
cd apps/harmony
devecocli check lint
```

For a focused check:

```bash
devecocli check lint entry/src/main/ets
```

Use `devecocli check lint --help` if the installed CLI version has different options. Do not guess flags from old blog posts.

### 15.4 Build

The normal Sujiu debug-module build is:

```bash
cd apps/harmony
devecocli build --modules entry --build-mode debug
```

A single-entry project may also allow:

```bash
devecocli build
```

For release verification:

```bash
devecocli build --modules entry --build-mode release
```

Use:

```bash
devecocli build --help
```

to confirm the installed version's exact flags.

Do not report HarmonyOS code as verified merely because Rust/Android/Desktop builds pass. A HarmonyOS change is not build-verified until the ArkTS/Hvigor build succeeds.

### 15.5 Device connection and run

List devices first:

```bash
devecocli device list
```

For a single connected device, the normal flow is:

```bash
cd apps/harmony
devecocli run --module entry
```

With multiple devices, specify the target returned by `device list`:

```bash
devecocli run --module entry --device <device-or-serial>
```

If wireless HDC must be connected manually, use the `hdc` shipped with the HarmonyOS toolchain, for example:

```bash
hdc tconn <ip>:<port>
devecocli device list
```

Do not hard-code a user's device address into scripts or source.

### 15.6 Logs and crash diagnosis

After running the app, inspect runtime logs instead of treating a successful install as sufficient verification.

Useful commands include:

```bash
devecocli log --level E
devecocli log --tail 200
devecocli log --follow --bundle-name <bundle-name>
devecocli log --crash --bundle-name <bundle-name>
```

Use `devecocli log --help` for the installed version's supported filters.

For UI work, also use the CLI's device/UI inspection commands when available rather than relying only on static reasoning:

```bash
devecocli device list
devecocli ui --help
```

### 15.7 DevEco CLI agent integration

DevEco CLI can install its HarmonyOS skill/MCP integration into supported coding agents.

For OpenCode, the standard pattern is:

```bash
devecocli init --agent opencode
devecocli init --mcp --agent opencode --project "$(pwd)"
```

Run those from the HarmonyOS project directory when configuring the project-level MCP entry.

Do not run `devecocli create` inside Sujiu: this repository already contains a HarmonyOS project. `create` is only for scaffolding a new project.

### 15.8 HarmonyOS verification order

For HarmonyOS changes, use this order:

```text
read issue / AGENTS.md
  -> inspect apps/harmony project metadata
  -> devecocli check lint
  -> devecocli build --modules entry --build-mode debug
  -> devecocli device list
  -> devecocli run --module entry [--device ...]
  -> exercise the changed UI/flow
  -> inspect devecocli log / crash output
  -> only then report the HarmonyOS path verified
```

If a physical device is unavailable, still perform lint + build and state clearly that install/runtime behavior was not verified.

## 16. Error handling

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

## 17. Tool-loop safety

Always keep a finite maximum number of tool rounds.

Do not create recursive or unbounded tool execution paths.

Multiple tool calls in one assistant turn must be supported where the provider allows them.

Tool results must be returned to the model before the next assistant continuation.

## 18. Testing requirements

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

## 19. CI status matters

The repository has Core CI.

If CI fails:

1. inspect the failing job
2. distinguish formatting failures from compilation/test failures
3. fix the actual cause
4. do not weaken tests merely to make CI green

A failed behavior test is evidence to investigate, not an invitation to change the expected result without justification.

## 20. Keep changes focused

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

## 21. Do not over-engineer early

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

## 22. Backward compatibility and schema evolution

For internal serialized data:

- version schemas where persistence depends on them
- add fields compatibly when possible
- use explicit migrations for incompatible persisted-data changes
- preserve unknown external extension fields when importing third-party formats

For model-visible tools:

- prefer adding optional fields over renaming/removing existing fields
- keep stable tool names when semantics remain the same
- avoid unnecessary schema churn because prompts and provider behavior may depend on it

## 23. Documentation

Update documentation when changing architectural contracts.

At minimum:

- `docs/ARCHITECTURE.md` for ownership/layer changes
- `docs/TOOLS.md` for tool/context protocol changes
- `README.md` for user/developer-visible repository structure changes
- `AGENTS.md` when agent rules themselves change

Do not leave the code and documented architecture contradicting each other.

## 24. Transcript, session, and provider continuation state

Sujiu keeps three different views of one conversation. They must stay separate:

1. **Model transcript** — the complete, ordered record of user messages, assistant steps, tool calls, and tool results. This is the canonical history. It is never flattened.
2. **UI projection** — what the user finally sees. It may fold, hide, or merge tool steps. Folding the UI must never delete transcript steps.
3. **Provider continuation state** — provider-specific metadata such as call ids, response ids, and encrypted reasoning/continuation items. It is stored with the transcript, not treated as disposable UI scratch data.

The correct shape of a turn that used tools is:

```text
U1 -> A1(tool_call T1) -> R1(tool_result) -> A2(tool_call T2) -> R2 -> A3(final)
```

The next turn's model context continues from `U1, A1+T1, R1, A2+T2, R2, A3, U2`. Persisting only `U1, A3, U2` is a bug, not a display choice.

Rules:

- one turn contains many steps; the final answer is only the **last** assistant step, never the only assistant content of the turn
- history is append-only. Apart from explicit compaction or context editing, anything already sent to the model stays byte-for-byte; new content is appended at the tail
- a tool call and its tool result are one atomic pair. No trimming, compaction, or migration may leave a call without a result, a result with no matching call id, reordered calls, or a deleted interrupted call
- interruption and cancellation must be recorded as an explicit interrupted/cancelled tool result, not by discarding the partial transcript
- provider continuation state may be reused raw only when the transport actually supports chaining **and** the full provider identity matches (kind, config id, endpoint, model). Kind plus model name is not enough: two OpenAI-compatible gateways can serve the same model name with unrelated state. Otherwise fall back to the normalized model transcript and let the adapter convert
- continuation advances within a turn. Each round continues from the previous round's handle, not from the one the session had before the turn. A provider says whether it replaced the handle, has no opinion, or dropped it; do not encode those three answers in an `Option`
- a continuation answer is **persisted**, including a clear. Storing only the handle makes a later clear degrade to "nothing", and the next turn's lookup then walks past it and resurrects the dead handle. Store the event and stop at the first one a provider ever gave
- state the protocol produced must survive serialization. An unrecognisable stored event is an error, never an empty handle; a legacy shape is read and degraded to something that cannot claim a capability it cannot prove; and a stored document the runtime cannot read is protected from every later save, not only the one that discovered it
- an assistant message carries visible content, an optional provider-native sidecar and optional tool calls together. Do not hang the sidecar off one variant of the message: a step that reasoned and then answered with no tool call must keep its reasoning
- assistant reasoning travels with the assistant step and is replayed on the next request when the negotiated protocol needs that. The wire field name is the adapter's, never guessed, and never sent to an endpoint that did not ask for it
- reasoning metadata is kept separate from ordinary visible assistant text, and it carries the provider that produced it. Visible text may cross providers; a reasoning sidecar is replayed only to the identity that produced it
- protocol capability comes from **negotiating with the endpoint**, never from a vendor name, a hostname or a model name. `reasoner`, `r1` and `thinking` in a model string prove nothing and go stale. The provider config a platform fills in stays provider id, base URL, credential reference and model
- only evidence that a path does not exist may fall through to the next protocol. 401/403 is a credential problem, 429 is a rate limit, 5xx and timeouts are transient, and a 400 counts only when it names an unknown endpoint, path or route. A bare "not found" is not that, because "model not found" is what a working endpoint says most often
- cache a protocol answer by provider id plus normalized base URL, never by a transient one, and forget it when the provider is reconfigured
- model discovery is a settings-screen convenience. Every failure — no listing endpoint, no permission, rate limited, unreachable — still leaves manual model entry available, and the discovered list never decides the conversation protocol
- a description of the provider is a claim about it, not a partial update. Merge a per-turn provider over the saved one, and let an ordinary turn send none at all
- what the model can read and what the world book can trigger on must agree. When a summary is put into the prompt, the keyword scan sees it too, or compaction silently changes lore semantics
- prompt cache is a design goal: a session-stable prompt block, a verbatim history, and new content appended at the tail. Judge cache continuity on the real provider-visible message list, never on internal region bookkeeping. Turn-local content (near-history entries, post-history instructions, the current input) is expected to vary; when it does, declare it as a cache break instead of calling the request append-only. Do not rewrite or drop already-sent steps just to make the stored history "look clean" or to make the UI show only the final answer
- never buy a cache break with a prompt semantic change. World-book entries stay at the position their meaning asks for; report the cache cost instead of moving the text
- compaction summaries must be cumulative. Expose what a caller has to summarize rather than documenting that it must be, because a caller cannot summarize turns it cannot see
- a turn that stops for any reason still returns its transcript. Completion, cancellation, exhausted rounds and provider errors all commit what already finished, because those steps contain tool calls whose results the next request must repeat. Report the reason separately; do not use an error return to throw a partial turn away

The stored session must be the transcript itself, not a derived text projection. When persisting a turn, keep every assistant step, tool call, and tool result that the model actually saw.

Required automated coverage:

- one tool call, then a second round
- three or more consecutive tool calls, then a second round
- a tool call interrupted, then continued
- a tool error, then continued
- session persisted, runtime closed, reopened, then continued
- after compaction, then continued
- after a provider/model switch, then continued
- UI hides tool details while the model transcript stays complete
- tool call/result ids and ordering match strictly
- for cache-capable providers, consecutive turns do not break the cache prefix

## 25. Completion standard

Before declaring a task complete, verify:

- the change belongs in the correct layer
- provider-specific details did not leak upward
- platform UI logic did not leak into Rust
- new readable data reused the Context protocol where appropriate
- tool schemas are not unnecessarily exposed every turn
- no unrestricted RP memory write path was introduced
- intermediate assistant/tool/result steps stayed in the model transcript
- tool calls and their results stayed atomically paired
- UI folding did not delete transcript steps
- relevant tests were added or updated
- Rust formatting passes
- Rust workspace tests pass
- documentation matches the implementation

If any of these are knowingly unresolved, state that clearly instead of calling the work finished.
