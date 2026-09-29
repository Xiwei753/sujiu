# Architecture

Sujiu follows one hard rule: **the Rust core owns domain data and deterministic transformations; each platform owns presentation and interaction.**

## Layers

### 1. Platform frontends

- `apps/desktop`: Qt/QML
- `apps/android`: Kotlin/Android
- `apps/harmony`: ArkTS/ArkUI

Frontends own navigation, input methods, rendering, animations, window state and platform storage integration.

### 2. sujiu-core

Pure Rust. No Qt, Android, HarmonyOS or HTTP SDK dependencies.

It owns:

- character data
- world-book data and deterministic keyword matching
- chat/session records
- provider-neutral prompt plans
- prompt compilation
- provider configuration types

It must not own cursor state, UI animation state, widgets, navigation or platform lifecycle.

### 3. sujiu-ffi

A deliberately small boundary around `sujiu-core`. The boundary exchanges versioned JSON envelopes first so all three platforms can integrate before platform-specific generated bindings are chosen.

### 4. Compatibility codecs

SillyTavern/Character Card/World Book support belongs in a codec/adapter layer. External formats are parsed into Sujiu's internal types and exported back out. Unknown external extension fields should be preserved where possible.

The internal model must never become a mirror of SillyTavern's implementation.

## Prompt pipeline

```text
App system prompt
  -> character definition
  -> triggered world-book entries
  -> example dialogue
  -> chat history
  -> post-history instruction
  -> current user input
  -> PromptPlan
  -> provider adapter
```

`priority` means client-side retention/ordering priority. It is not presented as a magic model attention weight.

## Dependency direction

```text
platform UI -> FFI -> sujiu-core
provider runtime -> sujiu-core types
compat codec -> sujiu-core types
```

The core never imports a platform frontend.
