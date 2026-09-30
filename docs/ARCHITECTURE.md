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
- the protocol vocabulary: which wire formats exist, in what order they are preferred, and what a failed probe does and does not prove
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
- protocol negotiation against the endpoint
- provider HTTP adapters
- model discovery for the settings screen
- built-in retrieval tools such as world-book search

The runtime accepts zero, one or multiple tool calls from a model turn, executes client-owned tools in Rust, appends tool results to the transcript and asks the provider to continue until a final assistant response is produced or the configured round limit is reached.

A turn that runs out of rounds, gets cancelled, or hits a provider error still returns what it produced. The loop reports how it stopped — `AgentStop::Completed`, `Cancelled`, `MaxRounds(n)` or `Failed(reason)` — and hands back a complete `Turn`. There is deliberately no error-only return path: every exit has a transcript, because discarding a partial turn drops steps the model actually saw.

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

### History is append-only, and a cache break is declared rather than hidden

Apart from explicit compaction, anything already sent to the model stays exactly as it was and new content is appended at the tail. Rewriting old history costs prompt-cache prefix hits and risks breaking call/result correspondence.

`PromptPlan` has three regions:

| Region | Contents | Stable across turns? |
| --- | --- | --- |
| `prefix` | the prompt block: app system prompt, `BeforeCharacter` world book, character system prompt, character definition, `AfterCharacter` world book, example dialogue | yes, as long as the app prompt, the character and the triggered lore stay put |
| `history` | the verbatim transcript | append-only apart from compaction |
| `suffix` | `NearHistory` world book, the post-history instruction, the current user input | no — turn-local by nature |

Each piece of the prompt block sits at **its semantic position**. Moving a `BeforeCharacter` entry after the character definition would have bought a more stable prefix at the cost of changing what the model reads, and a cache miss is a performance cost while prompt semantics are the product. So the block is ordered honestly and the cache consequence is reported instead of designed away.

The suffix is the reason a request is not always append-only. In turn 2 the previous turn's near-history, instruction and user message have moved down behind the answer, so request 1 is not a prefix of request 2. That is normal, and it is not hidden.

`PromptPlan::cache_continuity_with(previous)` judges this on the **real provider-visible message list** — the longest common prefix of the two `model_messages()` sequences, not the internal regions — and reports what happened:

- `Unchanged` — the new request is the old one plus appended content
- `BrokeAtTurnLocalTail` — the block and the transcript are intact; the previous tail was pushed down
- `BrokeAtWorldBook` — a lore entry started or stopped being triggered inside the block
- `BrokeAtPromptBlock` — the app prompt or the character was edited
- `BrokeAtHistory` — the transcript itself was rewritten, which only compaction does

Editing a character *should* invalidate the block; hiding that would only make the next cache miss harder to explain.

### A turn that fails keeps what already finished

A provider failure is not a reason to discard the turn. If round 1 produced a tool call whose result the model has already seen, and round 2 hits an HTTP error or a broken stream, those completed rounds stay in the transcript and the turn is committed as `Failed`. The failure reason is reported separately to the platform as a `turn_failed` event.

Throwing the turn away would leave the next request either missing the call or sending it without a matching result.

This is why the agent loop reports how it stopped rather than returning an error: `AgentStop::Completed`, `Cancelled`, `MaxRounds(n)` and `Failed(reason)` all carry a transcript. There is no error path left that has no return value to commit.

### Compaction

Long sessions are not solved by appending forever. `Transcript::compact` moves the oldest turns into a `CompactedTurns` record and replaces them with a summary. Moved turns are kept, not deleted, so exact older detail stays retrievable through the context protocol.

Compaction cuts on turn boundaries. Because a call and its result live in the same turn, a cut cannot split a pair. Turn ids stay unique across the archive as well as the live turns, since they key the UI bubbles.

A second compaction must **not** silently overwrite the first summary. The summarizer receives every archived turn — previously archived ones included — together with the previous summary text, so the caller can produce a cumulative summary or deliberately re-summarize everything it still holds.

`Transcript::compaction_input(keep_recent)` and the FFI's `SujiuRuntime::compaction_input` expose that material directly, and `CompactionInput::to_prompt_text` renders it for a summarizer prompt. Without it a caller can only see the turns being archived *now*, so it has no way to write a cumulative summary and would quietly drop the older half of the conversation. Documenting "must be cumulative" is not enough when the caller cannot see what it must be cumulative over.

Compaction also has to leave **what the model can see** and **what the world book can trigger on** in agreement. The summary is put into the model's history, so if `Transcript::scan_text()` ignored it, a long conversation would silently change its lore semantics the moment it was compacted: the model would read "Black Tower" in the summary while no world-book entry could match it. `scan_text` therefore includes the compacted summary. The archived raw turns are still not scanned, because they genuinely left the prompt and are reached through the context protocol instead.

### One assistant message, whatever it carries

A step's visible text, its provider sidecar and its tool calls are three properties of **one** assistant message, so the model layer has one shape for it: `ModelMessage::Assistant { content, reasoning, calls }`.

