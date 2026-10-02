# Android — remaining scope

`docs/UI_ARCHITECTURE.md` is the specification, and §1 applies to this frontend
exactly as it does to HarmonyOS. This file records where the Android frontend is
behind it, so "the UI is unified across platforms" is never claimed by accident.

HarmonyOS is the reference implementation. The items below are the port of a
destination, not a redesign.

## Blocking the §1 information architecture

- [ ] **Conversation contents page.** §1.2 makes the conversation title the entry
      to *this conversation's* contents and bindings; §1.8 says that page holds
      the participants / Persona / world book / prompt references. The title is
      currently a label because there is nothing for it to open.
      Needs: a screen with the four reference groups and an apply action, plus
      the write path in the bridge (`ChatController` has no `bindResources`, and
      `SujiuBridge.ets` has no `listPersonas` / `listWorldBooks` /
      `listPromptProfiles` to fill it from).
- [ ] **Context inspector entry point.** `ui/chat/ContextSheet.kt` is correct and
      currently unreachable: §1.6 allows exactly one entry, and it is the
      conversation contents page above. Wiring it to the top bar instead would
      re-create the second entry point this round removed.
- [ ] **Provider & API settings category.** §1.9 lists it, and it is not drawn
      because `SujiuBridge` exposes no endpoint configuration, no protocol probe
      and no model discovery. The category appears when the bridge does.
- [ ] **Diagnostics settings category.** Same reason: `SujiuBridge` has no
      diagnostics log, and HarmonyOS's diagnostics page is a view of that log
      rather than an independent feature.
- [ ] **Four-resource library split.** §1.7 makes Character, Persona, WorldBook
      and PromptProfile four separate resources.
      `ui/library/CharacterScreens.kt` is characters only, so `Library` currently
      opens a single list. Needs the other three list screens, their editors, and
      `listPersonas` / `listWorldBooks` / `listPromptProfiles` /
      `bindResources` in the bridge.
- [ ] **Library management, not conversation selection.** `CharacterDetailScreen`
      still offers "Chat with this character", which writes the conversation's
      character. §1.8 says the conversation stores references and the library
      manages resources; the entry point is the contents page.

## Copy

`res/values/strings.xml` was added by this round and covers the chat top bar and
the settings pages. It is the first resource file this frontend has had — every
other screen still has Kotlin string literals, which is why the Android UI can
only be read in English.

- [ ] Migrate `HistoryPanel.kt`, `Composer.kt`, `ConversationList.kt`,
      `MessageItemView`, `ModelSelectorSheet.kt`, `ContextSheet.kt` and
      `CharacterScreens.kt` to `stringResource`.
- [ ] Add `values-zh-rCN/strings.xml`. Nothing in this frontend can currently
      render a second language, which is the failure `scripts/check-i18n.sh`
      exists to prevent on the HarmonyOS side.
- [ ] `scripts/check-i18n.py` only inspects the HarmonyOS tables. Extending it to
      the Android resource table is what stops this file growing again.

## Not claimed

Android does **not** follow the HarmonyOS conversation, retrieval or provider
behaviour yet; it runs against `InMemorySujiuBridge` and has no native library
on its classpath. See §5.6. The items above are UI-scope debt against that
bridge, not a statement that the runtime parity exists.
