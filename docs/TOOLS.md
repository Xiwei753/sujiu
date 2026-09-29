# Tool and context protocol

Sujiu keeps model-visible tools small and stable. Domain growth should normally add
new data behind existing tools rather than add a new tool for every feature.

The internal tool representation is intentionally MCP-shaped:

- `name`
- `title`
- `description`
- `input_schema`
- optional `output_schema`
- annotations such as read-only/idempotent/destructive/open-world hints
- result `content`
- optional `structured_content`
- `is_error`

Provider adapters translate this representation to OpenAI, Anthropic, Gemini or
other wire formats. Local discovery metadata is kept separate and is never
treated as model attention weight.

## Standard context model

World lore, past plot, long-term memory and old chats are all retrievable
context. They differ by kind and authority, not by tool protocol.

| Context kind | Intended contents |
| --- | --- |
| `world_lore` | World books, setting facts, places, factions, rules, items |
| `story_event` | Canonical plot events, scene summaries, timeline entries |
| `character_memory` | Durable character facts, relationships, promises, learned information |
| `chat_history` | Older raw conversation records outside the immediate prompt window |
| `persona` | User/player persona facts made available to the roleplay |
| `note` | Author notes and other searchable local notes |
| `other` | Extension data that does not yet deserve a new stable kind |

Every record has a stable URI, source ID, kind, title, content, optional
keywords/tags/timestamp, scope and metadata.

The URI is a retrieval address. It is deliberately independent of SQLite,
files, embeddings or any other storage implementation.

## The three standard read tools

### `list_context_sources`

Lists available context collections and metadata. It never dumps an entire
source.

Use it when the model needs to know what collections exist or needs a
`source_id` to narrow a search.

### `search_context`

Searches all or selected context kinds and sources.

Input supports:

- natural-language query
- optional `kinds`
- optional `source_ids`
- result limit
- optional time bounds

It returns ranked **snippets and URIs**, not full database contents.

The storage backend owns ranking. The first implementation is lightweight
keyword scoring; a later SQLite FTS/BM25/vector/hybrid implementation can
replace it without changing the model-visible schema.

### `read_context`

Reads the complete records for a small list of URIs returned by
`search_context`.

This produces MCP-style resource content plus structured records. The model
should search first and read only the few hits it actually needs.

## Fast path versus tool path

There are two context paths and both are intentional.

```text
cheap deterministic path
  recent input/history
    -> keyword/rule match
    -> a small number of known-relevant entries
    -> PromptPlan

deferred model path
  model sees list_context_sources/search_context/read_context
    -> search_context
    -> ranked snippets + URIs
    -> read_context on selected URIs
    -> continue generation
```

A tiny, obvious world-book keyword match should not spend another model round.
A large lorebook, distant plot event or ambiguous memory should not be dumped
into every prompt.

## Tool discovery

The three core context tools are intentionally small enough to remain available
when a context store is registered.

Other tools may remain deferred. `sujiu_search_tools` searches Sujiu's local
tool catalog and exposes matching schemas on the next model round.

This keeps a large plugin/tool installation from consuming the context window
on every request.

## Memory writes are a separate pipeline

The roleplay model does **not** receive an unrestricted `write_memory` tool by
default.

A generated roleplay answer is not automatically a canonical fact. Directly
writing its own claims into durable memory would allow hallucinations and
temporary narration to become permanent truth.

The planned write path is:

```text
conversation turn
  -> candidate extractor / scene summarizer
  -> classify (story event / character memory / discard)
  -> validate and deduplicate against existing records
  -> reconcile contradictions / authority
  -> persist
```

A future explicit editing tool can be added for user-authorized memory changes,
but persistence policy remains separate from the read protocol.

## Long conversations

Immediate recent messages stay in the normal prompt window.

Older material can be represented by:

- raw `chat_history` records for precise retrieval
- `story_event` scene summaries for compact narrative continuity
- `character_memory` records for durable facts and relationships

This lets Sujiu eventually compact long sessions without losing the ability to
retrieve an exact old event.

`Transcript::compact` moves the oldest turns into a `CompactedTurns` record and
replaces them with a summary, so the exact turns stay retrievable through the
context protocol after they leave the prompt.

## A turn is many steps, not one message

A turn that used tools is:

```text
U1 -> A1(tool_call T1) -> R1 -> A2(tool_call T2) -> R2 -> A3(final)
```

The final answer is the last assistant step, not the only assistant content of
the turn.

A tool call and its tool result are one nested record, so an unpaired call
cannot be represented. A call the runtime never got to run is stored with an
explicit state (`Completed`, `Failed`, `Interrupted`, `Cancelled`) plus an
explanatory result, so cancelling a turn never leaves a dangling call.

History is append-only apart from explicit compaction, and already-sent history
is never rewritten. That keeps prompt-cache prefixes intact and keeps
call/result correspondence intact.

A tool loop that stops early — cancelled, out of rounds, or cut short by a
provider error — still commits the rounds that finished, so the calls the model
has already seen keep their results on the next request.

Deferred discovery has a cache cost: when a `sujiu_search_tools` result makes
more schemas visible, the tool definition block changes and the cache is
invalidated from that point. That break is deliberate, and tool order is
otherwise stable.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full three-layer contract: model
transcript, UI projection, and provider continuation state.

## External tools and MCP

Sujiu's internal contract is not a claim that Sujiu itself is an MCP server.
It is shaped this way so built-in Rust tools and external MCP tools can share a
registry with minimal translation.

External MCP metadata that cannot be represented by a provider should remain
in the Rust tool layer rather than leak into platform UI code.

## Stability rule

Adding a new database, plugin, lore format or memory algorithm should first ask:

> Can this be represented as another ContextSource / ContextRecord behind the
> existing list/search/read protocol?

If yes, do that.

New model-visible tools are reserved for genuinely new **actions or
capabilities**, not merely new categories of readable information.
