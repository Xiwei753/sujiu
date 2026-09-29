# Sujiu

Sujiu is a multi-platform AI role-playing client built around a small shared Rust core and thin native frontends.

## Goals

- Native frontends for desktop, Android and HarmonyOS.
- Shared Rust domain logic for characters, world books, sessions and prompt compilation.
- Provider-specific adapters stay outside UI code.
- SillyTavern ecosystem compatibility is implemented as import/export codecs, not as the internal data model.
- Platform UI state, rendering and animation remain platform-owned.

## Repository layout

```text
apps/
  desktop/       Qt/QML shell
  android/       Kotlin/Android shell
  harmony/       ArkTS/ArkUI shell
crates/
  sujiu-core/    Pure Rust domain model and prompt compiler
  sujiu-ffi/     Stable cross-language boundary
docs/
  ARCHITECTURE.md
```

## Core

```bash
cargo test --workspace
```

The platform projects are intentionally thin at this stage. Their build-system glue and generated SDK files will be added per-platform instead of committing large IDE-generated trees before the API boundary settles.

## License

AGPL-3.0. See [LICENSE](LICENSE).
