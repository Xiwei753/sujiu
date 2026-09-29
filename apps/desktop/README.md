# Desktop frontend

Qt 6 / QML shell for Linux and other desktop targets.

The desktop layer owns windows, keyboard/IME integration, rendering, animation and native file dialogs. It consumes the small C/JSON bridge from `sujiu-ffi`; it must not move QML state machines into Rust.

Bootstrap build:

```bash
cmake -S apps/desktop -B build/desktop
cmake --build build/desktop
```

Rust-library linking is intentionally the next integration step rather than hidden inside this first UI shell.