They used to be split, with the sidecar hanging off a tool-calling variant. That is what let a thinking model that reasoned and then answered a question — no tool call at all — lose its reasoning on the next turn, because the variant that carried reasoning was never emitted. Content, sidecar and calls are independent here; the adapter decides which of them the negotiated protocol can actually express.

A step with neither text nor calls is not a message. Emitting an empty assistant turn would put a blank reply on the wire, which is worse than saying nothing.

### Reasoning is replayed when the endpoint needs it

Thinking-mode transports reject a request whose earlier assistant message returns without the reasoning that produced it, so a step's reasoning has to travel with that message on the next request.

The **wire field name** is the adapter's business, taken from the negotiated `EndpointCapabilities` rather than guessed. A blank sidecar is dropped rather than sent as an empty string, and the reasoning is deliberately left out of the flat text view in `PromptPlan::segments()`, because that text is what the world book is scanned against and private reasoning should not match keywords.

An older snapshot may hold the reasoning as a bare string, written before the sidecar carried an identity. That is read and kept, with an unknown identity, which makes it non-replayable on its own: a real identity always names at least a model, so an empty one can never match. Old data therefore loses a capability it cannot honestly claim and keeps the conversation readable, which is the only acceptable order.

### Provider continuation state

`ProviderContinuation` is stored with the transcript step that produced it. Two conditions must both hold before it is reused raw:

- `ContinuationSupport::is_chainable()` — the transport really has a continuation handle
- the `ProviderIdentity` matches: same kind, same provider config id, same base URL, same model

Matching only on kind and model is not enough: two OpenAI-compatible gateways can both serve a model called `gpt-4o-mini` and have entirely unrelated conversation state. On any mismatch the runtime falls back to the normalized model transcript and lets the adapter convert.

Continuation is also **chained within a turn**. Round 2 continues from round 1's handle, not from the handle the session had before the turn began. What a round says about the handle is a `ContinuationUpdate`, not an `Option`, because "I have nothing to chain" and "that handle is dead" are different answers:

- `Unchanged` — no opinion, keep carrying what was in hand
- `Clear` — the handle is no longer usable; the next round runs on the transcript alone
- `Replace(state)` — continue from this handle

`Option<ProviderContinuation>` cannot express `Clear`, so a provider that had just lost the ability to resume would have been handed a dead handle for the rest of the turn.

The event is also **persisted** on the step. Chaining correctly inside one turn is not enough: if only `Replace` is written down, a later `Clear` degrades to "nothing" and the next user turn's `find_map` walks past it and resurrects the dead handle. So a step stores the whole `ContinuationUpdate`, and `Transcript::continuation_for` stops at the first event a provider ever gave, whatever that event was — a replaced handle is superseded and a cleared one is dead.

A stored event that cannot be read is an **error**, not an empty handle. Every field of `ProviderContinuation` is optional, so reading an unrecognised shape with it would silently produce a blank handle — a different answer to "what should happen to the handle" than the one the provider gave. Documents written before continuation events were named are still read, but only when the value is recognisably a bare handle.

### A document the runtime cannot read is protected, not overwritten

The same rule governs the snapshot as a whole. A directory holding a document this build cannot parse enters a **protected** state: `persist()` becomes a no-op, `storage_protection()` reports why, and nothing the runtime does — configuring a provider, creating a session, sending a turn, compacting — writes over it. The user leaves that state deliberately, by discarding the document, or a future migration resolves it.

Skipping the one write that discovered the problem is not enough. The next save destroys it just as thoroughly, one step later, which is the same data loss with a delay.

A directory with no document is not protected: a fresh install still seeds itself, and a readable older document still migrates.

### Assistant reasoning carries its provider

A thinking-mode endpoint may reject a request whose previous assistant message returns without the reasoning that produced it, so the reasoning is stored on the step and replayed. It is a `ReasoningSidecar { content, identity }`, not a bare string.

The identity matters: visible assistant text is portable across providers, provider reasoning is not. Handing one provider's reasoning to another under our own field name would put a foreign protocol's text where the endpoint expects its own. The wire field is only restored when the sidecar's identity matches the endpoint being called, and otherwise the reasoning stays in the transcript for diagnostics while the request replays the normalized transcript alone.

A legacy document that stored the reasoning as a bare string is read and kept, with an unknown identity — which makes it non-replayable on its own, because a real identity always names at least a model, so an empty one can never match. Losing the text entirely would be the worse failure: the session would not even open.

Chat Completions has no continuation concept, so its adapter records the completion id for reference but marks the state `ContinuationSupport::Unsupported`, which makes it permanently ineligible for replay. That is what stops a plain completion label from being mistaken for a chainable handle. The field exists so an adapter that does have one, such as OpenAI Responses with its `previous_response_id`, can use it.

### A per-turn provider is a change, not a restatement

A caller that describes the provider again is making a claim about it. A field the description leaves out is not "unchanged", it is that field reset to a default — so a turn that repeated the saved provider minus one capability silently revoked it.

