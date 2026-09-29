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

### 2.5 Dependency direction

```text
ui → presentation → bridge → sujiu-ffi
ui → platform services
presentation → platform services (through service interfaces only)
platform services must never import ui or presentation
bridge must never import ui
```

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
| `text_delta` | `Streaming`, append to the streaming assistant message |
| `tool_call_started` | `WaitingForTool` → `ExecutingTool`, insert collapsed tool row |
| `tool_call_finished` | fill the tool row result, `ContinuingAfterTool` |
| `turn_completed` | `Completed` |
| `turn_failed` | `Failed` with a user-safe message |
| `turn_cancelled` | `Cancelled` |

While a tool runs, the UI shows lightweight status and keeps the tool row
collapsed. It must not pretend the model is stuck, and it must not silently
swallow the tool round.

---

## 4. Bridge contract required by the frontends

The frontends need a coarse, provider-neutral conversation API. The current
`sujiu-ffi` surface is **not sufficient** — see [§5](#5-current-ffi-gap-analysis).

Required data (provider-neutral, stable IDs, no provider wire format):

```text
SessionSummary   id, title, updated_at_ms, message_count
CharacterSummary id, name, avatar_uri, greeting_preview, updated_at_ms
ModelSummary     id, name, provider_label, capabilities, available
ContextSourceSummary id, kind, label, record_count, last_used_at_ms
ConversationSnapshot  messages + per-turn tool/context records
```

Required operations:

```text
list_sessions()                 list_characters(query)
list_models()                   list_context_sources(session_id)
conversation_state(session_id)  send_turn(session_id, input) -> TurnEvent stream
cancel_turn(session_id)
```

Rules:

- add fields compatibly; do not churn the model-visible schema;
- expose coarse application events (§3), not provider events;
- keep presentation-only concerns out of the boundary;
- do not require each frontend to parse provider-specific tool-call JSON.

---

## 5. Current FFI gap analysis

`sujiu-ffi` currently exports:

```text
sujiu_core_version()
sujiu_compile_prompt_json(input_json)
sujiu_string_free(value)
```

That is enough for prompt compilation only. The following are **missing**, and
block the chat shell from using real runtime behaviour:

| Need | Status in `sujiu-ffi` | Consequence |
| --- | --- | --- |
| session/character/model listing | missing | selectors and history cannot be filled from Rust |
| conversation turn entry point | missing | `sendTurn` has no runtime to call |
| streaming events | missing | `Streaming` / `WaitingForTool` cannot be driven by real data |
| tool call records | missing | the context/tool inspector has no source |
| context source/record listing | missing | the context sheet has no source |
| cancellation | missing | `Cancelled` cannot be produced |
| credential storage | intentionally **not** in Rust | owned by `CredentialService` |

`AgentRuntime::run` is also currently **non-streaming**: it awaits one provider
turn, executes tools, and returns a final `AgentOutcome`. A streaming event
source is required before any frontend can render incremental text or a live
tool row.

Until those land, each frontend ships a single bridge implementation that is
sourced from local preview data (`InMemorySujiuBridge` and equivalents) behind
the same interface, so that presentation code and tests are already written
against the contract in §4. Replacing it with the FFI-backed implementation must
not change any UI or presentation file.

---

## 6. Directories

The naming may follow platform best practice, but the responsibilities must
match §2.

```text
apps/android/app/src/main/java/io/sujiu/app/
  ui/            Compose screens, components, theme
  presentation/  view state, intents, view models
  bridge/        sujiu-ffi boundary + preview implementation
  platform/      capability services (clipboard/appearance/platform info today)
  MainActivity   composition root (AppGraph)

apps/harmony/entry/src/main/ets/
  pages/         navigation destinations
  components/    reusable ArkUI components
  presentation/  observable view state + controllers
  bridge/        sujiu-ffi boundary + preview implementation
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
