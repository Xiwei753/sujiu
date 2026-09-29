# Android frontend

Native Kotlin / Jetpack Compose client for Sujiu, built as a chat-first app.

The Android layer owns Android UI, lifecycle, IME behavior, secure credential
storage and rendering. Shared character/world-book/prompt logic stays in Rust.

Interaction model: the conversation canvas is the home surface, history lives in
a drawer (permanent sidebar on wide layouts), model and character selection are
lightweight sheets, tool and thinking activity is collapsed by default, and
settings follow the platform settings pattern. See
[`docs/UI_ARCHITECTURE.md`](../../docs/UI_ARCHITECTURE.md).

## Layers

```text
ui/           Compose screens and components (rendering only)
presentation/ ChatController, ChatUiState, Navigator (page state, no I/O)
bridge/       SujiuBridge contract + preview implementation (→ sujiu-ffi)
platform/     Clipboard / appearance / platform info capabilities
MainActivity  composition root; the only place that picks implementations
```

UI code never talks to the bridge, to a provider, to a tool, or to a platform
API. It renders `ChatUiState` and emits `ChatIntent`.

`SujiuBridge` currently has a preview implementation (`InMemorySujiuBridge`)
because `sujiu-ffi` does not expose the conversation surface yet; switching to
the real bridge is a one-line change in `AppGraph`.

## Build baseline

- Android Gradle Plugin 9.4.x with built-in Kotlin
- Jetpack Compose (BOM-less explicit versions, `buildFeatures.compose = true`)
- Gradle 9.6+ (the repository does not commit a wrapper)
- JDK 17
- compileSdk/targetSdk 36
- minSdk 26

```bash
gradle :app:assembleDebug :app:testDebugUnitTest
```

`app/src/test` holds presentation unit tests (turn lifecycle, cancellation,
failure, selection, clipboard routing, navigation) that run on the JVM.
