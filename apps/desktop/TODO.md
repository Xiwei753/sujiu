# Desktop — remaining scope

`docs/UI_ARCHITECTURE.md` is the specification, and §1 applies to this frontend
exactly as it does to HarmonyOS. This file records where the Desktop frontend is
behind it, so "the UI is unified across platforms" is never claimed by accident.

HarmonyOS is the reference implementation. The items below are the port of a
destination, not a redesign.

## Blocking the §1 information architecture

- [ ] **Conversation contents page.** §1.2 makes the conversation title the entry
      to *this conversation's* contents and bindings; §1.8 says that page holds
      the participants / Persona / world book / prompt references. The title is
      currently a `Label` in `ChatTopBar.qml` because there is nothing for it to
      open.
      Needs: a `ConversationContentsPage.qml` with the four reference groups and
      an apply action, plus the write path in `ChatController` — it has no
      `bindResources`, and `SujiuBridge.h` has no persona / world book / prompt
      listing to fill the page from.
- [ ] **Context inspector entry point.** `ContextPanel.qml` is correct and
      currently unreachable: the wide-layout `Pane` in `ChatShell.qml` and the
      removed narrow `Dialog` both hung off the three-dot overflow, and §1.6
      allows exactly one entry — the conversation contents page above. The
      `inspectorVisible` flag is kept so the pane is one line away from working
      again.
- [ ] **Provider & API settings category.** §1.9 lists it, and it is not drawn
      because `ChatController` exposes no endpoint configuration, no protocol
      probe and no model discovery (`app.endpoint()` is read read-only in
      `sendTurn`). The category appears when the bridge does.
- [ ] **Diagnostics settings category.** Same reason: there is no diagnostics log
      in the desktop bridge, and the HarmonyOS diagnostics page is a view of that
      log rather than an independent feature.
- [ ] **Four-resource library split.** §1.7 makes Character, Persona, WorldBook
      and PromptProfile four separate resources.
      `CharacterLibraryPage.qml` / `CharacterDetailPage.qml` are characters only,
      so `Library` opens a single list.
- [ ] **Library management, not conversation selection.** `ChatController` still
      has `selectCharacter()` and `CharacterDetailPage.qml` still calls it, which
      writes the conversation's character. §1.8 says the conversation stores
      references and the library manages resources.

## Copy

- [ ] Add `i18n/*.ts` translation catalogues. Every user-visible string is
      wrapped in `qsTr()`, but with no catalogue `lrelease`/`lupdate` never
      produces a second language, so this frontend cannot render one.
- [ ] `scripts/check-i18n.py` only inspects the HarmonyOS tables; the Qt
      catalogues need their own check before a string can be forgotten in two
      languages at once.

## Visual language

§2.7 says the platform theme owns ordinary controls. `Main.qml` still sets a
hand-written `palette` for window, base, alternateBase, text, placeholderText,
button, buttonText and mid, and `palette.highlight` was removed by this round
precisely because a fixed indigo ignores the desktop theme.

- [ ] Move the remaining palette onto the Qt style's defaults, or onto a
      stylesheet derived from the platform theme, so light and dark stop being
      two hardcoded sets of literals.
- [ ] The five top-bar slots are text labels because Qt Quick Controls ships no
      icon set and this project has no icon resources. If an icon theme is added,
      it replaces the labels — it does not become a second convention.

## Not claimed

Desktop still ships `InMemorySujiuBridge` behind the §4 interface (see §5.6), so
the items above are UI-scope debt against that bridge, not a statement that the
runtime parity exists.
