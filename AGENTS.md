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
  sujiu-runtime/ Shared runtime and app-facing API
  sujiu-uniffi/  Android UniFFI binding (generated Kotlin)
  sujiu-napi/    HarmonyOS N-API binding (generated types)

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

## 14. Cross-language bindings are generated, never written by hand

The Rust app-facing API is the **single source** of every cross-platform
interface. Platform bindings are build outputs of it.

```text
sujiu-core / sujiu-ai
        ↓
sujiu-runtime          app-facing API, no binding machinery
        ↓
├── sujiu-uniffi       Android: UniFFI → generated Kotlin
└── sujiu-napi         HarmonyOS: napi-rs → generated N-API + index.d.ts
```

Hard rules. Each of these exists because breaking it has already cost a
real bug, not as a matter of taste:

1. **Rust is the only source.** If a platform needs a field, an operation or
   a whole entity, add it to `sujiu-runtime` and export it. The alternative is
   two contracts that answer to no one.

2. **Android uses UniFFI; HarmonyOS uses napi-rs.** There is no mature
   UniFFI target for ArkTS, so napi-rs is the correct route there. Do not
   argue for one generator across both platforms.

3. **Never hand-copy a Rust DTO or API into Kotlin, ArkTS or TypeScript.** A
   `data class CharacterSummary` in Kotlin that was typed out by hand is the
   regression this section exists to prevent. It is not a placeholder and it
   is not faster.

4. **Platform view models are allowed, and are not the contract.** A platform
   may have its own shapes for rendering, reached only by converting from a
   generated record:

   ```text
   GeneratedRecord -> PlatformViewModel
   ```

   A view model that happens to match a Rust record field-for-field is still a
   view model. Keep the conversion exhaustive: a `when` over a generated enum
   should fail to compile when a case is added, not fall through to "other".

5. **Regenerate or verify after touching an export.** Changing a Rust DTO,
   adding a `#[uniffi::export]` or adding a `#[napi]` method means running:

   ```bash
   scripts/generate-bindings.sh
   scripts/check-bindings.sh
   ```

   For HarmonyOS, a green `check-bindings.sh` is necessary but not sufficient:
   it proves the declaration matches Rust, and the compiler still has to be
   reading it. That is `scripts/check-harmony-types.sh` (see §15.4a), and a
   binding is not verified until both have run.

6. **CI must fail on binding drift.** `scripts/check-bindings.sh` regenerates
   beside the committed artifact and diffs it. A check that repairs what it is
   checking reports success for a repository it has already changed. The same
   script also asserts that the HarmonyOS bridge still reads that declaration,
   because a declaration nothing reads is as good as a stale one.

7. **Never add a second hand-written bridge contract to make something run.**
   A preview bridge is acceptable while no native library is linked; it must
   speak view models, name no generated type, and be replaced by the real
   bridge rather than accumulating alongside it.

8. **Do not expose provider internals through a binding.** No wire format, no
   OpenAI/Anthropic request shape, no HTTP status, no internal store document.
   Platforms see Sujiu's domain model: Conversation, Character, Persona,
   WorldBook, PromptProfile, endpoint and model discovery, turn events,
   diagnostics.

Keep FFI small and stable. Prefer versioned, provider-neutral data structures.
Do not expose platform-specific UI state through a binding. Do not require
every frontend to understand provider-specific tool-call JSON. Rust should
normalize the runtime behavior first.

A summary row and an editor body are different records. Prefilling an editor
from a list row starts a blank form over a fully-written resource, and saving
destroys whatever the row did not carry.

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

Keep the Command Line Tools outside the repository. On this machine they are at:

```text
~/.harmony-cli/
```

which contains `arktsdoc`, `bin`, `codelinter`, `emulator`, `hstack`, `hvigor`, `ohpm`, `sdk` and `tool`. The NDK the Rust cross build needs is at
`$DEVECO_CLI_CLT_PATH/sdk/default/openharmony/native/llvm/bin`.

`devecocli` itself is a separate npm global and is **not on the default `PATH`**:

```bash
export PATH="$HOME/.local/npm-global/bin:$PATH"
devecocli --version   # 1.3.4
```

Set the toolchain root explicitly. Do not assume Linux can auto-discover a DevEco Studio installation.

