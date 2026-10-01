# UI Architecture

This document fixes the shared **information architecture** and the **platform layer
boundaries** for all Sujiu frontends. It is the reference for
`apps/android`, `apps/harmony` and `apps/desktop`.

It complements:

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — ownership of Rust vs. platform code
- [`TOOLS.md`](TOOLS.md) — tool and context protocol
- [`../AGENTS.md`](../AGENTS.md) — contribution rules

Sujiu is a **modern AI chat app first** and a role-play client second. The UI
reuses interaction patterns that modern AI clients have already validated, and it
does **not** invent a separate "tavern-style" home screen.

The three frontends unify **behaviour, navigation semantics, view state and the
FFI data model**. They do **not** unify pixels:

| Platform | UI system | Navigation idiom |
| --- | --- | --- |
| Android | Jetpack Compose + Material 3 (Material 3 adaptive) | Drawer, bottom sheet, Navigation 3 |
| HarmonyOS | ArkTS / ArkUI | `Navigation` + `NavDestination`, bind sheets, side bar container |
| Desktop | Qt Quick Controls | Adaptive row: drawer, or permanent side bar + optional inspector |

---

## 1. Information architecture

### 1.1 Narrow screen

The app starts on the **conversation canvas**, not on a character home or a
contact list.

```text
Top bar
Conversation
Composer
```

There is no permanent bottom tab bar and no permanent dashboard.

### 1.2 Top bar

Lightweight, single row:

