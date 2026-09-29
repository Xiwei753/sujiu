# HarmonyOS frontend

Native ArkTS / ArkUI client for Sujiu, built as a chat-first app.

HarmonyOS owns ArkUI navigation, text input, window/safe-area behavior,
animations, credential storage and platform file/share capabilities.
Character/world-book/prompt behavior stays in Rust.

Interaction model matches the other frontends: the conversation canvas is the
home surface, history lives in a sheet (permanent sidebar on wide layouts),
model and character selection are lightweight sheets, tool and thinking activity
is collapsed by default, and settings follow the platform settings pattern. See
[`docs/UI_ARCHITECTURE.md`](../../docs/UI_ARCHITECTURE.md).

## Layers

```text
pages/       ArkUI pages (pages/Index is the chat shell and the router host)
components/  reusable views: conversation, composer, history, selector sheets
presentation/ ChatController + Navigator (page state, no I/O)
bridge/      SujiuBridge contract + preview implementation (→ sujiu-ffi)
platform/    clipboard, system appearance, platform info capabilities
app/         composition root; the only place that picks implementations
```

ArkUI views never call a provider, a tool or a platform API directly. They read
`ChatController` state and call its methods.

`SujiuBridge` currently has a preview implementation (`InMemorySujiuBridge`)
because `sujiu-ffi` does not expose the conversation surface yet; switching to
the real bridge is a one-line change in `app/Controller.ets`.

DevEco/Hvigor-generated project metadata and signing configuration are
intentionally not committed here; signing keys/config remain local.