```bash
export DEVECO_CLI_CLT_PATH="$HOME/.harmony-cli"
export PATH="$HOME/.local/npm-global/bin:$DEVECO_CLI_CLT_PATH/bin:$PATH"
```

If the local Command Line Tools layout exposes Node or hdc outside `bin`, add the corresponding installed directories to `PATH` rather than copying binaries into the repository.

Verify the installation before touching project code:

```bash
devecocli --version
devecocli --help
```

If those fail, fix the local toolchain first. Do not edit project source to compensate for a broken CLI installation.

`devecocli check lint` currently reports `Files checked: 0` on this machine, with or
without a path argument. It inspects nothing, so it is **not** a verification step
here and must not be reported as one. `devecocli build` is the real arbiter: it
compiles the ArkTS and fails on a real type error.

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

### 15.4a A green HarmonyOS build can still have checked nothing

This is the one HarmonyOS failure that a build cannot report, so it gets its own step.

The ArkTS compiler reads the generated N-API declaration **only if the native module is declared as a local folder package**. Two files are required, and neither works alone:

```text
apps/harmony/entry/src/main/cpp/types/libsujiu_napi/oh-package.json5
  { "name": "libsujiu_napi.so", "types": "./index.d.ts", "version": "" }

apps/harmony/entry/oh-package.json5
  "dependencies": { "libsujiu_napi.so": "file:./src/main/cpp/types/libsujiu_napi" }
```

Without the `dependencies` entry the app **still builds**. Every import from
`libsujiu_napi.so` is typed as `any`, so the bridge can disagree with Rust in any
way at all and compile anyway. The only evidence is one SDK line reading
`Currently module for 'libsujiu_napi.so' is not verified`, which reads like a
limitation of the SDK rather than a misconfigured project. This repository
shipped exactly that state and concluded the SDK could not consume the
declaration, which was wrong.

Do not accept a bare `devecocli build` as proof that the bridge is type-checked.
Run:

```bash
scripts/check-harmony-types.sh
```

It compiles the app **and** fails when that warning appears, because a green
build with an unverified module proves less than it appears to. It also fails on
a real compile error, and prints the SDK's own wording so the cause is not
guessed at.

`scripts/check-arkts-contract.py` asserts the association statically and runs in
CI, because the Command Line Tools are a 7 GB install a runner does not have. It
covers what needs no compiler — the wiring, that every native call the bridge
makes still exists in the declaration, and that no hand-written `Native*`
contract has come back. Argument types, return types and DTO fields need the real
compiler, which is why `scripts/check-harmony-types.sh` is not optional after
touching `SujiuNativeBridge.ets`.

If the association was just added, remove `entry/oh_modules/` before rebuilding so
ohpm reinstalls the package.

### 15.5 Device connection and run

List devices first:

```bash
devecocli device list
```

#### A real device needs a signature, and the signature must not be committed

`apps/harmony/build-profile.json5` is git-tracked and ships with
`signingConfigs: []`. That is correct: the entry is empty precisely so no
machine's paths or passwords are in the repository. It is also why
`devecocli run` against a real device fails with:

> Target device is a real device, but the artifact for 'entry' is not signed.
> Real devices cannot install unsigned packages.

Generate the signing material once per machine:

```bash
cd apps/harmony
devecocli signature generate --product default
```

**This writes machine-local paths and encrypted passwords straight into
`apps/harmony/build-profile.json5`, which is a tracked file.** It must never be
committed. Before committing anything in this repository, restore it:

```bash
git checkout -- apps/harmony/build-profile.json5
```

Restoring it also re-breaks device installs, so the order is: generate, test on
the device, restore, commit. Do not "fix" the file by committing it, and do not
add it to a commit that was meant to be about something else.

An emulator accepts an unsigned package, so a missing signature only shows up
once a real device is involved — which makes it look like a device problem when
it is a build-configuration one.

For a single connected device, the normal flow is:

```bash
cd apps/harmony
devecocli run --module entry
```

With multiple devices, specify the target returned by `device list`:

```bash
devecocli run --module entry --device <device-or-serial>
```

If wireless HDC must be connected manually, use the `hdc` shipped with the
HarmonyOS toolchain. Note that it is **not** in `$DEVECO_CLI_CLT_PATH/bin`; it is
inside the SDK:

```bash
export PATH="$DEVECO_CLI_CLT_PATH/sdk/default/openharmony/toolchains:$PATH"
hdc tconn <ip>:<port>
devecocli device list
```