- open the history drawer (narrow layouts only);
- the conversation title, which opens this conversation's own contents;
- the current model selector;
- overflow menu (context & sources, this conversation's contents, new chat);
- **two fixed entries at the right: library, then settings — the gear is
  rightmost and stays rightmost.**

The two fixed entries exist because they are the two places the whole app can
go, and neither of them is a property of the current conversation. Everything
that *is* a property of the current conversation stays in the conversation's own
surface instead of competing for a top-bar slot, and the overflow menu carries
them. A third top-bar button for a resource kind would be a per-resource-type
dashboard in miniature.

Both entries are rendered by one shared definition so they cannot drift into two
different-looking buttons. The library uses its own icon; settings uses a gear.

The model selector is a sheet/menu showing the current model plus the small set
of necessary facts (name, capability/status, provider). Provider credentials and
full provider configuration live in settings — never in the chat page.

### 1.3 Conversation

The conversation owns the majority of the screen.

- Assistant text is rendered as body content, not as heavy bubbles.
- User turns use a light background block for separation.
- Avatars are not repeated per message.
- Tool calls, context retrieval and thinking are **collapsed by default** and
  expand on tap.
- Message actions live behind long-press / an overflow menu, not as a permanent
  button row under every message.

### 1.4 Composer

```text
+    type a message                        Send
```

`+` opens the attachment/capability sheet (images/files, explicitly enabled
tools, later input extensions). No permanent row of feature buttons above the
input.

### 1.5 History

History is a **list**, so it is presented as one — but it is not the main
surface.

```text
New chat

Today
  session
  session

Yesterday
  session
```

Narrow screen: drawer. Wide screen: the same list becomes a permanent side bar.

### 1.6 Context & sources

Context transparency is a lightweight sheet opened from the chat page, never a
dashboard:

```text
This turn

Character            included
World book           3
Story memory         1
Old chat             2
Tool calls           1

View details
```

This screen maps to existing Rust data: `ContextSource`, `ContextRecord`,
`list_context_sources`, `search_context`, `read_context` and tool call records.
It exists for transparency and debugging; it is not a required step per turn.

### 1.7 Library, and what a conversation actually uses

Rust already splits these entities, and the UI follows that split instead of
merging them back together for convenience:

| Rust entity | Library section | User-facing name |
| --- | --- | --- |
| `Character` | 角色 / Characters | 角色 |
| `Persona` | Persona / 身份 | Persona |
| `WorldBook` | 世界书 | 世界书 |
| `PromptProfile` | 提示词 | 提示词 |

The library answers one question: **what resources do I have?** It is a
secondary page reached from the top-bar entry, and it opens with four sections
rather than one combined list, because a combined list has to invent a
category column and a combined row has to hide four different kinds of
detail behind one summary line.

Each section is a list with new / import / edit / delete / view, and each row
opens an ordinary editor for that resource's own fields:

- **角色** — name, description, personality, scenario, first message,
  alternate greetings, example dialogue, system prompt, post-history
  instructions.
- **Persona** — name, description, user prompt, its own world book list. This is
  the user's own side of a role-play; it must not stay hidden inside a
  character, because a persona is reusable across characters and conversations.
- **世界书** — entries with name, content, keywords, enabled/constant flags and
  position. A world book is an independent resource that many characters,
  personas and conversations may reference at once.
- **提示词** — system prompt, user prompt, post-history instructions, format
  rules, and the other fixed prompt segments. It is named 提示词 in the UI; the
  internal Core type name is not user-facing vocabulary.

The library never edits a conversation. A resource that this conversation
happens to use is not owned by it.

Character-card import formats, an advanced world-book editor and prompt
templates are later work; the import control states that rather than being a
dead button.

### 1.8 What this conversation uses

A conversation only ever stores **references** — `participants`, `persona_id`,
`worldbook_ids`, `prompt_profile_id` — and picks them from the same library
resources. It has its own surface (the conversation title, or "this
conversation" in the overflow menu) for choosing participants, persona, world
books and prompt profile.

This boundary is the important one. If the library is where a world book is
edited, and the conversation is where a copy of that world book is edited, then
editing a conversation quietly forks a resource and the next conversation using
the original shows the old text. Resource management and conversation binding
are therefore separate screens with separate ownership, and the same resource is
reused by as many conversations as want it.

Character detail is ordinary hierarchical navigation into the character editor.
No game-progression dashboard.

### 1.9 Settings

Settings is **app and service configuration only**:

- Provider / API (endpoint address, credential, model selection, re-probe)
- Appearance
- Diagnostics log
- Data & backup
- About

It deliberately does **not** contain character editing, persona editing, world
book editing, prompt body editing, or the current conversation's bindings. A
user who opens settings to change their endpoint must never be one edit away from
rewriting a character's system prompt, and a prompt body that lives in settings
belongs to the app rather than to a reusable conversation resource.

### 1.10 Wide screen

Wide layout is a **layout promotion of the same information architecture**, not a
second application:

```text
Sidebar | Conversation | optional inspector
```

- narrow drawer → permanent side bar;
- context sheet → optional right inspector;
- the same presentation state backs both;
- business logic is never duplicated per form factor.

---

## 2. Platform layers

Every frontend is split into the same four layers:

```text
UI / View
    ↓ intents        ↑ view state
Presentation / ViewModel / Controller
    ↓                     ↑
Application Bridge ──→ Rust (sujiu-runtime)
    ↓
Platform Services
    ↓
OS / Framework APIs
```

### 2.1 UI / View

Responsible for layout, components, gestures, rendering, animation and visual
state, and nothing else.

UI **must not**:

- request permissions;
- read/write Keychain, Keystore or credential storage;
- open the file system;
- invoke the system share sheet;
- post notifications;
- own application lifecycle;
- build provider HTTP requests;
- execute tools;
- touch raw FFI pointers/ABI.

#### Reactive binding

The UI layer binds presentation state to the platform's own reactive system and
owns that choice. It is a UI concern, not a presentation one: presentation must
not know about `@State`, `@ObjectLink`, `StateFlow`, `mutableStateOf`,
`Q_PROPERTY` or any other framework binding primitive.

Two rules follow, and both are easy to get wrong:

- Bind the observed object the page actually renders from, not the container
  that happens to hold it. ArkUI observes the first level of a `@State` value,
  so a page that reaches through a composition root to `graph.controller.x`
  renders once and then goes stale. Bind `controller` itself, or pass it into a
  child component as `@ObjectLink`.
- Ephemeral visual state stays in the view. Sheet and drawer *visibility* is
  local view state; the *data* they show is presentation state.

### 2.2 Presentation / ViewModel / Controller

Turns UI intents into application behaviour and owns **page state**:

- whether a reply is generating;
- the display data of the current character/model;
- drawer, sheet, menu and inspector data;
- composer submission state;
- inline page errors;
- which context/tool section is expanded.

Presentation state is **frontend state**. Rust never holds:

- navigation;
- animation progress;
- scroll position;
- input focus;
- cursor position;
- window state.

Sheet, drawer and disclosure **visibility** is local view state: the view may
hide a panel without telling anyone. What a panel *contains* is presentation
state, so a panel is never a place where the UI fetches or transforms data.

### 2.3 Application Bridge

Connects the presentation layer to `sujiu-runtime` and translates Rust events into
provider-neutral presentation data. It contains **no visual logic** and no
provider wire formats.

### 2.4 Platform Services

Every platform capability lives in a dedicated service that knows nothing about
a specific page:

```text
CredentialService          FileService
ShareService               PermissionService
LifecycleService           NotificationService
ClipboardService           SystemAppearanceService
PlatformInfoService
BackgroundHoldService
```

Services expose capabilities (`CredentialService.saveSecret`,
`FileService.pickCharacterCard`, `ShareService.shareFile`,
`SystemAppearanceService.currentAppearance`) — never page verbs
(`SettingsPage.saveApiKey`, `CharacterPage.openFilePicker`,
`ChatPage.shareMessage`).

The test for a new capability: **rewriting a page layout must not require
touching credentials, files, permissions, sharing, notifications or lifecycle
code.**

#### Endpoint configuration and credentials

An endpoint entry is application data, so it crosses the bridge:

```text
UI settings form (base URL + key, no model yet)
  -> presentation.exploreEndpoint(baseUrl, key)
      -> bridge.discoverEndpoint(baseUrl, key)   // writes nothing
  -> user picks a listed model, or types one
  -> presentation.saveProvider(draft, secret)
      -> bridge.configureProvider(draft)         // runtime validates the address
      -> CredentialService.saveSecret(secret)    // platform storage
      -> bridge.listModels()                     // now non-empty
```

**Asking comes before saving.** The model is the one choice a machine cannot
make for the user, and it is also the one thing nobody can know before the
endpoint has been asked. So the settings screen offers "ask this endpoint" from
a form that holds nothing but an address and a key, and discovery writes nothing
to disk. Saving a model is a separate, later act.

A listing failure never blocks that act: every outcome of discovery — no listing
route, no permission, rate limited, unreachable — leaves manual model entry
available, and the screen says which one happened rather than showing an empty
list.

A credential is not page state, not presentation state and not a field on an
endpoint entry. It is a secret that:

- is written through `CredentialService` and nowhere else;
- is read immediately before a turn and handed to the bridge per turn, so the
  bridge holds it for the duration of one request and does not persist it;
- never appears in a `ProviderDraft`, a UI state object, a log line, or a
  bridge event.

The form opens on what is already configured. A settings screen that starts empty
says "nothing is set up" to someone who did set it up, and the most likely
response is to type over a working endpoint. The stored address and model are
read back and drawn into the form; the credential is not, because it was never
page state to begin with and there is nothing to draw it from. An address or a
model that is genuinely absent is drawn as an empty field, which is different
from not having asked.

The user chooses no vendor and no protocol. The runtime advertises the protocols
it can speak, in the order it prefers them, and negotiation settles which one an
endpoint actually gets. A hostname may produce a display label and nothing else.

**The field order is the order the work happens in.** The settings form is
address, then key, then model — not address, model, key. A model in the middle of
that form was a model the user had to know before anybody had asked the endpoint,
which is the same problem the previous "ask this endpoint" button was created to
remove. Putting the key second is deliberate too: it is the last thing the
runtime needs before it can start working, and the field the user is most likely
to reach for once the address is entered.

The model field stays editable in **every** state, including after a failed probe.
It is not a field that unlocks on success. A server that chats perfectly well and
lists nothing is a server the user must still be able to configure, and a field
that greys out on a listing failure turns a cosmetic problem into a dead end.

#### What the settings screen has to be able to say

A screen that reports every problem as "探测失败" cannot be acted on. The runtime
returns a `status` from negotiation and a `listing` from discovery, and the
presentation layer turns those into an `EndpointState` the UI renders one line
for:

| state | the user is told |
| --- | --- |
| `NeedAddress` | no address yet |
| `NeedKey` | no key yet |
| `Ready` | address and key are in, ask when ready |
| `Probing` | asking now |
| `Usable` | a protocol answered |
| `ModelsFound` | N models found, pick one or type a name |
| `ListingUnavailable` | the protocol works, the model list could not be read |
| `AuthFailed` | the key was refused |
| `NoSuchEndpoint` | the address or path is wrong |
| `NetworkError` | nothing answered |
| `ServerError` | the service returned something wrong |
| `RateLimited` | try again later |
| `NoUsableProtocol` | this build speaks none of what the endpoint has |
| `Undetermined` | the runtime declined to conclude — not an error |
| `Unreadable` | the answer could not be read |

Two of these are load-bearing and easy to get wrong. `ListingUnavailable` is a
**success** state, not a failure: the endpoint can chat, it just will not list, and
presenting it as an error sends the user to fix something that is not broken.
`Undetermined` is not a failure at all and must not be coloured as one.

The state is **computed live from the form, not cached**. A cached copy is stale
the moment the user types, and a settings screen that lags its own input is worse
than one that shows nothing.

#### Reading the diagnostic log

The settings screen carries a collapsible diagnostics section showing
`controller.diagnostics`, newest last, with discovery lines and chat lines in
different weights so the two halves are separable by eye as well as by filter. It
offers copy — so a user can paste a trace into a bug report without a
screenshot — and clear.

That section is the reason the log is worth having. A user who says "it does not
work" cannot be asked to reproduce anything; they can only be asked to press
"ask again" and paste what came out. The copy of a trace is the diagnostic.

The log is rendered, not re-derived. Each `DiagnosticEntry` arrives from the
runtime already redacted and already formatted into a single `detail` line, so a
platform cannot leak a key by being careless with what it was handed, and cannot
produce a different reading of the same trace from another one.

#### Persistence

Conversation, history and catalog semantics belong to the runtime, so the
persisted documents belong to the runtime. A platform supplies only a
**location**:

```text
FileService.dataDirectory(context)   // platform capability, reads no app data
  -> bridge.useDataDirectory(path)   // optional; false when there is none
      -> runtime persists its own document there
```

The runtime decides the format and when to write, writes atomically, and never
picks a path itself. A bridge without storage is not a broken bridge, so the
contract method is optional and reports whether a directory was attached.

The layout inside that directory is rooted at the conversation:

```text
sujiu-library.json                       manifest: generation, ids, endpoint, stored context
generations/gen-<n>/conversations/<conversation-id>/conversation.json
generations/gen-<n>/characters/<character-id>.json
generations/gen-<n>/personas/<persona-id>.json
generations/gen-<n>/worldbooks/<world-book-id>.json
generations/gen-<n>/prompt_profiles/<profile-id>.json
```

A platform must not create, read or interpret these documents. It opens a data
directory and it reads summaries over the bridge.

A `<id>` above is a **logical** id and is never the path spelling. Every one of
them is mapped through a single path-safe rule before it becomes a directory or
a file name, because an id can arrive from an imported card, a world book or a
migrated document and must not be able to choose where it is stored. So a
platform that looks for `characters/some.card.json` on disk is reading a
document the runtime never promised to write, and the ids it should be using are
the ones in the manifest and over the bridge. See
[ARCHITECTURE.md § A domain id is not a path](ARCHITECTURE.md).

#### Surviving the background

**Locking the screen sends the app to the background.** It is not a pause: a
suspended process loses its network access, so a turn that is streaming an answer
dies part-way through when the screen dims. No permission causes this and no
permission prevents it — `ohos.permission.INTERNET` is granted at install and
says nothing about the foreground. The remedy is a **transient task**: a short,
time-limited window in which the app may keep running in order to finish work it
has already started. A long-running task would be the wrong instrument, because
it demands a category that matches the work and a visible notification, and a
chat answer is not that.

The hold is held only while it is needed, and that is decided by who owns what:

- the platform service owns the hold, and wraps the platform API;
- the ability reports **lifecycle** and knows nothing about turns;
- presentation reports **that a turn reached one of its three endings** and
  nothing about the process around it;
- the hold is taken when the app backgrounds while a turn is still running, and
  released the moment that turn ends — however it ends, including a failure and a
  cancellation, because those are the two ways a turn actually stops.

A refused hold is not an error. The platform may decline one, and when it does
the turn continues exactly as before, without the guarantee. Treating a refusal as
a failure would turn a scheduling decision into a broken conversation.

The window is not ours to size. The platform caps a transient task at about three
minutes, and less on a low battery, so the hold is asked for once and given back
when the turn ends. A turn that outlives the window fails the way it would have
without it — and the transcript keeps every step it did finish.

```text
ability.onBackground()  -> graph -> if (controller.busy) hold.acquire()
ability.onForeground()  -> graph -> hold.release()
controller settles      -> graph -> hold.release()
```

### 2.5 Dependency direction

```text
ui → presentation → bridge → sujiu-runtime
ui → platform services
presentation → platform services (through service interfaces only)
platform services must never import ui or presentation
bridge must never import ui
```

### 2.6 User-visible copy

Copy is a **UI-layer** concern, and it is the only layer that produces sentences.

- The UI layer reads copy from platform resources: one string table per locale,
  English in the base table, one translated table per language.
- Presentation exposes **states and codes**, never English text. A tool status is
  `ToolCallStatus.Running`, not "running"; a failure is `errorCode` plus an
  optional runtime detail, not a sentence.
- The bridge passes codes and enum values through. It may carry a human-readable
  detail string for diagnostics, but it must not decide what the user reads.
- Rust returns keys, codes and enums. It must not format display copy for the
  UI, and an untitled session stays untitled rather than being given a name.
- When the runtime already has the right string, it passes it through. A tool
  call is described by the `title` its definition already carries; the runtime
  does not re-derive one from the tool id, because `search_context` would become
  "Search Context" and English casing in the runtime cannot be localized.

The same rule runs the other way. **A platform supplies values, not semantics.**
A settings form that types a base URL, a key and a model hands those three
fields to presentation; it does not also decide a vendor, a protocol, a sampling
`max_tokens` or a `temperature`. Those are endpoint semantics, so they live in
the runtime as defaults that a platform may override through `overrides` but never
has to restate. Otherwise two platforms can configure the same endpoint into two
different conversations.

It also does not decide the model by guessing at it. The form asks the endpoint
what it offers and shows the answer, and the person picks. A form that filled the
model field in from a hostname would be making a protocol decision by name, which
is the same mistake one layer up.

Passing content through unshortened is the same discipline. The runtime hands a
character's description as its author wrote it and does not cut it into a
one-line teaser, because picking the sentence, the length and the ellipsis is a
presentation choice, and a truncation rule in the runtime is also the wrong place
to decide what counts as a sentence in a given language.

The one thing the runtime may do is bound a projection. A session row needs a
line of its last message without the whole message crossing the FFI boundary,
so `SessionSummary.preview` is collapsed and length-bounded on purpose. It
invents nothing, and a platform is free to render or further clip it.

The consequence for translators: they edit one JSON file per locale and never
read ArkTS. The consequence for reviewers: grepping the Rust or presentation
sources for a user-visible sentence is a bug, and so is grepping a view for a
provider default.

---

## 3. Streaming presentation states

Streaming is the primary design constraint, not an optimisation. The
presentation layer must be able to express:

```text
Idle
Submitting
Streaming
WaitingForTool
ExecutingTool
ContinuingAfterTool
Completed
Failed
Cancelled
```

UI never sees provider-specific event shapes. Rust normalizes first, then
emits coarse application events:

| Normalized event | Presentation effect |
| --- | --- |
| `turn_started` | `Submitting` |
| `thinking_delta` | `Streaming`, append to the collapsed thinking block |
| `text_delta` | `Streaming`, append to the streaming assistant message |
| `tool_call_requested` | `WaitingForTool`, insert a collapsed tool row in the queued state |
| `tool_call_started` | `ExecutingTool`, mark the tool row as running |
| `tool_call_finished` | fill the tool row result, `ContinuingAfterTool` |
| `turn_completed` | `Completed` |
| `turn_failed` | `Failed` with a user-safe message |
| `turn_cancelled` | `Cancelled` |

The `kind` strings are the wire contract, spelled exactly as above, and a
platform switches on them **by value**. Renaming a variant on either side
silently breaks the other: an unrecognized kind cannot be distinguished from a
broken one, so a frontend that falls back quietly reports every turn as an
immediate failure. Two rules follow:

- keep the strings in one place per platform and treat an unknown kind as a
  visible contract mismatch, never as a silent no-op;
- pin them in a runtime test, so a rename fails a test instead of a device.

While a tool runs, the UI shows lightweight status and keeps the tool row
collapsed. It must not pretend the model is stuck, and it must not silently
swallow the tool round.

---

## 4. The app-facing API the frontends require

The frontends need a coarse, provider-neutral conversation API. What follows
describes that API and what is still missing — see
[§5](#5-generated-binding-surfaces).

**None of it is written out per platform.** Every record and operation below
exists once in `sujiu-runtime` and reaches Kotlin and ArkTS through a
generator. Read this section to know what the contract *is*; do not use it as
something to transcribe. A platform holds view models reached by converting a
generated record, never a hand-typed copy of these shapes.

Required data (provider-neutral, stable IDs, no provider wire format):

```text
ConversationSummary id, title, participants, character_id, character_name,
                    preview, updated_at_ms, message_count
ParticipantSummary character_id, name, role
CharacterSummary   id, name, description
ModelSummary       id, name, provider_label, capabilities, available
ContextSourceSummary id, kind, label, record_count, last_used_at_ms
ConversationSnapshot  participants, persona_id, worldbook_ids, prompt_profile_id,
                      messages + per-turn tool/context records
```

A conversation is the root entity and holds zero, one or many participants.
`character_id`/`character_name` on the summary are derived from the first
participant, kept so a screen that still shows a single character keeps
working; new code should read `participants`.

Required operations:

```text
list_sessions()                 list_characters(query)
list_models()                   list_context_sources(session_id)
conversation_state(session_id)  send_turn(session_id, input) -> TurnEvent stream
cancel_turn(session_id)         create_conversation(participants, persona_id,
                                                   worldbook_ids, prompt_profile_id)
                               create_session(character_id)  <- shortcut for one participant
                               set_conversation_bindings(session_id, participants,
                                                        persona_id, worldbook_ids,
                                                        prompt_profile_id)
```

Library resources need their own coarse surface too, or the library page can
only ever be a preview of hard-coded data:

```text
list_personas()      persona(id)      save_persona(request)      delete_persona(id)
list_world_books()   save_world_book(request)                    delete_world_book(id)
list_prompt_profiles() prompt_profile(id) save_prompt_profile(request)
                                               delete_prompt_profile(id)
save_character(request)  character(id)  delete_character(id)
```

Two rules govern that surface:

- **a summary row and an editor body are different shapes.** A list row carries
  the fields a row can show; an editor gets the whole resource. Pre-filling an
  editor from a summary starts a blank form over a fully-written resource, and
  saving it destroys everything the summary did not carry.
- **deleting a resource unbinds it.** A delete that leaves a dangling id in a
  conversation or a character's world book list produces a screen that shows the
  binding and then cannot honour it, so delete clears every reference and says
  nothing.

Rules:

- add fields compatibly; do not churn the model-visible schema;
- expose coarse application events (§3), not provider events;
- keep presentation-only concerns out of the boundary;
- do not require each frontend to parse provider-specific tool-call JSON;
- "new chat" is a runtime session, not an empty page state: the session must
  exist before the first message is sent, so the UI never has to invent a
  placeholder session id.

---

## 5. Generated binding surfaces

A platform does not call Rust and does not transcribe Rust. §4 exists once, in
`sujiu-runtime`, and two generators project it:

```text
sujiu-runtime  (records + operations)
   ├─ crates/sujiu-uniffi  →  UniFFI   →  generated Kotlin   (Android)
   └─ crates/sujiu-napi    →  napi-rs  →  generated N-API     (HarmonyOS)
```

The two generators are not a failure to converge. There is no mature UniFFI
target for ArkTS, so HarmonyOS uses napi-rs and Android uses UniFFI, and both
are the right answer for their platform.

### 5.1 Where the generated files live, and who owns them

The two artifacts are deliberately not treated the same way.

**Kotlin is generated at build time and not committed.** A Gradle task builds
`libsujiu_uniffi.so`, runs the generator and adds the result to the source set,
so there is no checked-in file that can be out of date. The contract cannot
drift from Rust because the build overwrites it every time.

**`index.d.ts` is generated and committed.** The ArkTS compiler *does* consume it,
but only once the native module is declared as a local folder package — that
association is what turns the SDK's "module is not verified" warning into real
type checking. Both halves are required and neither works alone:

```json5
// entry/src/main/cpp/types/libsujiu_napi/oh-package.json5
{ "name": "libsujiu_napi.so", "types": "./index.d.ts", "version": "" }
```

```json5
// entry/oh-package.json5
{ "dependencies": { "libsujiu_napi.so": "file:./src/main/cpp/types/libsujiu_napi" } }
```

Without the dependency, every import from the module is `any` and nothing about
the native surface is checked. Because a stale declaration would then be
invisible to the compiler as well, `scripts/check-bindings.sh` regenerates it
beside the committed copy and diffs the two; CI runs that check. The compiler and
the script cover different failures — the compiler catches a bridge call that no
longer matches Rust, the script catches a declaration that was never regenerated.

```bash
scripts/generate-bindings.sh          # both platforms
scripts/generate-bindings.sh kotlin
scripts/generate-bindings.sh arkts
scripts/check-bindings.sh             # fail on drift, change nothing
```

The check regenerates beside the original and compares. A check that repairs
what it is checking reports success for a repository it has already changed.

### 5.2 What a platform is allowed to write

A platform may write view models, and only view models:

```text
generated record  →  platform view model  →  UI
```

`bridge/GeneratedMapping.kt` on Android and the mapping helpers in
`SujiuNativeBridge.ets` are the whole of that conversion layer, and the
conversions over generated enums are exhaustive on purpose: adding a
`TurnEventKind` should fail the build rather than render as "something else".

What a platform may **not** write is a second copy of the native surface. The
`Native*` interfaces that used to sit at the top of `SujiuNativeBridge.ets` —
including a `NativeRuntime` that restated the whole method table — were exactly
that, and they had already drifted: `useDataDirectory` was declared as returning
`Promise<void>` when Rust returns nothing, and `rememberedEndpoint` as returning
`undefined` when napi-rs returns `null`. Both were legal TypeScript that failed
at runtime. They are gone; the file imports the generated `*Dto` types and calls
`SujiuRuntimeBridge` methods directly, so those two mistakes are now compile
errors.

The one local shape that remains is the turn-event payload, because a turn
streams many events and `sendTurn` therefore reports each one as a JSON string
through a callback — there is nothing for the generator to produce a type from.
It is named `RuntimeTurnEventPayload` rather than `Native*` so that it cannot be
mistaken for a generated declaration later.

#### What checks this, and what cannot

Two different failures are involved, and one tool covers neither on its own.

| Failure | Caught by |
|---|---|
| A Rust export changed, declaration never regenerated | `scripts/check-bindings.sh`, in CI |
| The bridge calls something the declaration does not declare | the ArkTS compiler; `scripts/check-arkts-contract.py` in CI |
| Argument types, return types, DTO fields | the ArkTS compiler only |
| The compiler is not reading the declaration at all | `scripts/check-harmony-types.sh` |

The last row is the one that is invisible by construction. If the folder-package
association in §5.1 is removed, the app still builds, every native call is `any`,
and the bridge may disagree with Rust in any way. Nothing in a green build says
so; only the SDK's `is not verified` line does.

CI cannot run the compiler — the Command Line Tools are a 7 GB install — so
`check-bindings.sh` asserts the association statically and checks method
existence, while the type-level half is `scripts/check-harmony-types.sh`, which
AGENTS.md §15.4a puts in the verification order. That split is a compromise
forced by the runner, not a claim that the static check is a type checker: it
cannot see a wrong argument type, and it says so in its own header.

### 5.3 The C ABI in `sujiu-runtime/src/lib.rs`

That file also exports a coarse C ABI of about twenty-five `sujiu_*` functions,
every one of them wrapping the same operations in a
`{"ok":…,"data":…,"error":…}` JSON envelope and handing back a string that the
caller frees with `sujiu_string_free`. A null pointer becomes an error envelope
rather than a crash, and a null `limit` means the whole log.

No platform uses it. `sujiu-napi` and `sujiu-uniffi` both call the Rust API
directly, which is why a method could be added to §4 in this round without
touching a line of it — and why `list_models()` could be missing from one
platform's export while the operation existed in Rust the whole time.

It is therefore a third hand-written copy of the same surface, which is exactly
what AGENTS.md §14 rules 3 and 7 exist to prevent, and it is scheduled to be
removed. Until it is, it is not a route to take: a new binding crate goes
through `sujiu-runtime`, and adding an operation to the C ABI as well would be
the drift this document is about.

### 5.4 Turn events cross the boundary already normalized

A turn reports application events, not provider events. The generated names
differ per platform — Kotlin gets `TURN_STARTED`, the `.d.ts` gets
`turnStarted` — and they are the same event:

```text
turn_started
text_delta
thinking_delta
tool_call_requested     the model asked for a tool
tool_call_started       the runtime is running it
tool_call_finished      the result is back
turn_completed / turn_failed / turn_cancelled
```

`tool_call_requested` is what lets a frontend show *WaitingForTool* while the
model is still streaming, instead of jumping straight to *ExecutingTool*. On
the Rust side it exists because `StreamSink` can observe a tool call in the
SSE stream before the turn finishes.

The agent loop reports through `StreamSink` and takes a `CancelToken`; a
provider that cannot stream still works, because `AiProvider::stream` has a
default body that calls `complete` and emits the result as one delta.

### 5.5 What is still missing

| Need | Status | Consequence |
| --- | --- | --- |
| credential storage | intentionally **not** in Rust | owned by `CredentialService`; until it exists a turn without a key ends in a structured `missing_credential` failure |
| persistence | in-memory catalog | sessions and messages do not survive a restart |
| imported character cards | codec only | the character library has no import yet |
| long-term memory writes | not started | see AGENTS.md §11 |

### 5.6 Per-platform status

HarmonyOS talks to the real runtime today: `SujiuNativeBridge` imports the NAPI
module and implements §4 on top of it, and the preview bridge is gone. The
native library is cross compiled and staged by
`scripts/build-harmony-runtime.sh`.

Android has `UniffiSujiuBridge`, which calls the generated Kotlin interface, so
its binding is generated too. It is not yet the bridge the app is wired to,
because the app still runs without a native library on the classpath; until it
does, `InMemorySujiuBridge` remains the source. The preview bridge builds the
same view models from the same operation set, which is what makes swapping it a
change to one file rather than to the UI.

Desktop still ships a preview bridge behind the same interface, so its
presentation code is already written against §4. Replacing it with the
FFI-backed implementation must not change any UI or presentation file.

---

## 6. Directories

The naming may follow platform best practice, but the responsibilities must
match §2.

```text
apps/android/app/src/main/java/io/sujiu/app/
  ui/            Compose screens, components, theme
  presentation/  view state, intents, view models
  bridge/        sujiu-runtime boundary (preview implementation for now)
  platform/      capability services (clipboard/appearance/platform info today)
  MainActivity   composition root (AppGraph)

apps/harmony/entry/src/main/ets/
  pages/         navigation destinations
  components/    reusable ArkUI components
  presentation/  observable view state + controllers
  bridge/        sujiu-runtime boundary; SujiuNativeBridge talks to the real runtime
  platform/      capability services (clipboard/appearance/platform info today)
  app/           composition root (AppGraph)

apps/desktop/
  qml/           Qt Quick views
  src/presentation/  C++ controllers exposing view state
  src/bridge/        sujiu-runtime boundary + preview implementation
  src/platform/      capability services (clipboard/appearance/platform info today)
  src/main.cpp       composition root
```

The platform catalogue in §2.4 is the menu, not a quota. A capability service is
added when a feature needs it, so the current frontends only ship clipboard,
system appearance and platform info.

---

## 7. Rejected directions

Do not introduce:

- game-launcher style home screens;
- glowing borders, gradient stacks or layered glassmorphism;
- one oversized card per feature;
- a contact list as the home screen;
- permanently visible tool or world-book buttons;
- three-column thinking on narrow screens;
- a re-invented model selector;
- platform capabilities inline in page code;
- pixel-level parity between the three platforms;
- a per-resource-type button on the chat top bar instead of one library entry;
- resource editing, or prompt body editing, inside settings;
- a conversation holding its own copy of a world book or prompt profile.

Accent colour is reserved for selection, primary actions, state and a very small
amount of emphasis. Content over decoration.

---

## 8. Review checklist for UI changes

- [ ] The change is in the correct layer (`ui`, `presentation`, `bridge`, `platform`).
- [ ] No new platform capability was added inside a page.
- [ ] No provider-specific field or event format leaked into presentation/UI.
- [ ] Streaming states are handled, including the tool round and cancellation.
- [ ] New readable data reuses the context protocol instead of a new tool.
- [ ] A resource is edited in the library and only referenced by a conversation.
- [ ] Settings gained no resource editing and no prompt body editing.
- [ ] Wide layout is a layout promotion of the same state, not a second logic path.
- [ ] Tests cover the new presentation behaviour where behaviour is non-trivial.
- [ ] `cargo fmt --all -- --check` and `cargo test --workspace` pass.
- [ ] Documentation matches the actual directory layout and responsibilities.
