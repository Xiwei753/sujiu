# Desktop frontend

Qt 6 / QML client for Linux and other desktop targets.

The desktop layer owns windows, keyboard/IME integration, rendering, animation
and native file dialogs. It consumes the small C/JSON bridge from `sujiu-ffi`; it
must not move QML state machines into Rust.

Interaction model: the conversation canvas is the home surface, history lives in
a drawer (permanent sidebar on wide windows), model and character selection are
lightweight sheets, tool and thinking activity is collapsed by default, and
settings follow the platform settings pattern. See
[`docs/UI_ARCHITECTURE.md`](../../docs/UI_ARCHITECTURE.md).

## Layers

```text
qml/           QML views: rendering, local disclosure state, navigation wiring
src/bridge/    SujiuBridge contract + preview implementation (→ sujiu-ffi)
src/presentation/ ChatController (QObject): page state, generation state
src/platform/  clipboard, system appearance, platform info capabilities
src/main.cpp   composition root; the only place that picks implementations
```

QML never calls a provider, a tool or a platform API directly. It reads
`Sujiu.App` properties and invokes its methods.

`SujiuBridge` currently has a preview implementation (`InMemorySujiuBridge`)
because `sujiu-ffi` does not expose the conversation surface yet; switching to
the real bridge is a one-line change in `main.cpp`.

## Build

```bash
cmake -S apps/desktop -B build/desktop
cmake --build build/desktop
```

Rust-library linking is intentionally the next integration step rather than
hidden inside this UI shell.