Do not hard-code a user's device address into scripts or source.

#### The wireless device on this machine

`devecocli device list` returning nothing does not mean there is no device. It
means nothing has connected yet, and the fix is to connect rather than to fall
back to an emulator — treating "no device" as "no device exists" sends you
down a two-hour emulator path when the real phone is on the same wifi.

The phone enables **wireless debugging** and the debug port is **46857**. That
port does not change; the address does, because the phone takes its address from
the hotspot's DHCP. So the port is the thing worth remembering and the address is
the thing worth looking up.

`hdc list targets` shows nothing until `hdc tconn`, and after that it shows the
`ip:port` pair. On this machine the phone lands on the same /24 as the laptop, so
scanning the subnet for that one port finds it:

```bash
seq 1 254 | xargs -P 64 -I{} bash -c \
  'timeout 1 bash -c "exec 3<>/dev/tcp/192.168.43.{}/46857" 2>/dev/null \
   && echo "192.168.43.{}:46857"'
hdc tconn 192.168.43.19:46857
devecocli device list
```

`devecocli device list` then names the model and gives the serial to pass to
`--device`. A working target ends a real-device run with hvigor reporting
`BUILD SUCCESSFUL`, `App installed successfully`, the ability launch succeeding,
and `Smoke: PASS`.

#### The emulator binary needs a compat symlink on this machine

`devecocli emulator` exits 127 on this host:

```text
~/.harmony-cli/emulator/Emulator: error while loading shared libraries:
libbz2.so.1.0: cannot open shared object file
```

The system libraries are all SONAME `libbz2.so.1`; Huawei's emulator wants
`libbz2.so.1.0`, and no distribution copy with that SONAME exists here. A
symlink outside the repository fixes it:

```bash
mkdir -p "$HOME/.harmony-cli/compat-libs"
ln -sf /usr/local/lib/libbz2.so.1 "$HOME/.harmony-cli/compat-libs/libbz2.so.1.0"
export LD_LIBRARY_PATH="$HOME/.harmony-cli/compat-libs"
```

This is a local toolchain repair, not a project change, so it belongs in the
shell environment and not in a commit. With it, `devecocli emulator list` works.

Prefer the real device anyway. An emulator accepts an unsigned package while a
real device does not, so a device run exercises the signing step that a real
release goes through, and it is the only target that proves the package
installs at all.

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
  -> scripts/check-harmony-runtime.sh     # only when Rust changed
  -> scripts/check-bindings.sh            # only when an #[napi] export changed
  -> scripts/check-harmony-types.sh       # compiles AND proves the compiler reads the declaration
  -> devecocli device list
  -> devecocli run --module entry [--device ...]
  -> exercise the changed UI/flow
  -> inspect devecocli log / crash output
  -> only then report the HarmonyOS path verified
