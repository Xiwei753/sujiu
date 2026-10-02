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

- the conversation, which is the root entity (see [Domain entities](#domain-entities))
- character, persona, world-book and prompt-profile data as independent entities
- deterministic world-book keyword matching
- the conversation transcript (see [Conversation transcript](#conversation-transcript))
- a transcript stored on the conversation itself, rather than a derived text history
- the protocol vocabulary: which wire formats exist, in what order they are preferred, and what a failed probe does and does not prove
- provider-neutral prompt plans
- prompt compilation
- provider configuration data

It must not own cursor state, UI animation state, widgets, navigation or platform lifecycle.

## Domain entities

The conversation is the root. A character is a participant in a conversation, not
the thing a conversation is filed under, and a conversation can have no
participant, one, or many.

```text
Conversation
  id
  participants: [Participant]   0..N
       Participant { character_id, role, display_name }
       role is Character (default) or Narrator (a game master, a table)
  persona_id:      Option<id>    -> Persona
  worldbook_ids:   [id]          -> WorldBook
  prompt_profile_id: Option<id>  -> PromptProfile
  transcript:      Transcript
  metadata
```

| Entity | What it is | What it may not do |
| --- | --- | --- |
| `Character` | name, description, personality, scenario, first message, alternate greetings, example dialogue, a character-level system prompt override, extensions | own a world book. It may name default world books by id, and that is a reference, not ownership |
| `Persona` | who the user is in this conversation: name, description, user prompt, default world-book ids | live inside a `Character` |
| `WorldBook` | named entries with keys, priority and position, plus the deterministic matcher | have a lifetime that depends on a character being deleted |
| `PromptProfile` | app/system prompt, user-side persona prompt, post-history instructions, format rules, extra fixed segments | stay scattered across a character card, a provider config and UI state |

A conversation binds a persona, any number of world books and one prompt profile.
World books additionally bind globally, so a world book can be reachable from
several conversations at once and survive any of them. Binding order is
deliberate and stable: the conversation's own bindings first, then the defaults
named by its participant characters, then the persona's defaults, then the
global ones — deduplicated by id, so a book bound twice is injected once.

Because the participant list is a list, a group chat is `participants = [a, b, c]`
and a tabletop is a Narrator plus several characters. Neither is a "main
character with extra text", so neither needs the storage model changed again when
those UIs are built.

### Managing a resource is not binding one

`Character`, `Persona`, `WorldBook` and `PromptProfile` are edited as themselves.
A conversation only stores ids, and rebinding a conversation never edits the
resource it points at.

That separation exists because the alternative is quietly destructive. If a
conversation held its own copy of a world book, editing the conversation would
fork the resource, the next conversation using the original would keep showing
the old text, and neither copy would look wrong. So:

- editing a resource edits that resource, for every conversation that references
  it;
- changing a conversation's bindings changes only the id list;
- deleting a resource clears every reference to it — participants,
  `persona_id`, `worldbook_ids`, `prompt_profile_id`, the character- and
  persona-level default lists, and the global world-book list. A dangling id is
  not a soft state: it renders as a binding the runtime cannot honour.

Editing one field of a resource also edits only that field. Replacing a
`PromptProfile` wholesale would let a form that knows about the four prompt
strings silently drop the fixed segments it never rendered.

Because personas and world books are also projected into the context store, a
mutation to one of them re-derives the projection. Otherwise `search_context`
keeps answering with a world book entry that was deleted a moment ago, which is
a retrieval result that is confidently wrong rather than merely stale.

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
- the diagnostic log
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

### A step records who spoke

A conversation can hold several characters, so "assistant" is not an identity: two answers from two participants stored as two anonymous assistant messages cannot be told apart afterwards, and a later group chat would need another transcript migration to fix it. Each step therefore carries the `character_id` of the participant it spoke for.

The speaker is set when the step is **born**, from `AgentConfig.speaker`, not stamped afterwards. That is the prompt-cache rule applied to an identity: rewriting a step that was already sent changes the wire shape of history that a provider has read.

`None` is a real answer rather than a missing field. Choosing between participants is a speaking-order policy, and this runtime has not got one, so the only speaker it ever picks is a conversation's single participant. With zero or several, it stores `None` instead of a guess.

A protocol's ability to carry the id is the adapter's call. Chat Completions has a `name` field and gets the speaker there; Responses has no such field, and the adapter sends nothing rather than inventing one or putting the id into text the model did not write. The transcript keeps the speaker either way.

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

Long conversations are not solved by appending forever. `Transcript::compact` moves the oldest turns into a `CompactedTurns` record and replaces them with a summary. Moved turns are kept, not deleted, so exact older detail stays retrievable through the context protocol.

Compaction cuts on turn boundaries. Because a call and its result live in the same turn, a cut cannot split a pair. Turn ids stay unique across the archive as well as the live turns, since they key the UI bubbles.

A second compaction must **not** silently overwrite the first summary. A cumulative summary is built from exactly two things: the previous summary, and the turns this compaction is archiving **now**.

The already-archived raw turns do **not** go back into the summarizer. `previous_summary` already stands for them, so re-sending them is not "extra safety" — it is re-paying for the whole conversation on every compaction, and the summarizer prompt would then grow with the session until the summarizer itself hits the context limit, which is exactly when automatic compaction has to work. The raw turns stay in `CompactedTurns.turns` in full, for the UI and for exact retrieval through the context protocol. They are retained, not re-billed.

`Transcript::compaction_input(keep_recent)` and the FFI's `SujiuRuntime::compaction_input` expose that material directly, and `CompactionInput::to_prompt_text` renders it for a summarizer prompt. It renders what the model was actually shown — the turn's provider-neutral messages, including tool arguments and tool results — because a turn that looked something up and then said "found it" would otherwise archive the only copy of what it found. Reasoning is deliberately left out: it is provider state, possibly another provider's, and folding it into a summary would write private deliberation into a record later turns read as what was said.

Without the previous summary in the input, a caller can only summarize the slice in front of it and would quietly drop the older half of the conversation. That is why the caller is handed what it must be cumulative *over* rather than merely told to be cumulative.

Compaction also has to leave **what the model can see** and **what the world book can trigger on** in agreement. The summary is put into the model's history, so if `Transcript::scan_text()` ignored it, a long conversation would silently change its lore semantics the moment it was compacted: the model would read "Black Tower" in the summary while no world-book entry could match it. `scan_text` therefore includes the compacted summary. The archived raw turns are still not scanned, because they genuinely left the prompt and are reached through the context protocol instead.

Compaction moves turns out of the **prompt** and out of nothing else. `ui_messages()` projects the archived turns too, oldest first, because a kernel that compacts on its own would otherwise shorten a user's scrollback every time the budget ran out — the conversation would visibly lose its own history in the middle of a session. The projection is what a screen reads; `compacted` is what the prompt stopped reading.

#### Who decides

The kernel decides, and a platform may only change the numbers. `CompactionPolicy { max_input_chars, keep_recent_turns }` is the whole of that surface, it defaults to something bounded, and `SujiuRuntime::set_compaction_policy` changes the size rather than moving the decision. A platform that starts compaction on its own schedule, with its own threshold and its own notion of which messages matter, has reintroduced exactly the per-platform conversation semantics this architecture exists to keep in one place.

The budget is in **characters**, because the kernel cannot know the endpoint's tokenizer. It is a trigger, not an accounting: the provider's own reported input tokens are the real number, and they are recorded separately as the turn's usage. A char budget that guessed well for one model and badly for another would be worse than one that is honestly an estimate.

The decision is made **before** the turn, on a turn boundary, and the summarizer is a real request: the same provider, no tools, no continuation handle. The last part matters. Continuing a summarization from the transcript it is being asked to fold up would splice two different conversations together, and the summarizer has no tool to wander off with because it has no tools at all.

A summary that fails is not applied. The turn carries on with the transcript it has, and the failure is recorded as a `compaction_failed` diagnostic. Archiving turns behind a summary that is not there would move them out of reach while still charging for the request that never produced anything.

### One assistant message, whatever it carries

A step's visible text, its speaker, its provider sidecar and its tool calls are four properties of **one** assistant message, so the model layer has one shape for it: `ModelMessage::Assistant { content, speaker, reasoning, calls }`.

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

Continuation is also **chained within a turn**. Round 2 continues from round 1's handle, not from the handle the conversation had before the turn began. What a round says about the handle is a `ContinuationUpdate`, not an `Option`, because "I have nothing to chain" and "that handle is dead" are different answers:

- `Unchanged` — no opinion, keep carrying what was in hand
- `Clear` — the handle is no longer usable; the next round runs on the transcript alone
- `Replace(state)` — continue from this handle

`Option<ProviderContinuation>` cannot express `Clear`, so a provider that had just lost the ability to resume would have been handed a dead handle for the rest of the turn.

The event is also **persisted** on the step. Chaining correctly inside one turn is not enough: if only `Replace` is written down, a later `Clear` degrades to "nothing" and the next user turn's `find_map` walks past it and resurrects the dead handle. So a step stores the whole `ContinuationUpdate`, and `Transcript::continuation_for` stops at the first event a provider ever gave, whatever that event was — a replaced handle is superseded and a cleared one is dead.

A stored event that cannot be read is an **error**, not an empty handle. Every field of `ProviderContinuation` is optional, so reading an unrecognised shape with it would silently produce a blank handle — a different answer to "what should happen to the handle" than the one the provider gave. Documents written before continuation events were named are still read, but only when the value is recognisably a bare handle.

### A document the runtime cannot read is protected, not overwritten

The same rule governs the snapshot as a whole. A directory holding a document this build cannot parse enters a **protected** state: `persist()` becomes a no-op, `storage_protection()` reports why, and nothing the runtime does — configuring a provider, creating a conversation, sending a turn, compacting — writes over it. The user leaves that state deliberately, by discarding the document, or a future migration resolves it.

Skipping the one write that discovered the problem is not enough. The next save destroys it just as thoroughly, one step later, which is the same data loss with a delay.

A directory with no document is not protected: a fresh install still seeds itself, and a readable older document still migrates.

### Assistant reasoning carries its provider

A thinking-mode endpoint may reject a request whose previous assistant message returns without the reasoning that produced it, so the reasoning is stored on the step and replayed. It is a `ReasoningSidecar { content, identity }`, not a bare string.

The identity matters: visible assistant text is portable across providers, provider reasoning is not. Handing one provider's reasoning to another under our own field name would put a foreign protocol's text where the endpoint expects its own. The wire field is only restored when the sidecar's identity matches the endpoint being called, and otherwise the reasoning stays in the transcript for diagnostics while the request replays the normalized transcript alone.

A matching identity is necessary but not sufficient, because each protocol carries reasoning differently and none of them carries it the same way twice:

- **Responses** — reasoning is a provider-native opaque item living inside a server-side response chain, not a request field. There is nothing an adapter could fill, and a reasoning *summary* is not the reasoning. It therefore travels only with that chain, which is why it replays only inside one user turn's tool loop.
- **OpenAI-compatible Chat Completions** — the endpoint may accept a reasoning field on an assistant message (`reasoning_content` today). It is sent only when the negotiated capabilities say this endpoint asks for it, and only for a sidecar this endpoint produced. Whether a given gateway accepts that field is exactly the kind of fact that is negotiated rather than assumed; `forced_reasoning_replay` is the advanced override for a gateway the probe cannot reason about, and it is deliberately not the normal path.
- **An endpoint that carries no reasoning field at all** — the sidecar is dropped from the request and kept in the transcript.

Either way the visible text of the same step always travels: visible text is portable, provider reasoning is not, and losing the reasoning with the turn is never acceptable.

A legacy document that stored the reasoning as a bare string is read and kept, with an unknown identity — which makes it non-replayable on its own, because a real identity always names at least a model, so an empty one can never match. Losing the text entirely would be the worse failure: the conversation would not even open.

Chat Completions has no continuation concept, so its adapter records the completion id for reference but marks the state `ContinuationSupport::Unsupported`, which makes it permanently ineligible for replay. That is what stops a plain completion label from being mistaken for a chainable handle. The field exists so an adapter that does have one, such as OpenAI Responses with its `previous_response_id`, can use it.

### A per-turn provider is a change, not a restatement

A caller that describes the provider again is making a claim about it. A field the description leaves out is not "unchanged", it is that field reset to a default — so a turn that repeated the saved provider minus one capability silently revoked it.

A per-turn provider is therefore **merged** over the saved one: blank strings inherit, stated fields win, and `extra` merges key by key. The bridge follows the same rule by not describing the provider at all on an ordinary turn — it is already configured in the runtime — and by sending one only for a deliberate single-turn override, in which case it is sent whole.

### 4. sujiu-runtime

A deliberately small boundary around Rust. It exposes the conversation runtime to Qt/Kotlin/ArkTS without making any platform reimplement provider or tool semantics: runtime state lives in Rust, and every call returns either domain data or normalized turn events.

It is also the **single source** of every cross-platform interface. Two binding crates turn it into platform code, and neither one is a second answer to what a platform may ask:

```text
sujiu-uniffi   Android      UniFFI  -> generated Kotlin
sujiu-napi     HarmonyOS    napi-rs -> generated N-API + index.d.ts
```

`sujiu-uniffi` holds no logic beyond translation: records the platforms receive are UniFFI records with explicit `From` conversions, and a platform cannot reach a runtime type that has no conversion. `sujiu-napi` does the same through `#[napi(object)]` DTOs. Neither crate carries UI state, navigation or platform concerns, and neither carries the other's generator — a crate that could only be used by one of the two platforms would make a plain Rust consumer of the runtime carry a binding toolchain it never asked for.

The consequence that matters is negative: a field a platform needs does not get added to Kotlin or ArkTS. It gets added here and exported. That is why `sujiu-uniffi` had to grow a `list_models()` during this work — the Android bridge wanted the model list, and the honest fix was an export, not a second hand-written shape.

Two binding crates means two generators, and that is deliberate rather than a failure to converge. There is no mature UniFFI target for ArkTS, so napi-rs is the correct route for HarmonyOS. Asking one generator to serve both would mean a language binding nobody needs in order to avoid writing the Android one properly.

The generated artifacts are build outputs with different owners, because the two toolchains have different constraints:

- **Kotlin is generated at build time** by a Gradle task and is not committed. Nothing can drift from a file the build overwrites.
- **`index.d.ts` is committed and is read by the ArkTS compiler**, via a local folder package declared in `entry/oh-package.json5` whose `types` field points at it. That association is what makes the declaration authoritative instead of decorative: with it, a bridge call that no longer matches a Rust export fails the build; without it, every import from the module is `any`. A Rust export change that skipped *regeneration* is a separate failure the compiler cannot see, so `scripts/check-bindings.sh` regenerates beside the committed copy and diffs.

See [UI_ARCHITECTURE.md §5](UI_ARCHITECTURE.md) for the two generated binding surfaces, where their outputs live, the event names that cross the boundary, and what is still missing.

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

### The context protocol is a view, not a vault

`ContextSource` / `ContextRecord` is the unified shape the model searches and
reads. It is **not** where domain data lives. Characters, personas, world books,
conversations and prompt profiles are stored as themselves, per entity, and are
projected into context sources for the cases where a model-facing read of them
is genuinely useful.

```text
Character / Persona / WorldBook / Conversation / PromptProfile
  -> independent domain storage (conversations/, characters/, personas/, ...)
  -> projection into ContextSource / ContextRecord
  -> search_context / read_context
```

`project_library` performs that projection. A source it created is marked as
projected and is **not** written back as stored data, so the projection cannot
drift into a second source of truth. Persistence and editing are per domain
entity; only retrieval is unified.

Two things deliberately stay out of the projection: a prompt profile and a
transcript. Both reach the model through prompt assembly, so making them
searchable would give the model a second, worse way to read what it is already
told every turn.

## Prompt pipeline

```text
App system prompt (or the conversation's PromptProfile system prompt)   [system]
  -> BeforeCharacter world book                                         [system]
  -> each participant's character system prompt                         [system]
  -> each participant's character definition                            [system]
  -> persona: who the user is                                           [system]
  -> persona: what the user asks of the model                            [user]
  -> AfterCharacter world book                                          [system]
  -> example dialogue                                                  [system]
  -> format rules                                                      [system]
  -> profile user prompt                                                [user]
  -> extra profile segments, by declared position and role
  -> prompt block          <- cacheable; each piece at its semantic position
  -> conversation history  <- verbatim transcript, append-only
  -> NearHistory world book
  -> post-history instruction, then the profile's post-history segments
  -> current user input
  -> PromptPlan
  -> sujiu-ai
  -> provider
  -> optional tool-call loop (each round appends steps to the transcript)
  -> final assistant response
```

`priority` means client-side retention/ordering priority. It is not presented as a magic model attention weight.

### A fixed instruction speaks from the side it belongs to

Role and position are stored on a profile segment rather than assumed, because
"a fixed prompt segment" is not one thing. A format rule is a system
instruction. A refusal rule belongs after the transcript, where it can still see
the turn it applies to. A standing user request is the user's own message.

Folding all of them into one block of system text at the top is what made
`PromptProfile.user_prompt` a dead field: the content existed and nothing could
carry it. A `PromptProfileSegment` therefore declares a `role` and a
`position` (`Prefix` or `PostHistory`), and the compiler sends it as a message
of that role at that position. A persona is split the same way — who the user is
is the system's account of the conversation, what the user asks for is the user
speaking. A system message that speaks for the user is a message that
misattributes it.

`PromptPosition` is deliberately coarser than a world-book position: it answers
which side of the conversation the text is on, not where in a lore block it
belongs. A segment with no declared role or position reads as system/prefix,
which is what a segment was before those fields existed.

### How much lore one request may carry

World books are injected directly now, so "how much" is a decision the runtime
makes: a book that matches two hundred entries cannot all go into every prompt,
and a runtime that either sends everything or sends nothing is not making a
decision. `WorldBookBudget` bounds it — an entry count and a character count,
both client-side limits on what is pushed into a request, not claims about model
attention.

Constant entries go first: the book's author marked them always-on, and an
instruction conditional on a keyword scan is not what they asked for. They are
still bounded, and a dropped constant is reported with `constant: true`,
because a book whose constant entries alone exceed the budget is a fact about
the book rather than a budget decision. Keyed hits then compete by priority. An
entry too long for what remains of the character budget is **skipped, never
truncated**: half a lore entry is a fact the model cannot use, and a partial one
costs the same as a whole one.

The kept entries stay in priority order regardless of which pass chose them —
deciding *what* to send must not silently relocate what was always going to be
sent. And what was left out is reported on the plan in
`dropped_world_book_entries` rather than quietly omitted, so a request that sent
less lore is distinguishable from one that sent all of it. A dropped entry is
still in the book and still one `search_context` away.

### Three layers, and only the third is a tool

The pipeline is layered by how the content gets in, and the layer decides
whether the model is involved at all.

1. **Injected directly, every turn.** The system prompt, the current persona,
   every participating character definition and scenario, the format rules, the
   stable instructions from the character and the prompt profile, and the
   constant (always-on) world-book entries. The model must never have to call a
   tool to learn who it is playing.
2. **Filtered by the runtime, then injected.** Ordinary world-book and lorebook
   entries. The prompt compiler decides by keyword, scope, priority, position and
   budget, and the hits go straight into the prompt. World-book triggering is
   prompt-compilation and context-assembly behaviour, not model behaviour.
3. **Tool calls only, as a fallback for what is large or on demand.** Old
   history, large long-term memory, external documents, big databases,
   unpredictable relevance, heavy resources.

A character card, a system prompt, a persona or a current-conversation world book
is **never** a tool call. The flow this avoids is the expensive one: the model
thinks, calls a tool to fetch the card, reasons again, maybe calls a world-book
tool, and only then answers.

```text
Core assembles a stable prefix
  -> runtime filters the world book
  -> one request
  -> tools only to supplement what did not fit
```

That ordering is also what makes prompt caching pay: the stable prefix stays
byte-identical across turns, and a world-book hit that changes is a declared
cache break rather than a reordered prompt.

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

#### What a turn cost

A turn with tools is several requests, and the number charged is the sum over them. `Turn::usage()` returns `TurnUsage` — rounds, tool rounds, input, cached input, cache writes, output — and every step's reported usage is absorbed into it. Reading only the last step reports the cheap round, which is the same failure as the double-billed prefix run backwards: the request looks cheaper than it was, so nothing looks wrong.

`None` means **not reported**, not zero. A provider that reports no cache writes has told us nothing about cache writes, and collapsing that into `0` would make a real cache bill look like a free one. `rounds_reported` alongside `rounds` is what distinguishes a complete ledger from a partial one.

It is provider-neutral because the adapters have already normalised the spellings: `input_tokens` and `prompt_tokens`, `cached_tokens` nested under either `input_tokens_details` or `prompt_tokens_details`, `cache_creation_input_tokens`. The numbers are recorded exactly as the provider accounted for them.

The kernel computes **no price and no currency**. Not because it is inconvenient, but because a price in the kernel is a price that goes stale, and a wrong one is worse than none: it would be acted on. Pricing belongs to whoever is paying the bill.

`SujiuRuntime` writes each turn's ledger as a `turn_usage` diagnostic carrying the endpoint, the protocol, the model and the numbers, and it writes it for cancelled and failed turns too — those rounds were sent, and they were billed. A compaction summary is a request as well, and its usage is recorded with the compaction entry, because it is the largest hidden spend in a long session.

Current concrete transports:

- OpenAI Responses
- OpenAI-compatible Chat Completions

Planned adapter reuses the same runtime:

- Anthropic Messages
- Gemini GenerateContent / function calling

## Protocol negotiation

What an endpoint can do is the endpoint's business, so the runtime asks instead of guessing. Guessing by vendor or model name does not survive contact with the world: a gateway serves any model under any name, a service renames its models, and a name like `reasoner` or `r1` means nothing to the next one. So no capability is read out of a hostname or a model string anywhere in the runtime.

### The user does not choose a vendor or a protocol

There is no vendor field in the configuration. A user supplies a base URL and a key, and Sujiu works out what is there. The only thing they are then asked to decide is the model, because that is the one choice a machine genuinely cannot make for them.

A hostname may still produce a friendly word to show a person — `api.deepseek.com` shows as "DeepSeek", an unrecognised gateway shows as its hostname. That label decides no protocol, no capability, no continuation and no tool behaviour. A display label that changed how a conversation was spoken would be the same mistake wearing a different hat, so `EndpointConfig::display_label()` is the only place a host is allowed to say anything.

`EndpointConfig` carries only what a user can honestly supply: an endpoint id, a base URL, a credential reference, a chosen model and optional advanced overrides. `selected_model` is optional because discovery runs before a model is known.

### Discovery comes before the model

The order of work is the other way round from what the configuration shape suggests:

```text
base URL + key
  -> explore the endpoint
       |- model listing: what is there, or why not, and a model may still be typed
       `- protocol negotiation: which wire format answers, best first
  -> the user picks a model, or types one
  -> the endpoint and the chosen model are saved
```

Exploring an endpoint writes nothing to disk. A settings screen asks before anything is saved, so the list it shows is what the endpoint actually offered rather than what an earlier choice left behind. `discover_endpoint` needs nothing but an address and a key: requiring a saved configuration first would put the choice in front of the question it is chosen from.

Model listing and protocol negotiation are independent. A gateway can list nothing and still speak Responses; a gateway can list fifty models and speak only the oldest chat protocol. One of them failing says nothing about the other.

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

#### A protocol you cannot send is not a protocol you offer

The priority list is what gets **probed**. What gets offered is `IMPLEMENTED_PROTOCOLS`, and the two are not the same thing. Anthropic Messages is probed for real — the request shape, the credential header and the version header all work — and there is no adapter behind it, so it is not in `IMPLEMENTED_PROTOCOLS` and `SujiuRuntime::supported_protocols()` does not advertise it.

Listing a protocol that has no adapter is worse than not probing it. Negotiation selects from what the endpoint supports, so an Anthropic-only endpoint would be handed `AnthropicMessages`, and the runtime's adapter choice — Responses, else Chat — would send it to a Chat Completions route the service does not serve. The user is told the endpoint speaks something it cannot speak, and the failure arrives as a rejected request rather than as an honest "this build cannot talk to that endpoint yet".

So negotiation reports it as what it is: the endpoint speaks a protocol this build has no adapter for. Probing it stays worthwhile, because that sentence is only reachable if the endpoint was asked. Adding the protocol back means adding the adapter in the same change.

`build_provider` matches the negotiated protocol explicitly and turns anything else into that failure. It is deliberately not an `else`, because a silent fall-through is how a negotiated Anthropic conversation ends up spoken in a dialect the endpoint never agreed to.

### The identity is built after the protocol is known

```text
EndpointConfig
  -> negotiate the protocol
  -> ProviderIdentity { protocol, endpoint_id, base_url, model }
  -> match stored continuation state against that identity
  -> build the adapter
```

The order is the point. `ProviderIdentity` is what stored continuation state is matched against, so building it before negotiation means asking "is this state mine?" with a protocol nobody has established yet. There is no honest answer available at that moment, and the tempting fill-in — `Protocol::default()`, which is `OpenAiResponses` — turns "unknown" into a specific claim. A Responses handle would then be found for an endpoint that turned out to speak Chat Completions, and replayed as a `previous_response_id`.

`Transcript::continuation_event()` therefore takes no identity. It answers the question that can be answered without one — *what was the last thing any provider said about continuation* — and the identity filter is applied afterwards, against the protocol that was actually negotiated. A stored `Clear` still stops the walk, still regardless of protocol: a cleared handle is dead for everyone.

This is also why a protocol downgrade cannot resurrect Responses state. The identity filter runs after the protocol changed, so the match fails and the transcript is sent whole, which is what the endpoint has actually seen.

### A failed probe is not a missing protocol

The single most important rule: **only evidence that a path does not exist may move negotiation to the next protocol.** Every other failure is the service declining to answer right now, and recording that as "this endpoint cannot speak" is how a gateway ends up permanently misclassified after one bad minute.

| Response | Verdict | Why |
| --- | --- | --- |
| 2xx | supported | the only positive evidence |
| 404, 405, 410, 501 **and** a body naming an unknown endpoint, path or route | unsupported | the service says the route is unknown |
| 404, 405, 410, 501 **and** a body naming a model, function or deployment we may not use | inconclusive, model unavailable | the path is fine; this key cannot reach that model |
| 404, 405, 410, 501 with anything else, including an empty body and plain text | ambiguous | could be a missing path, could be an unroutable request; see below |
| 400/422 naming an unknown endpoint, path or route | unsupported | the service says the route is unknown |
| 400/422 saying anything else | inconclusive | usually a bad request, or a typo'd model |
| 401, 403 | inconclusive, credentials | a key problem, never a protocol fact |
| 429 | inconclusive, rate limited | must never downgrade a protocol |
| 5xx, timeout, connect failure | inconclusive, unavailable | transient by nature |

Negotiation stops on an inconclusive verdict rather than walking on: asking the next protocol after a rate limit is asking a throttled service more questions. It walks on after an ambiguous one, because a missing route on the protocol being tried is still the likeliest reading and the next protocol costs one request; it simply records nothing.

A "not found" phrase only counts when it names an endpoint, a path or a route. A bare "not found" is deliberately excluded, because `model not found` is the most common rejection a perfectly usable endpoint returns, and reading it as a missing route would downgrade a working provider on the first typo.

### A 404 status is not evidence on its own

This one was found by asking a real gateway, and it is worth writing down because the obvious implementation is wrong in a way no test invented in advance would have caught.

Services answer 404 for at least three unrelated things, and on real gateways two of them are byte-for-byte identical:

- the path is not implemented, and the body names the route as missing
- the path is fine, but the model is retired, not deployed, or not reachable from this key
- neither, and the service replies with its router's own plain-text page

The third case is the trap. A gateway that **routes by model** answers a request for a model it cannot resolve with the very same `404 page not found` it uses for a path it never had. Reading the status alone therefore reports that a working endpoint speaks none of our protocols, and the user is sent off to reconfigure something that was never broken.

The verdict is therefore three-way, and only one of the three is proof:

| Verdict | Means | Negotiation |
| --- | --- | --- |
| `Unsupported` | the body positively names a missing route | move to the next protocol, and remember that this one is absent |
| `Ambiguous` | a bare 404, 405, 410 or 501 that is equally consistent with a missing path and an unroutable request | move to the next protocol, but draw no conclusion |
| `Inconclusive(reason)` | credentials, a rate limit, a transient service failure, a model this key may not use, or a body we cannot read | stop, and report the reason |

`Unsupported` is the only proof of absence, and it is the only verdict worth remembering. `Ambiguous` is worth walking past but worthless as a fact, which is why the two are different verdicts rather than one with a flag.

It follows that the final message differs too. When every attempt came back ambiguous the runtime says **"we could not tell what it speaks"**, and only says "this endpoint offers none of the protocols this build speaks" when the endpoint actually said so. The first is recoverable and the second is a false statement the user will act on. A 410 joins the ambiguous group, because a retired model is the same shape of answer as a missing route and is not evidence about the path.

A probe must still name a model, and before a model is chosen there is none to name. The placeholder that stands in for it is safe *because* of the rule above rather than in spite of it: a service that answers a request at all has still proved the protocol is served, and a service that does not produces an inconclusive verdict instead of a false one. The fix belongs in how the answer is read, not in what we ask.

### A service that goes quiet is a fact about the service

Every provider call goes through one client with a connect timeout and a read timeout. A default client has neither, and a request to an endpoint that accepts the connection and then says nothing never returns: the turn sits on "sending" until the user finds the stop button, and nothing anywhere says why.

The read timeout measures the **gap between reads**, not the length of the response, so a long answer that keeps streaming is never cut off. Only silence counts, and silence is bounded. When it fires, the failure is a transport failure, which is the shape the rest of the runtime already knows how to report: not a protocol the endpoint lacks, and not a completed turn.

**Silence and slowness send the same bytes.** The read timeout is 120 seconds,
and that is a measured compromise rather than a round number. A slow endpoint
sends *exactly* the same bytes as a socket that died — nothing at all — and no
client-side setting can tell them apart. Measured against NVIDIA's free tier in
`local/endpoints.json`, on one account and one base URL:

| model | to first byte |
| --- | --- |
| `/models` listing | ~1s |
| `z-ai/glm-5.3-flash` | **6.3s** |
| `deepseek-ai/deepseek-v4.1-flash` | **62–81s**, every time |

At 60 seconds the second model was cut off having learned nothing. At 120 the
same turn completes, streaming `ThinkingDelta` and `TextDelta` before
`TurnCompleted`.

The spread is the real lesson, and it is worth stating plainly: **latency here
is a property of the model, not of the endpoint.** One free tier serves a
6-second model and an 80-second model from the same address with the same key, so
"this endpoint is slow" is not a fact any client can establish. That is exactly
why the timeout cannot be a judgement about one endpoint, and why it is global.

Raising it is not free, and the cost was accepted deliberately. It buys slow
models at the price of a dead endpoint taking twice as long to admit it, which is
the failure this timeout exists to prevent. 120 seconds is where the slow model
in that table fits and the cost is still tolerable; past that, more of the second
failure is bought back with more of the first, and where that stops being worth
it is a judgement call rather than something a measurement settles. The timeout
was also left **global** rather than per-endpoint, so a slow service is fixed in
one place instead of turning every configuration screen into a tuning panel.

The failure is reported as what it is either way — a transport failure with a log
line naming the stage — rather than being silently tolerated.

### A probe without a model is blind to a gateway that routes on one

A request has to name a model, and on a settings screen no model has been chosen
yet, so the probe sends a placeholder. Against an endpoint that ignores the model
field that works. Against a gateway that dispatches on it, it is blind: the
answer for "no such model" is the same `404 page not found` the same gateway
gives for a path it never had, so the walk comes back ambiguous and the runtime
can honestly say only that it could not tell.

Honest is not the same as useful. A screen that reports "we could not tell" about
an endpoint that works is a screen that cannot help anybody, so when the model
listing — which runs first and independently — already returned a model the user
could have picked, **the probe asks again with that model**. It is one extra
request, spent only on the path that learned nothing: `negotiate_asking` retries
exactly when the first walk came back `undetermined` *and* the listing produced
something to ask with. The second attempt logs `probe_retry` and `probe_retry_end`
so a log shows both questions and both answers.

The retry is honest about its own limits. A listing whose models are all
unservable still comes back inconclusive, and that is the truthful answer rather
than a guess. The live endpoint is a real instance: its listing returns 81 models,
most of which 404 for a given account, so the retry names the first one, is told
"no such function", and reports that it still could not tell.

That first name is chosen without knowing which models are servable, so a retry
can pick a dead one. What makes this recoverable is that the question is asked
again the moment it can be answered: once a model is chosen, the probe names it,
and the same endpoint that answered `undetermined` now returns `asked=2,
selected=openai_chat_completions` off a `404 page not found` and a `200`. A
**turn** therefore never needs the retry, because a turn already has a model and
names it. The retry exists for the settings screen's first visit — the one
moment the runtime is asked about an endpoint it has never spoken to and has
nothing to go on.

### The cache is an optimisation, not a fact

Results are cached by endpoint id plus a normalized base URL (scheme and host lowercased, one trailing slash removed, the path left alone because `/v1` and `/v2` are different APIs). A cached answer is reused, a transient or inconclusive answer is never cached, and reconfiguring an endpoint forgets the whole cache: an endpoint id may have been re-pointed at a different address under the same name, and a stale answer about an endpoint we no longer talk to is worse than no answer.

A manual override still exists for a gateway the probe cannot reason about, but it is an advanced compatibility fallback, not the normal path. A platform describes the endpoint; it does not know that one protocol requires an extension field on tool calls.

### Model discovery is separate

Listing models is a settings-screen convenience and never a step in a conversation. It is a real fallback chain rather than one request: the OpenAI-compatible listing, then the Anthropic-compatible shape (`x-api-key` plus a version header, because some gateways that speak only Anthropic reject `Authorization` outright and answer 401 to a request that should have been asked more politely), then a versioned host-root route for a base that is not already versioned. A response body is read as a list in whichever of the usual shapes it arrives, and the chain moves on only when a route is missing — a rejected key or a rate limit is an answer about this request, and trying the next strategy would spend the user's key again and still report the wrong reason.

Every outcome — no listing endpoint, a key without permission, a rate limit, an unreachable host — still leaves manual model entry available. Not having a listing endpoint says nothing about whether the endpoint can chat, and the discovered list never influences which protocol the transcript uses.

### One outcome, one status

A settings screen that says "探测失败" for every failure is a screen that cannot help anybody, and the user cannot act on it. `Negotiation::status()` therefore returns one of a fixed set:

| status | meaning |
| --- | --- |
| `supported` | a protocol answered |
| `no_usable_protocol` | every protocol was asked and every answer said the route is not there |
| `credentials_rejected` | the key was refused |
| `no_such_endpoint` | the service named a route it does not have |
| `rate_limited` | try again later |
| `server_unavailable` | the service answered 5xx, or refused a model |
| `network_error` | nothing answered at all |
| `unreadable` | something answered in a shape the runtime cannot parse |
| `undetermined` | the runtime declined to conclude — not an error. A settings screen retries once with a listed model first |

`server_unavailable` and `network_error` come out of the same `ProbeFailure::Unavailable` and are separated by whether any attempt got an HTTP status at all. Same verdict, opposite fix: one is the service's problem, the other is the address, the network or the key.

`undetermined` is not a failure and must never be drawn as one. A probe that stopped early because the model was unavailable is the runtime refusing to guess, and rendering that as an error teaches the user to distrust a screen that is being careful.

The same discipline applies to the listing, which reports `available` / `unavailable` / `permission_denied` / `rate_limited` / `unreachable` / `unknown` separately from the protocol status. A gateway that chats fine and lists nothing is a working configuration, and the screen has to be able to say so.

## Diagnostics

Negotiation and discovery are hard to debug by hand, because the interesting question is never "did it work" but "which of the six things failed": a typo in the address, a rejected key, a missing route, a rate limit, an endpoint that lists nothing, or a service that says something the runtime cannot read. Guessing from a blank model list is not debugging. So the runtime keeps a log of what it asked, what came back, and what it concluded.

`DiagnosticLog` is a bounded ring of `DiagnosticEntry` records. Each entry is a timestamp, a `DiagnosticKind`, a stage, a message and a set of named fields. The ring holds 400 entries and drops the oldest past that, because an unbounded log in a chat app is a file that grows forever and eventually gets deleted by the thing it was written to help.

### Two halves, and why they are separated

`DiagnosticKind` is `Discovery` or `Chat`, and that split is load-bearing rather than cosmetic. A settings screen's probing and a conversation's tool-calling both speak HTTP, and a user reporting "the model list looks wrong" or "it stopped calling tools" is asking about entirely different halves. Mixed together, a question about continuous tool calls has to be answered by reading past a run of listing attempts. Filtered, each is a few lines.

`discovery_diagnostics` returns the first half alone; `diagnostics` returns everything.

### A credential is never in the log

This is the hard requirement, because the most useful things to log — a request URL, an auth header, a response body — are exactly the places a key lives. So no message and no field value is stored before passing through `redact`.

The redaction is not a regular expression over `"sk-"` and it is not "truncate the body". A credential appears in several shapes, and each needs a different answer:

| shape | what it looks like | what the log keeps |
| --- | --- | --- |
| a bare key | `sk-proj-1234567890abcdef` | `sk-****cdef` — enough to tell two keys apart, not enough to use one |
| a named field | `"api_key": "sk-..."`, `x-api-key: ...` | the field name, and `****` for the value |
| an auth header | `Authorization: Bearer eyJhbG...` | the header name, and nothing after it |

The header case is why an unquoted value runs to the end of the line rather than to the next space: `Bearer eyJ...` is **one** credential written as two words, and stopping at the space would leave the token sitting in the log in plaintext while appearing to have redacted it.

Redaction removes a secret rather than truncating around it, and it keeps the prose around the removal. A log line that reads `bearer **** rejected` is still a diagnosis; a line reduced to nothing is not. `text_without_a_secret_is_left_exactly_as_it_was` and `nothing_written_to_a_log_can_contain_a_credential` are the two tests that hold this, and they are the ones to re-read before changing any of it.

Response bodies are collapsed to a single line and capped at 512 characters. A body is worth a truncated excerpt — often the error message is the whole answer — and is never worth an unbounded copy.

### The stages

Stages are named so a log reads as a sequence rather than a pile:

```text
discovery
  discovery_request      address, masked key
  probe_start            which protocols are about to be asked
  probe_request          POST url, model used
  protocol_result        protocol, HTTP status or "no answer", body excerpt
  probe_end              what was selected, what was asked, why
  model_listing_request  strategy name and url
  model_listing_result   listing outcome, count, up to 20 model ids
chat
  endpoint_configured    address, chosen model
  chat_request           protocol actually used, model, message count
  turn_completed         steps, tool calls
  turn_cancelled
  turn_failed            the stage it failed in
storage
  save_failed            what could not be written, and that the previous generation is still live
```

`turn_failed` carries the stage rather than only a message, because "it stopped working" and "the model returned something unreadable" are different bugs and the same user report.

`save_failed` is a stage of its own because a failed write is the one failure
with no other symptom: the turn succeeded, the answer is on screen, and nothing
anywhere says the conversation was not kept. It is logged and returned rather
than swallowed, because a write that fails quietly leaves the manifest claiming
a document was saved.

### Persistence is separate from the conversation

The log is stored under its own key, `sujiu-diagnostics.json`, and saved by its own path. That is not tidiness: the conversation document has a version and a migration and a protection rule for documents this build cannot read. A log written by a newer build is not a conversation, and a log this build cannot parse must not put the conversation store into a protected state where the next save is refused. The two never block each other.

`clear_diagnostics` touches the log and nothing else. Endpoint, key reference and conversations survive it, because a user clearing a log is asking to start a fresh trace, not to reconfigure their account.

## The Responses adapter

An endpoint that speaks Responses gets Responses, because that transport carries continuation, reasoning and tool state in its own shape rather than in fields bolted onto a chat message. Chat Completions can be *made* to carry all three, but only by inventing places to put them and hoping the endpoint understands.

The differences are concrete:

- the request is a list of `input` items, and one provider-neutral message can be several of them — an assistant message that both spoke and called tools becomes a `message` item plus one `function_call` item per call
- a tool result is a `function_call_output` item keyed by the same `call_id`, which is what makes the pairing structural rather than conventional
- tool definitions are flat (`{type, name, description, parameters}`) instead of nested under a `function` key
- continuation is a real server-side handle: the completed response id is stored as a chainable `ProviderContinuation` and sent back as `previous_response_id`
- reasoning is an output item the endpoint produced, so it is deliberately **not** re-sent as text. There is no field an adapter could fill with it, and a reasoning *summary* is not the reasoning

A native handle does not replace the transcript. It lets a request skip the part the endpoint already holds. The transcript stays the portable base, and it is fully re-sent whenever the endpoint, protocol or model changes. A handle from a different identity, or a Chat Completions completion id — which is a label, not a handle — is never accepted as one.

#### A handle is used inside one turn, and stops at the turn boundary

The scope of a `previous_response_id` is **one user turn's tool loop**. Round one sends the conversation; every round after it names the handle from the previous round and sends only what the endpoint has not seen, which is what the double-billing bug was. At the start of the next user turn, no handle is sent, even when the recorded prefix still matches the prompt.

That boundary is not an oversight and it is not only about the prefix. A response id names something that exists on the other side of the network, and it expires: OpenAI retains Responses state for about thirty days by default, and a third-party compatible endpoint may keep it for less. A stored handle is therefore a claim about the past that a fresh user turn has no way to check — and an agent loop treats any provider error as the end of the turn, so a session opened after the expiry would fail its first new message even though the local transcript is intact and the prefix is unchanged.

What cross-turn chaining would buy is a smaller request body. What it costs is a session that can stop being reopenable. Those are not close, and the transcript is sent whole instead.

The handle is still **persisted** on the step. It is a record of what the provider gave and what it accounted for, which is worth keeping for diagnostics; it is simply not something a later turn sends. `Transcript::continuation_for` — which answers "may this state be replayed to that identity", a necessary but not sufficient condition — is kept for exactly that record and is not on the live path.

Reusing a handle across user turns again would be a deliberate change, and it would have to bring an expiry story with it: recognise the rejection that means "that response is gone", clear the handle, and retry once without it. Classifying an error by its text is the kind of guessing this architecture avoids elsewhere, which is why it is not here yet.

#### Reasoning stops at the same line

On Responses, reasoning is an **opaque output item inside that chain**. It is not a string an assistant message happens to carry, and there is no field an adapter could fill with the text we kept: re-sending it would be inventing a wire shape the protocol does not have, and passing a reasoning summary off as the reasoning itself would be lying to the next request about what the model still has.

So reasoning travels with the handle and stops where the handle stops: replayed inside one user turn's tool loop, absent from the first request of the next one. A new turn inherits the visible transcript and starts without the reasoning that produced it. That is a real loss and it is a deliberate one — the alternative is replaying the endpoint's own opaque items across turns, which needs the items stored verbatim, a lifecycle for them, and an expiry story for the chain holding them. The same problem `previous_response_id` has, and the same reason it is scoped to a turn.

The sidecar is still stored on the step, so the reasoning is not thrown away: it is in the transcript for diagnostics and for any future feature that can carry it honestly. What does not happen is a summary of it being passed off as the reasoning itself.

#### What a handle records, and when it is believed

A handle replaces a prefix, so it has to say which prefix. `ContinuationCoverage { sent, digest, assistant_messages, wire_items }` is that answer, and the interesting part is which unit it counts in.

It counts **provider-neutral messages**, not wire items. One message becomes several items — an assistant message that spoke and called tools is a `message` plus a `function_call` each — and the response adds its own output items on top. A single `usize` cannot keep meaning both, because the two counts drift apart permanently and a reader that picked the wrong one would skip the wrong number of messages. `wire_items` is recorded as an audit number and is never used to skip anything.

`sent` is measured over **this** request, not inherited from the handle this request continued from. The endpoint holds everything that was sent to produce the response, so that is what the new handle covers; carrying the old count forward is how a stale prefix gets re-sent and billed twice.

`assistant_messages` exists because the response's own output is in the endpoint's copy and did not exist when the request was built. Tool results are deliberately **not** counted: the endpoint has never seen them.

Believing the handle is a separate question from recording it. `proven_covers()` re-digests the covered prefix of the messages actually being sent and only shortens the request when the digest matches and the count still fits. `message_prefix_digest` is a non-cryptographic FNV-1a over the serialized messages, chosen because this needs a change detector and not a security primitive.

That proof is what keeps a turn's own rounds honest. A world-book entry matching for the first time, a changed persona, a changed PromptProfile, a different near-history window — each of those changes the prefix, the digest stops matching, and the whole conversation goes out again with no handle named, even inside one user turn. The rule is not "there is a handle, so chain": it is **name the handle only when the endpoint's copy is still a prefix of what we are sending**, which is the same condition that authorizes skipping.

`previous_response_id` is therefore set exactly when `covered > 0`, not whenever a handle exists. Splicing an endpoint's older, differently-shaped prompt onto a request that already re-sent everything would produce a conversation the user never had, and bill for it.

A coverage record from before the digest existed is read and **not believed**: the bare count has nothing to check against, and a count that cannot prove anything shortens nothing. Old documents lose a capability they cannot honestly claim, which is the same rule the reasoning sidecar and an unreadable document follow.

The digest covers the request prefix only. The assistant messages the response itself produced are skipped by the `assistant_messages` count rather than by a digest, because they were not in the request when it was built. That is safe today — history is append-only and nothing edits a turn — and it is the constraint a future editing or regeneration feature inherits: **any mutation of stored history has to invalidate continuation**, or a handle will be considered valid against a transcript it no longer describes. Either the lineage fingerprint grows to cover what the response produced, or the mutation refuses to happen while a handle is live.

#### What continuation saves, and what it does not

It is tempting to read a handle as free history. It is not. Input tokens earlier in a chain are still billed as input tokens: `previous_response_id` makes the endpoint *reuse* a prefix, not make it *free*.

What it avoids is the prefix being carried twice — once inside the server's chain and once stuffed into this round's input. That is double context, and double context is both a bill and a context window spent on nothing. On a request where the client would otherwise have re-sent the whole transcript, naming the handle is the difference between one copy of that history and two.

So whether chaining or caching actually saved anything is read from the usage ledger — `input_tokens` and `cached_input_tokens`, as the provider reported them — and never from counting items in an HTTP body. A body is what was sent; the ledger is what was charged, and only the second one is the number anyone is paying.

### The stored layout is rooted at the conversation

Persistence follows the domain, not a single blob:

```text
sujiu-library.json                       manifest: generation, ids, endpoint, stored context
generations/gen-<n>/conversations/<conversation-id>/conversation.json
generations/gen-<n>/characters/<character-id>.json
generations/gen-<n>/personas/<persona-id>.json
generations/gen-<n>/worldbooks/<world-book-id>.json
generations/gen-<n>/prompt_profiles/<profile-id>.json
```

A conversation owns a directory. Everything else is a file per entity, because
those entities are independent and may be shared between conversations. There is
no `characters/<id>/chats/…`: a character is not the root, and a world book
filed under a character could not outlive it.

### A domain id is not a path

An id is a **logical** id. It is whatever an imported card, a world book or a
migrated document happened to call the entity, so it can contain `/`, a
backslash, `..`, or nothing at all. Splicing such a value straight into a key
would let it choose the directory its document lands in — leave the generation
it belongs to, overwrite a sibling, or land on the manifest itself. That is a
storage bug with no error message: the store loads, and the wrong file is there.

So every id goes through one mapping, `documents::path_segment`, on the way in
and on the way out:

- letters, digits, `.`, `_` and `-` survive; every other byte becomes `%` plus
  two uppercase hex digits
- a segment that would be exactly `.` or `..` is escaped whole
- an empty id gets a marked name, and an id too long for a path component
  (percent-encoding triples it) gets a name derived from a stable 64-bit hash

Three properties are load-bearing:

- **Round trip.** Both sides call the same function, so a load looks in exactly
  the place a save wrote, with nothing to decode.
- **No collisions.** The output alphabet is the safe set plus well-formed escapes,
  and `%` is itself escaped, so a safe id can only be produced by itself. The
  marked names use `~`, which that alphabet cannot contain.
- **Stability for ordinary ids.** `session-1` maps to `session-1`. A store
  written by an earlier build keeps resolving, so this rule needs no migration of
  its own.

The manifest stores the logical id, unchanged. Nothing above this layer knows the
mapping exists — including a platform, which must not read these documents at
all.

The manifest is written **last**, and it names the generation it describes. That
is the whole transaction: a save writes a complete new generation directory,
then one atomic write switches the manifest over, then the previous generation
is removed. Any failure before the switch leaves the previous generation
complete and still the live one — a crash cannot produce a manifest that points
at a mixture of new and old files, which is what overwriting documents in place
would do.

Projected context sources are filtered out on save, so the projection never
accumulates in the manifest as if it were stored data. A save that cannot write
a document returns the error instead of reporting success, and the manifest is
never written after a failed step.

### What a stored document has to survive

Persisted state is versioned and migrated, and the shape of a stored conversation
has changed three times. A version 1 document stored conversations as a flat
list of text messages. A version 2 document stored real transcripts. A version 3
document kept those transcripts and stored an endpoint instead of a provider
config. A version 4 library stores each domain entity as its own document under
a conversation-rooted layout.

The migration has to know which of those it is reading, because a version 2
document read through the version 1 reader parses without complaint and returns a
conversation with no turns — the file loads, the store is writable, and the
conversation is simply gone with nothing saying so.

Two migrations carry real data, so neither is allowed to drop anything:

- a stored conversation with a single `characterId` becomes a conversation with
  one participant, keeping its id. Renaming the id would break any row a
  platform already stored.
- a world book embedded in a character card becomes a standalone world book
  whose id is then listed in that card's `worldbook_ids`. It is read out of the
  **raw JSON** before the card is parsed, because parsing the typed character
  first would silently discard the lore with nothing to report.

## Dependency direction

```text
platform UI -> FFI -> sujiu-ai -> sujiu-core
                       |
                       +-> provider adapters

compat codec -> sujiu-core
built-in tools -> sujiu-core data
```

The Rust runtime never imports a platform frontend.
