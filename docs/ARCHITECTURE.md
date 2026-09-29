# Architecture

Sujiu follows two hard rules:

1. **Rust owns conversation semantics, model I/O, tool execution and deterministic data transforms.**
2. **Each platform owns presentation and interaction.**

## Layers

### 1. Platform frontends

- `apps/desktop`: Qt/QML
- `apps/android`: Kotlin/Android
- `apps/harmony`: ArkTS/ArkUI

Frontends own navigation, input methods, rendering, animations, window state and platform secure-storage integration. They do not rebuild the AI/tool loop independently.

Each frontend is internally split into four layers — view, presentation, application bridge and platform services — so that page layout and OS capabilities stay independent. The interaction spec, the layer rules, the turn state machine and the FFI surface live in [UI_ARCHITECTURE.md](UI_ARCHITECTURE.md).

### 2. sujiu-core

Pure Rust domain data with no HTTP or platform SDK dependency.

It owns:

- character data
- world-book data and deterministic keyword matching
- the conversation transcript (see [Conversation transcript](#conversation-transcript))
- sessions, which store a transcript rather than a derived text history
- provider-neutral prompt plans
- prompt compilation
- provider configuration data

It must not own cursor state, UI animation state, widgets, navigation or platform lifecycle.

### 3. sujiu-ai

Rust conversation runtime.

It owns:

- provider-neutral model messages
- tool definitions, calls and results
- the multi-round agent loop
- deferred tool discovery
- provider HTTP adapters
- built-in retrieval tools such as world-book search

The runtime accepts zero, one or multiple tool calls from a model turn, executes client-owned tools in Rust, appends tool results to the transcript and asks the provider to continue until a final assistant response is produced or the configured round limit is reached.

A turn that runs out of rounds or gets cancelled still returns what it produced. The loop reports how it stopped (`AgentStop::Completed`, `Cancelled`, `MaxRounds`) and hands back a complete `Turn`, because discarding a partial turn would drop steps the model actually saw.

## Conversation transcript

One conversation has three distinct views. They must not be collapsed into each other.

| Layer | What it is | Who may flatten it |
| --- | --- | --- |
| Model transcript | the complete ordered record of user messages, assistant steps, tool calls and tool results | nobody |
| UI projection | what the user finally sees | the UI |
| Provider continuation state | provider-specific metadata: call ids, response ids, reasoning/continuation items | nobody, but it may be dropped when the provider or model changes |

A turn that used tools looks like this:

```text
U1 -> A1(tool_call T1) -> R1 -> A2(tool_call T2) -> R2 -> A3(final)
```

The final answer is the **last** assistant step, not the only assistant content of the turn. The next user turn continues from `U1, A1+T1, R1, A2+T2, R2, A3, U2`. Storing only `U1, A3, U2` is a bug, not a display choice.

A `Turn` therefore holds the user message plus an ordered list of `AssistantStep`s, and each step holds its text, its reasoning (kept separate from visible text), its tool calls, its provider continuation and its token usage.

### Call and result pairing is structural

A tool call and its result are one nested record, so an unpaired call cannot be represented:

```text
ToolCallRecord { id, name, title, arguments, result: ToolResultRecord }
```

A call the runtime never got to run is stored with an explicit state — `Completed`, `Failed`, `Interrupted` or `Cancelled` — and `interrupted_text` turns that into a result the model can read. Interruption never deletes the call.

`validate_pairing` still exists as a last check against a corrupted snapshot: duplicate call ids and calls with neither text nor structured content are rejected.

### History is append-only

Apart from explicit compaction, anything already sent to the model stays exactly as it was and new content is appended at the tail. Rewriting old history costs prompt-cache prefix hits and risks breaking call/result correspondence.

`PromptPlan` reflects this: it is split into a stable `prefix`, a verbatim `history` of model messages, and a `suffix` holding the post-history instruction and the current user input.

### Compaction

Long sessions are not solved by appending forever. `Transcript::compact` moves the oldest turns into a `CompactedTurns` record and replaces them with a summary. Moved turns are kept, not deleted, so exact older detail stays retrievable through the context protocol.

Compaction cuts on turn boundaries. Because a call and its result live in the same turn, a cut cannot split a pair.

### Provider continuation state

`ProviderContinuation` is stored with the transcript step that produced it and is only reused raw when the provider **and** the model both match. On a provider or model switch, the runtime falls back to the normalized model transcript and lets the adapter convert.

Adapters that have no place for continuation state in their wire format (Chat Completions, for example) simply never send it; the field exists so adapters such as OpenAI Responses can use it.

### 4. sujiu-ffi

A deliberately small boundary around Rust. It exposes the conversation runtime to Qt/Kotlin/ArkTS without making any platform reimplement provider or tool semantics: runtime state lives in Rust, and every call returns either domain data or normalized turn events.

`sujiu-napi` re-exports that same surface as a NAPI module. ArkTS imports it directly, so the HarmonyOS bridge calls the runtime without any C glue in ArkTS. The layer rules are unchanged by either crate: neither one carries UI state, navigation or platform concerns.

See [UI_ARCHITECTURE.md §5](UI_ARCHITECTURE.md) for the exported surface, the event names that cross the boundary, and what is still missing.

### 5. Compatibility codecs

SillyTavern/Character Card/World Book support belongs in `sujiu-codec`. External formats are parsed into Sujiu's internal types and exported back out. Unknown external extension fields should be preserved where possible.

The internal model must never become a mirror of SillyTavern's implementation.

## Context and retrieval

Sujiu does **not** treat the model context as a dumping ground.

There are two paths:

```text
deterministic route:
current turn/history
  -> keyword/rule match
  -> small set of world-book entries / tools
  -> prompt/provider

model-driven route:
small initial tool set + sujiu_search_tools
  -> model requests a capability
  -> Rust searches the deferred tool catalog
  -> matching tool schemas become visible next turn
  -> model calls the selected tool
  -> Rust executes it
  -> tool result returns to model
```

This keeps large lore books and large plugin catalogs outside the prompt unless they are relevant.

## Prompt pipeline

```text
App system prompt
  -> character definition
  -> deterministically triggered world-book entries
  -> example dialogue
  -> stable prefix        <- unchanged between turns, cacheable
  -> conversation history <- verbatim transcript, append-only
  -> post-history instruction
  -> current user input
  -> PromptPlan
  -> sujiu-ai
  -> provider
  -> optional tool-call loop (each round appends steps to the transcript)
  -> final assistant response
```

`priority` means client-side retention/ordering priority. It is not presented as a magic model attention weight.

Prompt caching is a design goal, so the prefix and the tool definition order stay stable, and a turn only ever appends. See [Conversation transcript](#conversation-transcript).

## Tool catalog

A tool contains:

- name
- description
- JSON Schema parameters
- local discovery keywords
- whether it must always be available
- Rust execution logic

The local discovery keywords are **not** sent as model weights. They are only a cheap selector for which schemas deserve context space.

If keyword selection is insufficient, `sujiu_search_tools` lets the model search the deferred catalog without exposing every tool up front.

## Provider boundary

`AiProvider` consumes a provider-neutral request and returns:

- assistant text
- zero or more tool calls
- finish reason
- provider continuation state, when the transport has a place for it
- token usage, including cache reads and cache writes

Usage is parsed from the stream (`stream_options.include_usage`), so cache behaviour is measurable rather than guesswork. Adapters must not require continuation state on the request: a wire format without a slot for it simply never receives it.

Current concrete transport:

- OpenAI-compatible Chat Completions

Planned adapters reuse the same runtime:

- OpenAI Responses
- Anthropic Messages
- Gemini GenerateContent / function calling

## Dependency direction

```text
platform UI -> FFI -> sujiu-ai -> sujiu-core
                       |
                       +-> provider adapters

compat codec -> sujiu-core
built-in tools -> sujiu-core data
```

The Rust runtime never imports a platform frontend.