```

`scripts/check-harmony-types.sh` wraps the `devecocli build` that used to be the
third step, and adds the assertion a bare build cannot make — see §15.4a. Running
`devecocli build` directly is still allowed for iterating, but a bare green build
is not evidence that the bridge was type-checked, so it does not stand in for this
step when reporting verification.

The build is the step that proves the ArkTS compiles. `devecocli check lint` is
listed here for completeness but currently inspects nothing on this machine (see
§15.1), so a green lint is not evidence and must not be cited as one.

`scripts/check-harmony-runtime.sh` belongs in the order whenever Rust changed,
and the fix it asks for is `scripts/build-harmony-runtime.sh`. The staged
`libsujiu_napi.so` is a committed build output, so a Rust change that was never
rebuilt reaches a device as an old runtime behind a new bridge. A new NAPI method
the ArkTS calls but the committed `.so` does not export is exactly that failure,
and it compiles cleanly until the call is made at runtime.

**A brand-new Rust file has to be committed before the library is built, or the
fingerprint silently omits it.** `harmony-runtime-fingerprint.sh` hashes only
files `git ls-files` reports, which is deliberate — an untracked build artifact
must not be able to change the record. The cost is that a new `crates/**.rs`
file is invisible to the fingerprint until it is tracked, so building it while
still untracked records a fingerprint that omits the very file you just added.
The local check passes, the device gets a correct library, and **CI fails on the
commit** where the file becomes tracked and the fingerprint moves.

This is not a theory: it is what the first issue-#3 push did, with
`crates/sujiu-ai/src/diagnostics.rs`. The fix is not a rebuild, because a
rebuild alone does not help if the file is still untracked. It is:

```text
git add the new Rust source   ->  scripts/build-harmony-runtime.sh  ->  commit
```

When a commit adds a `.rs` file and CI reports the runtime library stale while
your own check passes, this is why.

If a physical device is unavailable, still perform the build and state clearly
that install/runtime behavior was not verified.

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
- continuation state may be reused raw only when the transport actually supports chaining **and** the full identity matches (protocol, endpoint id, base URL, model). Two gateways can serve the same model name with unrelated state. Otherwise fall back to the normalized model transcript and let the adapter convert
- continuation advances within a turn. Each round continues from the previous round's handle, not from the one the session had before the turn. A provider says whether it replaced the handle, has no opinion, or dropped it; do not encode those three answers in an `Option`
- a continuation answer is **persisted**, including a clear. Storing only the handle makes a later clear degrade to "nothing", and the next turn's lookup then walks past it and resurrects the dead handle. Store the event and stop at the first one a provider ever gave
- state the protocol produced must survive serialization. An unrecognisable stored event is an error, never an empty handle; a legacy shape is read and degraded to something that cannot claim a capability it cannot prove; and a stored document the runtime cannot read is protected from every later save, not only the one that discovered it
- a persisted schema change needs a migration that knows **which** shape it is reading. Reading a transcript through a flat-message reader parses without complaint and returns a session with no turns, which loses a conversation silently. Bump the version and branch on it.
- an assistant message carries visible content, an optional protocol-native sidecar and optional tool calls together. Do not hang the sidecar off one variant of the message: a step that reasoned and then answered with no tool call must keep its reasoning
- assistant reasoning travels with the assistant step and is replayed on the next request when the negotiated protocol needs that. The wire field name is the adapter's, never guessed, and never sent to an endpoint that did not ask for it
- reasoning metadata is kept separate from ordinary visible assistant text, and it carries the provider that produced it. Visible text may cross providers; a reasoning sidecar is replayed only to the identity that produced it
- **a user does not choose a vendor, and a user does not choose a protocol.** The configuration a platform fills in stays endpoint id, base URL, credential reference and an optional chosen model. A hostname may produce a display label and nothing else: a label that changed how a conversation was spoken would be the same mistake wearing a different hat
- protocol capability comes from **negotiating with the endpoint**, never from a vendor name, a hostname or a model name. `reasoner`, `r1` and `thinking` in a model string prove nothing and go stale
- one fixed protocol priority for every endpoint, in one place, never reordered per user choice: Responses, then Chat Completions, then Anthropic Messages. The negotiated protocol picks the adapter
- only evidence that a path does not exist may fall through to the next protocol. 401/403 is a credential problem, 429 is a rate limit, 5xx and timeouts are transient, and a 400 or 404 counts only when it names an unknown endpoint, path or route. A bare "not found" is not that, because "model not found" is what a working endpoint says most often
- a **status code is not evidence on its own**. Gateways that route by model answer an unresolvable model with the same `404 page not found` they use for a path they never had. Distinguish three verdicts: only a body that names a missing route is **unsupported** and worth remembering; a bare 404 is **ambiguous** — walk past it, conclude nothing; credentials, rate limits and transient failures **stop** the walk. Say "we could not tell" rather than "this endpoint speaks none of our protocols": the first is recoverable and the second is a false statement the user will act on
- cache a protocol answer by endpoint id plus normalized base URL, never by a transient one, and forget it when the endpoint is reconfigured
- every provider call carries a connect timeout and a read timeout. A client with neither turns an endpoint that accepts a connection and then goes quiet into a turn that never ends. The read timeout measures the gap between reads, not the length of the response, so a long stream is never cut off; only silence is bounded
- discovery runs before a model is known. A base URL and a key must be enough to explore an endpoint; requiring a saved configuration first puts the choice in front of the question it is chosen from. Exploring writes nothing
- model listing and protocol negotiation are independent questions. A gateway can list nothing and still speak Responses, and can list fifty models and speak only the oldest protocol. Every listing outcome — no route, no permission, rate limited, unreachable — still leaves manual model entry available, and the list never decides the conversation protocol
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