A per-turn provider is therefore **merged** over the saved one: blank strings inherit, stated fields win, and `extra` merges key by key. The bridge follows the same rule by not describing the provider at all on an ordinary turn — it is already configured in the runtime — and by sending one only for a deliberate single-turn override, in which case it is sent whole.

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
  -> BeforeCharacter world book
  -> character system prompt
  -> character definition
  -> AfterCharacter world book
  -> example dialogue
  -> prompt block          <- cacheable; each piece at its semantic position
  -> conversation history  <- verbatim transcript, append-only
  -> NearHistory world book
  -> post-history instruction
  -> current user input
  -> PromptPlan
  -> sujiu-ai
  -> provider
  -> optional tool-call loop (each round appends steps to the transcript)
  -> final assistant response
```

`priority` means client-side retention/ordering priority. It is not presented as a magic model attention weight.

Prompt caching is a design goal: the stable prefix is unchanged between turns, and an ordinary turn only appends. Where a turn genuinely cannot append — a newly triggered world-book entry, an edited character, a tool-discovery reload that changes which tool schemas are sent — that is a cache break and is declared as one, not papered over. See [Conversation transcript](#conversation-transcript).

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

Deferred discovery has a prompt-cache cost worth stating plainly: when a search makes more schemas visible, the tool definition block changes, and that invalidates the cache from that point on. That is an explicit cache break in exchange for not shipping the whole catalog every turn, which is the trade Sujiu makes on purpose. Tool definition **order** otherwise stays stable, so a catalog that does not change produces an identical block.

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

## Protocol negotiation

What an endpoint can do is the endpoint's business, so the runtime asks instead of guessing. Guessing by vendor or model name does not survive contact with the world: a gateway serves any model under any name, a service renames its models, and a name like `reasoner` or `r1` means nothing to the next one. So no capability is read out of a hostname or a model string anywhere in the runtime.

`ProviderConfig` carries only what a user can honestly supply: a provider id, a base URL, a credential reference and a model. The provider layer then works out the rest.

### Priority

```text
OpenAI Responses
  -> not there
OpenAI Chat Completions
  -> not there
Anthropic Messages
```

Responses comes first because it carries continuation, reasoning and tool state natively, so degrading to Chat Completions and bolting a sidecar on is a downgrade in both fidelity and cache behaviour. Chat Completions is the common denominator: a great many gateways speak it and little else. Anthropic Messages is last because very few services offer it without also offering an OpenAI-compatible interface.

The selected protocol decides the adapter. The adapter decides the wire form. Nothing above the provider layer names a field.

### A failed probe is not a missing protocol

The single most important rule: **only evidence that a path does not exist may move negotiation to the next protocol.** Every other failure is the service declining to answer right now, and recording that as "this endpoint cannot speak" is how a gateway ends up permanently misclassified after one bad minute.

| Response | Verdict | Why |
| --- | --- | --- |
| 2xx | supported | the only positive evidence |
| 404, 405, 501 | unsupported | the path is not there |
| 400/422 naming an unknown endpoint, path or route | unsupported | the service says the route is unknown |
| 400/422 saying anything else | inconclusive | usually a bad request, or a typo'd model |
| 401, 403 | inconclusive, credentials | a key problem, never a protocol fact |
| 429 | inconclusive, rate limited | must never downgrade a protocol |
| 5xx, timeout, connect failure | inconclusive, unavailable | transient by nature |
| unparseable body | inconclusive, malformed | a proxy may have answered instead |

Negotiation also stops on an inconclusive verdict rather than walking on: asking the next protocol after a rate limit is asking a throttled service more questions.

A "not found" phrase only counts when it names an endpoint, a path or a route. A bare "not found" is deliberately excluded, because `model not found` is the most common 400 a perfectly usable endpoint returns, and reading it as a missing route would downgrade a working provider on the first typo.

### The cache is an optimisation, not a fact

Results are cached by provider id plus a normalized base URL (scheme and host lowercased, one trailing slash removed, the path left alone because `/v1` and `/v2` are different APIs). A cached answer is reused, a transient or inconclusive answer is never cached, and reconfiguring a provider forgets the whole cache: a provider id may have been re-pointed at a different endpoint under the same name, and a stale answer about an endpoint we no longer talk to is worse than no answer.

A manual override still exists for a gateway the probe cannot reason about, but it is an advanced compatibility fallback, not the normal path. A platform describes the provider; it does not know that one protocol requires an extension field on tool calls.

### Model discovery is separate

Listing models is a settings-screen convenience and never a step in a conversation. It tries the OpenAI-compatible listing first, then falls back, and every outcome — no listing endpoint, a key without permission, a rate limit, an unreachable host — still leaves manual model entry available. Not having a listing endpoint says nothing about whether the endpoint can chat, and the discovered list never influences which protocol the transcript uses.

## Dependency direction

```text
platform UI -> FFI -> sujiu-ai -> sujiu-core
                       |
                       +-> provider adapters

compat codec -> sujiu-core
built-in tools -> sujiu-core data
```

The Rust runtime never imports a platform frontend.
