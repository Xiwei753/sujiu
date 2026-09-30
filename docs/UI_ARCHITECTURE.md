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

- open the history drawer;
- current character selector;
- current model selector;
- overflow menu (context & sources, character detail, settings).

The model selector is a sheet/menu showing the current model plus the small set
of necessary facts (name, capability/status, provider). Provider credentials and
full provider configuration live in settings — never in the chat page.

The character selector uses the same pattern: current character, sheet, recents
first, search, and an entry into the full character library.

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

### 1.7 Character library and detail

The library is a secondary page: search, recents, favourites (later), import
character card, create character. Prefer media/content-card or compact grid
layouts over a contacts list (no forced avatar + two-line + chevron rows).

Character detail is ordinary hierarchical navigation: basics, character
definition, greeting, world book, model/preset overrides, memory settings,
import/export. No game-progression dashboard.

### 1.8 Settings

Standard system settings pattern with sections such as:

- Provider / API
- Default model
- MCP / Tools
- Data & backup
- Appearance
- Privacy / permissions
- About

### 1.9 Wide screen

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
Application Bridge ──→ Rust (sujiu-ffi)
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

Connects the presentation layer to `sujiu-ffi` and translates Rust events into
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

The user chooses no vendor and no protocol. The runtime advertises the protocols
it can speak, in the order it prefers them, and negotiation settles which one an
endpoint actually gets. A hostname may produce a display label and nothing else.

#### Persistence

Session, history and catalog semantics belong to the runtime, so the persisted
document belongs to the runtime. A platform supplies only a **location**:

```text
FileService.dataDirectory(context)   // platform capability, reads no app data
  -> bridge.useDataDirectory(path)   // optional; false when there is none
      -> runtime persists its own document there
```

The runtime decides the format and when to write, writes atomically, and never
picks a path itself. A bridge without storage is not a broken bridge, so the
contract method is optional and reports whether a directory was attached.

### 2.5 Dependency direction

```text
ui → presentation → bridge → sujiu-ffi
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

## 4. Bridge contract required by the frontends

The frontends need a coarse, provider-neutral conversation API. The current
`sujiu-ffi` surface is **not sufficient** — see [§5](#5-current-ffi-gap-analysis).

Required data (provider-neutral, stable IDs, no provider wire format):

```text
SessionSummary   id, title, character_id, character_name, preview, updated_at_ms, message_count
CharacterSummary id, name, description
ModelSummary     id, name, provider_label, capabilities, available
ContextSourceSummary id, kind, label, record_count, last_used_at_ms
ConversationSnapshot  messages + per-turn tool/context records
```

Required operations:

```text
list_sessions()                 list_characters(query)
list_models()                   list_context_sources(session_id)
conversation_state(session_id)  send_turn(session_id, input) -> TurnEvent stream
cancel_turn(session_id)         create_session(character_id, model_id, title)
```

Rules:

- add fields compatibly; do not churn the model-visible schema;
- expose coarse application events (§3), not provider events;
- keep presentation-only concerns out of the boundary;
- do not require each frontend to parse provider-specific tool-call JSON;
- "new chat" is a runtime session, not an empty page state: the session must
  exist before the first message is sent, so the UI never has to invent a
  placeholder session id.

---

## 5. FFI surface

`sujiu-ffi` owns the runtime state and exposes a coarse C ABI. `sujiu-napi`
re-exports the same surface as a NAPI module so a platform can call it without
C glue. The frontend never sees this layer; it only sees §4.

```text
sujiu_core_version()
sujiu_compile_prompt_json(input_json)
sujiu_string_free(value)

sujiu_runtime_new() / sujiu_runtime_free(runtime)
sujiu_configure_provider_json(runtime, config_json)
sujiu_list_sessions_json(runtime)
sujiu_list_characters_json(runtime, query)
sujiu_list_models_json(runtime)
sujiu_list_context_sources_json(runtime)
sujiu_conversation_state_json(runtime, session_id)
sujiu_create_session_json(runtime, character_id)
sujiu_send_turn_json(runtime, request_json)
sujiu_send_turn_streaming(runtime, request_json, callback, user_data)
sujiu_cancel_turn(runtime)
```

Every `*_json` entry point returns the same envelope, `{"ok":…,"data":…,"error":…}`,
and a null pointer becomes an error envelope rather than a crash, so a frontend
can trust the shape without knowing what failed.

### 5.1 Turn events cross the boundary already normalized

`send_turn_streaming` reports application events, not provider events:

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

### 5.2 What is still missing

| Need | Status | Consequence |
| --- | --- | --- |
| credential storage | intentionally **not** in Rust | owned by `CredentialService`; until it exists a turn without a key ends in a structured `missing_credential` failure |
| persistence | in-memory catalog | sessions and messages do not survive a restart |
| imported character cards | codec only | the character library has no import yet |
| long-term memory writes | not started | see AGENTS.md §11 |

### 5.3 Per-platform status

HarmonyOS talks to the real runtime today: `SujiuNativeBridge` imports the NAPI
module and implements §4 on top of it, and the preview bridge is gone. The
native library is cross compiled and staged by
`scripts/build-harmony-runtime.sh`.

Android and Desktop still ship preview bridges behind the same interface, so
their presentation code is already written against §4. Replacing them with the
FFI-backed implementation must not change any UI or presentation file.

---

## 6. Directories

The naming may follow platform best practice, but the responsibilities must
match §2.

```text
apps/android/app/src/main/java/io/sujiu/app/
  ui/            Compose screens, components, theme
  presentation/  view state, intents, view models
  bridge/        sujiu-ffi boundary (preview implementation for now)
  platform/      capability services (clipboard/appearance/platform info today)
  MainActivity   composition root (AppGraph)

apps/harmony/entry/src/main/ets/
  pages/         navigation destinations
  components/    reusable ArkUI components
  presentation/  observable view state + controllers
  bridge/        sujiu-ffi boundary; SujiuNativeBridge talks to the real runtime
  platform/      capability services (clipboard/appearance/platform info today)
  app/           composition root (AppGraph)

apps/desktop/
  qml/           Qt Quick views
  src/presentation/  C++ controllers exposing view state
  src/bridge/        sujiu-ffi boundary + preview implementation
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
- pixel-level parity between the three platforms.

Accent colour is reserved for selection, primary actions, state and a very small
amount of emphasis. Content over decoration.

---

## 8. Review checklist for UI changes

- [ ] The change is in the correct layer (`ui`, `presentation`, `bridge`, `platform`).
- [ ] No new platform capability was added inside a page.
- [ ] No provider-specific field or event format leaked into presentation/UI.
- [ ] Streaming states are handled, including the tool round and cancellation.
- [ ] New readable data reuses the context protocol instead of a new tool.
- [ ] Wide layout is a layout promotion of the same state, not a second logic path.
- [ ] Tests cover the new presentation behaviour where behaviour is non-trivial.
- [ ] `cargo fmt --all -- --check` and `cargo test --workspace` pass.
- [ ] Documentation matches the actual directory layout and responsibilities.
