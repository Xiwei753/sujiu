import { common } from '@kit.AbilityKit';
import { KeyboardAvoidMode } from '@kit.ArkUI';
import {
  DiagnosticEntry,
  ModelSummary,
  ProviderDraft,
  StoredEndpoint
} from '../bridge/SujiuBridge';
import { AppGraph } from '../app/Controller';
import { Composer } from '../components/Composer';
import { ConversationContents } from '../components/ConversationContents';
import { ConversationView } from '../components/ConversationView';
import { SafeArea } from '../ui/SafeArea';
import {
  CharacterEditor,
  PersonaEditor,
  PromptEditor,
  WorldBookEditor
} from '../components/LibraryEditors';
import {
  CharacterLibrary,
  LibraryHub,
  PageHeader,
  PersonaLibrary,
  PromptLibrary,
  WorldBookLibrary
} from '../components/Library';
import { HistoryPanel } from '../components/HistoryPanel';
import { ContextSheet, ModelSelectorSheet } from '../components/SelectorSheets';
import { AppearanceMode } from '../platform/PlatformServices';
import { ChatController, EndpointState } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
import { AppTheme } from '../ui/theme/Theme';

/**
 * Bottom sheets the chat surface can open.
 *
 * There is no overflow entry here, and that is the point rather than an
 * omission. The top bar's five slots are the whole navigation of this surface,
 * and a fourth kind here would be a place for a screen that has nowhere to be.
 */
enum SheetKind {
  None = 'none',
  Models = 'models',
  Context = 'context'
}

/**
 * Route names for ArkUI `Navigation`; the stack itself is platform owned.
 *
 * The library is a hub with four lists under it rather than four top-bar
 * entries, which is the navigation half of the split: a main screen with one
 * button per resource kind grows a button per kind forever and never says what
 * any of them is for.
 */
enum RouteName {
  Library = 'Library',
  Characters = 'Characters',
  CharacterEdit = 'CharacterEdit',
  Personas = 'Personas',
  PersonaEdit = 'PersonaEdit',
  WorldBooks = 'WorldBooks',
  WorldBookEdit = 'WorldBookEdit',
  Prompts = 'Prompts',
  PromptEdit = 'PromptEdit',
  ConversationContents = 'ConversationContents',
  Settings = 'Settings',
  /**
   * Settings has pages of its own, not one long form.
   *
   * Each category is a route rather than a section of one screen, because a
   * category is a thing the user chooses to go and read. Drawing them all at
   * once on one scrollable page is what turned a settings screen into a console:
   * the endpoint probe, the protocol status, eighty-one models and a log viewer
   * were all on the same surface as the appearance choice, so none of them read
   * as a category.
   */
  SettingsProvider = 'SettingsProvider',
  SettingsAppearance = 'SettingsAppearance',
  SettingsDiagnostics = 'SettingsDiagnostics',
  SettingsAbout = 'SettingsAbout'
}

/** Width at which history becomes a permanent sidebar (layout promotion). */
const EXPANDED_WIDTH: number = 840;

const APPEARANCE_MODES: AppearanceMode[] = [
  AppearanceMode.System,
  AppearanceMode.Light,
  AppearanceMode.Dark
];

/**
 * Radio group name for the appearance choice.
 *
 * Named so the three rows are one exclusive set. Without it each radio is its
 * own group and all three can end up ticked at once, which is a state no screen
 * should be able to reach.
 */
const APPEARANCE_GROUP: string = 'sujiu_appearance';

/**
 * Chat is the home surface and the root `NavDestination`.
 *
 * History is a `SideBarContainer` drawer on narrow windows and a permanent
 * sidebar on expanded windows: one component, layout promotion, not two apps.
 * Secondary pages are pushed onto ArkUI's `NavPathStack`, so the system back
 * key, back gesture and page lifecycle are the platform's.
 */
@Entry
@Component
struct Index {
  // Composition root. It stays a plain field: only the two observed objects the
  // page renders from are state, so ArkUI observes their properties directly
  // instead of the page reaching two levels deep through the graph.
  private graph: AppGraph = new AppGraph();
  @State private controller: ChatController = this.graph.controller;
  @State private theme: AppTheme = this.graph.theme;
  @State private wideLayout: boolean = false;
  @State private showSheet: boolean = false;
  @State private sheet: SheetKind = SheetKind.None;
  @State private drawerOpen: boolean = false;
  // Provider form: unsaved input is view state, the saved config is not.
  @State private providerBaseUrl: string = '';
  @State private providerModel: string = '';
  @State private providerKey: string = '';
  /**
   * Bumped only when something other than a keystroke changes a field, so the
   * field can be rebuilt to show it. See providerField.
   */
  @State private providerFieldEpoch: number = 0;
  /** True right after a successful copy, so the user knows the paste is safe. */
  @State private diagnosticsCopied: boolean = false;

  /**
   * Whether the discovered-model list is open.
   *
   * The list is a bounded scroll area rather than chips scattered down the
   * form, for one concrete reason: an endpoint that lists eighty-one models
   * pushed the save button off the bottom of the screen, and a form whose one
   * commit control is a scroll away is a form people abandon. Inside a panel
   * of fixed height the list can hold every model the endpoint reported — no
   * cap, no "and 69 more" that a user cannot get at — and the rest of the form
   * stays exactly where it was.
   *
   * It opens by default, because the reason a user presses "ask this endpoint"
   * is to see what came back, and an answer that stays hidden behind a second
   * tap is not the answer they asked for.
   */
  @State private modelListOpen: boolean = true;
  private pathStack: NavPathStack = new NavPathStack();

  aboutToAppear(): void {
    this.graph.bind(this.getUIContext().getHostContext() as common.Context);
    // RESIZE, not the default OFFSET: with OFFSET the IME pushes the whole
    // page up, so the top bar slides under the status bar and the conversation
    // overlaps the system area. Resizing keeps the chat chrome in place.
    this.getUIContext().setKeyboardAvoidMode(KeyboardAvoidMode.RESIZE);
    this.startUp();
  }

  /**
   * Starts the graph, then opens the fields on what it found.
   *
   * The order matters and is not incidental: `loadProvider` reads the stored
   * endpoint across the bridge, so a form drawn before it resolves sees nothing
   * configured and says so to a user who has already configured something.
   */
  private async startUp(): Promise<void> {
    await this.graph.start();
    this.openSettingsOnStoredEndpoint();
  }

  /**
   * Opens the settings fields on what is already configured.
   *
   * Reading it back is not a convenience. An empty form opened on an endpoint
   * that has a model and a conversation behind it says "nothing is set up", and
   * the reasonable response to that is to set it up again, over the top of what
   * is already there.
   *
   * The epoch is bumped because a TextInput's text is its *initial* value: new
   * state alone would leave the fields showing what they showed before. It is
   * not bumped per keystroke, so this cannot rebuild a field under a cursor —
   * a form is not open while somebody is typing in it.
   *
   * The credential is not read back and cannot be. It stays in platform storage
   * and is fetched immediately before a turn.
   */
  private openSettingsOnStoredEndpoint(): void {
    const stored: StoredEndpoint | null = this.controller.storedEndpoint;
    if (stored === null) {
      return;
    }
    this.providerBaseUrl = stored.baseUrl;
    // Assigned unconditionally, including the empty case. Skipping the
    // assignment when there is no stored model leaves whatever the field
    // happened to hold — so an endpoint that was just saved with no model still
    // shows the model it had before, and the user is told a model is
    // configured when it is not. An endpoint with no model is a normal state
    // and is drawn as an empty field, which is different from not having asked.
    this.providerModel = stored.selectedModel ?? '';
    this.providerFieldEpoch += 1;
  }

  /**
   * Back is resolved in the order a chat user expects: an open sheet closes
   * first, then the history drawer, then the pushed destination. Returning
   * false only at the root hands the gesture back to the system, which ends
   * the ability.
   */
  onBackPress(): boolean {
    if (this.showSheet) {
      this.closeSheet();
      return true;
    }
    if (!this.wideLayout && this.drawerOpen) {
      this.drawerOpen = false;
      return true;
    }
    if (this.pathStack.size() > 0) {
      this.pathStack.pop();
      return true;
    }
    return false;
  }

  /** Accessibility labels take plain strings, so resources are resolved. */
  private a11y(resource: Resource): string {
    return Copy.resolve(this.getUIContext().getHostContext() as common.Context, resource);
  }

  /**
   * Opens one of the chat surface's sheets.
   *
   * The context list is re-read here rather than at launch, because the set of
   * sources a conversation can reach changes with its bindings and with what
   * earlier turns imported. A list frozen at startup shows a source the model
   * cannot reach any more, or hides one it started reaching after the first
   * turn — and this is the moment the answer is actually asked for.
   */
  private openSheet(kind: SheetKind): void {
    if (kind === SheetKind.Context) {
      this.controller.loadContextSources();
    }
    this.sheet = kind;
    this.showSheet = true;
  }

  private closeSheet(): void {
    this.showSheet = false;
    this.sheet = SheetKind.None;
  }

  /**
   * The chat top bar.
   *
   * Exactly five slots, in this order: history, conversation title, model, then
   * the library and settings on the right with settings rightmost. There is no
   * overflow menu, and there is not going to be one added back. An earlier
   * version had a `⋯` here holding three items, and each of the three already
   * had a correct home elsewhere — the conversation's own contents under the
   * title, new chat under the history drawer's own button, and context under
   * both — so the menu was not carrying anything the bar had no slot for. It
   * was carrying the impression that the bar had a slot it did not.
   *
   * The three left slots are `Text` because they are *labels*: they name the
   * current conversation and its current model, and a button frame around a
   * name would claim something about them that is not true. The two right slots
   * are icon buttons because they go somewhere, and they use the platform's own
   * symbols rather than Unicode characters — a `▤` or `⚙` is a font dependency,
   * which is exactly the drift this screen had.
   */
  @Builder
  private topBar() {
    Row({ space: 4 }) {
      if (!this.wideLayout) {
        this.topBarEntry($r('sys.symbol.line_3_horizontal'), $r('app.string.a11y_chat_history'), () => {
          this.drawerOpen = !this.drawerOpen;
        });
      }

      // The title is the current conversation's own name, and tapping it opens
      // that conversation's contents — who is in it, which persona, world books
      // and prompt set it uses. It used to open a character picker, which put
      // resource *selection* behind the only label that names the conversation.
      Text(Copy.sessionTitle('', this.controller.currentCharacter?.name ?? ''))
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary)
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .onClick(() => {
          this.pathStack.pushPath({ name: RouteName.ConversationContents });
        });

      // The model shares the free width with the title rather than taking all
      // of it. A weight here is what keeps a long model id from squeezing the
      // conversation's own name down to an ellipsis: two flexing labels
      // truncate, one flexible label next to an inflexible one gets crushed.
      Text(
        this.controller.currentModel?.name ?? $r('app.string.title_default_model')
      )
        .fontSize(12)
        .fontColor(this.theme.textSecondary)
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .onClick(() => this.openSheet(SheetKind.Models));

      this.topBarEntry($r('sys.symbol.book'), $r('app.string.a11y_library'), () => {
        this.pathStack.pushPath({ name: RouteName.Library });
      });

      // Rightmost, and last in source order, on purpose. Settings is the one
      // thing a user looks for after anything else has failed, and a fixed
      // position in the corner is the only way it can be found without reading
      // the screen.
      this.topBarEntry($r('sys.symbol.gearshape'), $r('app.string.settings_title'), () => {
        this.pathStack.pushPath({ name: RouteName.Settings });
      });
    }
    .width('100%')
    // The window is laid out full screen so the navigation indicator yields to
    // the page instead of the page stepping around it. Nothing avoids the
    // status bar any more, so the bar that must not be under the clock says so
    // here. The six below is the bar's own breathing room, kept separate so the
    // inset can change without touching the spacing.
    .padding({ left: 8, right: 8, top: SafeArea.topVp + 6, bottom: 6 })
    .backgroundColor(this.theme.pageBackground);
  }

  /**
   * One icon button in the top bar.
   *
   * One builder for all three call sites, so "they match" is one definition
   * rather than three that happen to agree today.
   *
   * The button is `ButtonStyleMode.TEXTUAL`, which is the platform's own name for
   * "an icon that goes somewhere, with no fill behind it". That is not a
   * preference for a paler look: `ButtonStyleMode.EMPHASIZED` is the default,
   * and on this device it renders a pale emphasised disc behind each of the
   * three, which puts three filled discs around a conversation title and turns
   * the bar into decoration. Naming the mode is also what keeps the press
   * feedback platform-owned rather than something we would have to repaint.
   */
  @Builder
  private topBarEntry(symbol: Resource, label: Resource, action: () => void) {
    Button({ type: ButtonType.Circle, buttonStyle: ButtonStyleMode.TEXTUAL }) {
      SymbolGlyph(symbol)
        .fontSize(20)
        .fontColor([this.theme.textPrimary]);
    }
      .width(40)
      .height(40)
      .accessibilityText(this.a11y(label))
      .onClick(() => action());
  }

  /**
   * The dim that sits over the conversation while the narrow-layout drawer is
   * open, and the tap target that closes it.
   *
   * `SideBarContainer` provides no overlay of its own. Its whole attribute set
   * is `showSideBar`, `showSideBarWithGesture`, `autoHide`, `sideBarPosition`,
   * `divider` and the width bounds — there is no scrim, no mask and no dim, so
   * "the overlay scrim stays platform provided" is not something the platform
   * offers.
   *
   * That leaves a real trap on a narrow window. The sidebar is 300vp, which is
   * about 938px on this panel, so it covers x 0-938 — including the top bar's
   * own toggle at x 25-150. Once the drawer is open there is no button left to
   * close it, and a tap aimed at the conversation lands on the conversation
   * instead of dismissing anything. Edge swipe and the system back gesture both
   * work, but neither is discoverable and one of them is a system-level
   * shortcut rather than this app's.
   *
   * So the app draws the overlay it needs. Tapping the covered conversation
   * closes the drawer, which is what a drawer over content is expected to do,
   * and the content stops responding while it is covered, because it is not
   * what the user is looking at.
   */
  @Builder
  private drawerScrim() {
    if (this.drawerOpen && !this.wideLayout) {
      Column()
        .width('100%')
        .height('100%')
        .backgroundColor(this.theme.scrim)
        .onClick(() => {
          this.drawerOpen = false;
        });
    }
  }

  @Builder
  private conversationColumn(weight: number = 0) {
    Stack({ alignContent: Alignment.TopStart }) {
      Column() {
        this.topBar();
        ConversationView({ controller: this.controller, theme: this.theme });
        Composer({ controller: this.controller, theme: this.theme });
      }
      .width('100%')
      .height('100%')
      .backgroundColor(this.theme.pageBackground);

      this.drawerScrim();
    }
    .width('100%')
    .height('100%')
    .layoutWeight(weight);
  }

  /**
   * History drawer. On an expanded window it is pinned open; on a narrow one it
   * slides over the conversation and closes on selection.
   */
  @Builder
  private historyDrawer() {
    HistoryPanel({
      controller: this.controller,
      theme: this.theme,
      onPick: () => {
        if (!this.wideLayout) {
          this.drawerOpen = false;
        }
      }
    });
  }

  @Builder
  private sheetContent() {
    if (this.sheet === SheetKind.Models) {
      ModelSelectorSheet({
        controller: this.controller,
        theme: this.theme,
        // Goes through the controller rather than selectModel because the list
        // is what the endpoint *probed*, and a probed model is not a configured
        // one. Selecting it in place would highlight it and change nothing: the
        // turn builds its provider from the endpoint's own model, so a pick
        // here has to be saved to be real.
        //
        // The sheet closes immediately rather than after the save. The save
        // reaches the network, and a picker that hangs open on a slow endpoint
        // reads as a frozen screen; the model field in Settings and the top bar
        // both follow the save, so there is nothing to wait and watch.
        onPick: (modelId: string) => {
          this.closeSheet();
          this.controller.chooseModel(modelId);
        }
      });
    } else if (this.sheet === SheetKind.Context) {
      ContextSheet({ controller: this.controller, theme: this.theme });
    }
  }

  build() {
    Navigation(this.pathStack) {
      SideBarContainer(SideBarContainerType.AUTO) {
        this.historyDrawer();
        this.conversationColumn();
      }
      .width('100%')
      .height('100%')
      .sideBarWidth(300)
      .showSideBar(this.drawerOpen)
      // The platform control button floats over the content area, which would
      // sit below the top bar, and it is hidden anyway. The top bar owns the
      // toggle and `drawerScrim` owns the dismissal; the edge swipe stays
      // platform provided because the platform genuinely does provide it.
      .showControlButton(false)
      .showSideBarWithGesture(true)
      .autoHide(!this.wideLayout)
      .onChange((visible: boolean) => {
        this.drawerOpen = visible;
      })
      .onAreaChange((_oldArea: Area, newArea: Area) => {
        const wide: boolean = newArea.width >= EXPANDED_WIDTH;
        if (wide !== this.wideLayout) {
          this.wideLayout = wide;
          this.drawerOpen = wide;
        }
      });
    }
    .navDestination(this.destination)
    .hideTitleBar(true)
    .hideBackButton(true)
    .mode(NavigationMode.Stack)
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground)
    .bindSheet($$this.showSheet, this.sheetContent(), {
      height: SheetSize.MEDIUM,
      showClose: true,
      dragBar: true,
      backgroundColor: this.theme.surface
    });
  }

  @Builder
  private destination(name: string, param: Object) {
    // The builder must emit a NavDestination: Navigation registers it as a
    // real destination, renders its content and consumes the back gesture.
    // Emitting bare content here leaves a blank page and lets back exit the
    // whole app.
    NavDestination() {
      if (name === RouteName.Library) {
        LibraryHub({
          controller: this.controller,
          theme: this.theme,
          onBack: () => this.pathStack.pop(),
          onOpenCharacters: () => this.pathStack.pushPath({ name: RouteName.Characters }),
          onOpenPersonas: () => this.pathStack.pushPath({ name: RouteName.Personas }),
          onOpenWorldBooks: () => this.pathStack.pushPath({ name: RouteName.WorldBooks }),
          onOpenPrompts: () => this.pathStack.pushPath({ name: RouteName.Prompts })
        })
      } else if (name === RouteName.Characters) {
        CharacterLibrary({
          controller: this.controller,
          theme: this.theme,
          onBack: () => this.pathStack.pop(),
          onNew: () => this.pathStack.pushPath({ name: RouteName.CharacterEdit, param: '' }),
          onOpenCharacter: (id: string) => this.pathStack.pushPath({ name: RouteName.CharacterEdit, param: id })
        })
      } else if (name === RouteName.CharacterEdit) {
        CharacterEditor({
          controller: this.controller,
          theme: this.theme,
          characterId: param as string,
          onBack: () => this.pathStack.pop()
        })
      } else if (name === RouteName.Personas) {
        PersonaLibrary({
          controller: this.controller,
          theme: this.theme,
          onBack: () => this.pathStack.pop(),
          onNew: () => this.pathStack.pushPath({ name: RouteName.PersonaEdit, param: '' }),
          onOpenPersona: (id: string) => this.pathStack.pushPath({ name: RouteName.PersonaEdit, param: id })
        })
      } else if (name === RouteName.PersonaEdit) {
        PersonaEditor({
          controller: this.controller,
          theme: this.theme,
          personaId: param as string,
          onBack: () => this.pathStack.pop()
        })
      } else if (name === RouteName.WorldBooks) {
        WorldBookLibrary({
          controller: this.controller,
          theme: this.theme,
          onBack: () => this.pathStack.pop(),
          onNew: () => this.pathStack.pushPath({ name: RouteName.WorldBookEdit, param: '' }),
          onOpenWorldBook: (id: string) => this.pathStack.pushPath({ name: RouteName.WorldBookEdit, param: id })
        })
      } else if (name === RouteName.WorldBookEdit) {
        WorldBookEditor({
          controller: this.controller,
          theme: this.theme,
          worldBookId: param as string,
          onBack: () => this.pathStack.pop()
        })
      } else if (name === RouteName.Prompts) {
        PromptLibrary({
          controller: this.controller,
          theme: this.theme,
          onBack: () => this.pathStack.pop(),
          onNew: () => this.pathStack.pushPath({ name: RouteName.PromptEdit, param: '' }),
          onOpenProfile: (id: string) => this.pathStack.pushPath({ name: RouteName.PromptEdit, param: id })
        })
      } else if (name === RouteName.PromptEdit) {
        PromptEditor({
          controller: this.controller,
          theme: this.theme,
          profileId: param as string,
          onBack: () => this.pathStack.pop()
        })
      } else if (name === RouteName.ConversationContents) {
        ConversationContents({
          controller: this.controller,
          theme: this.theme,
          onBack: () => this.pathStack.pop(),
          onOpenContext: () => this.openSheet(SheetKind.Context)
        })
      } else if (name === RouteName.SettingsProvider) {
        this.settingsProviderView()
      } else if (name === RouteName.SettingsAppearance) {
        this.settingsAppearanceView()
      } else if (name === RouteName.SettingsDiagnostics) {
        this.settingsDiagnosticsView()
      } else if (name === RouteName.SettingsAbout) {
        this.settingsAboutView()
      } else {
        this.settingsView()
      }
    }
    .hideTitleBar(true)
    .onWillShow(() => {
      // The stored endpoint can change while the app is running — picking a
      // model from the top bar rewrites it — so a form filled once at launch
      // is a form showing what was true at launch. Two screens disagreeing
      // about which model is configured is not a cosmetic problem: re-saving
      // this form would revert the pick the user just made.
      //
      // Gated on the settings route because this runs for every destination,
      // and because the fields below are only ever drawn there.
      if (name === RouteName.Settings || name === RouteName.SettingsProvider) {
        this.openSettingsOnStoredEndpoint();
      }
    })
    .backgroundColor(this.theme.pageBackground);
  }

  /**
   * One settings field.
   *
   * The `text` parameter is the field's *initial* value. The toolkit this
   * project builds against offers no two-way binding for it, so a field wired
   * the obvious one-way way looks right for as long as the user is the one
   * typing, and quietly lies the moment something else writes to it: choosing a
   * model out of a discovered list, or clearing the form.
   *
   * So the input is wrapped in a ForEach keyed on an epoch, and the two moments
   * that write to a field from outside a keystroke bump that epoch. The rebuild
   * is what makes the new value visible. Typing does not bump it, so the field
   * is never rebuilt out from under the cursor.
   */
  @Builder
  private providerField(field: string, label: ResourceStr) {
    Column({ space: 2 }) {
      Text(label)
        .fontSize(12)
        .fontColor(this.theme.textSecondary)
        .width('100%');
      ForEach([this.providerFieldEpoch], (): void => {
        if (field === 'baseUrl') {
          TextInput({ text: this.providerBaseUrl, placeholder: '' })
            .type(InputType.Normal)
            .fontSize(14)
            .backgroundColor(this.theme.surfaceMuted)
            .fontColor(this.theme.textPrimary)
            .width('100%')
            .onChange((next: string): void => {
              this.providerBaseUrl = next;
            })
        } else if (field === 'model') {
          TextInput({ text: this.providerModel, placeholder: '' })
            .type(InputType.Normal)
            .fontSize(14)
            .backgroundColor(this.theme.surfaceMuted)
            .fontColor(this.theme.textPrimary)
            .width('100%')
            .onChange((next: string): void => {
              this.providerModel = next;
            })
        } else {
          TextInput({ text: this.providerKey, placeholder: '' })
            .type(InputType.Password)
            .fontSize(14)
            .backgroundColor(this.theme.surfaceMuted)
            .fontColor(this.theme.textPrimary)
            .width('100%')
            .onChange((next: string): void => {
              this.providerKey = next;
            })
        }
      }, (): string => this.providerFieldEpoch.toString())
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%');
  }

  /**
   * Hands the typed fields to presentation and clears the secret field. The
   * view supplies no provider semantics: the kind comes from the runtime's own
   * list and sampling defaults are the runtime's, so a settings form here
   * cannot disagree with how a turn is actually driven.
   */
  private saveProvider(): void {
    this.controller.saveProviderFromForm(
      this.providerBaseUrl.trim(),
      this.providerModel.trim(),
      this.providerKey.trim()
    );
    this.providerKey = '';
  }

  /**
   * Asks an address what it can do, before anything is saved.
   *
   * The button sits above the save button on purpose. Saving demands a model,
   * and the model is the one thing the user cannot be expected to know before
   * asking, so an endpoint that has to be saved before it can be explored asks
   * for its own answer in advance. This is the order the work actually happens
   * in: type an address, ask, then choose.
   */
  private exploreProvider(refresh: boolean = false): void {
    this.controller.exploreEndpoint(this.providerBaseUrl.trim(), this.providerKey.trim(), refresh);
  }

  private chooseModel(id: string): void {
    this.providerModel = id;
    this.providerFieldEpoch += 1;
  }

  /**
   * Copies the whole log for a bug report.
   *
   * The text comes from the runtime already redacted, so this path has nothing
   * to strip and no way to be the thing that leaks a key. The notice says the
   * paste is safe, because a user told only "copied" will not paste a log they
   * suspect contains their key.
   */
  private copyDiagnostics(): void {
    this.controller.copyDiagnostics().then((copied: boolean) => {
      this.diagnosticsCopied = copied;
    });
  }

  private clearDiagnostics(): void {
    this.controller.clearDiagnostics();
    this.diagnosticsCopied = false;
  }

  private clearProvider(): void {
    this.controller.clearProvider();
    this.providerBaseUrl = '';
    this.providerModel = '';
    this.providerKey = '';
    this.providerFieldEpoch += 1;
  }

  /**
   * The models this endpoint listed, in a panel that can be opened, closed and
   * scrolled.
   *
   * The list shows *every* model the endpoint reported. An earlier version
   * wrapped the first twelve into chips down the page and wrote "and 69 more",
   * which is the worst of both: the models were scattered through the form
   * instead of being a list, and the ones past the cap were unreachable without
   * typing a name that the user could not have known. A scroll area inside a
   * fixed-height panel has neither problem, and it is also the only shape in
   * which eighty-one rows are not a hazard.
   *
   * The height is bounded so the list never grows the page, and the manual
   * entry field stays right below it in every state, so a model this endpoint
   * did not report is still one text field away.
   */
  @Builder
  private modelPicker() {
    Column({ space: 6 }) {
      Row({ space: 6 }) {
        Text($r('app.string.provider_model_choose'))
          .fontSize(12)
          .fontColor(this.theme.textSecondary)
          .layoutWeight(1);
        Text($r('app.string.provider_models_count_short', this.controller.discoveredModelCount))
          .fontSize(12)
          .fontColor(this.theme.textSecondary);
        // A labelled control, not a triangle to be guessed at, and not a
        // hand-painted link: a settings screen is the wrong place to invent a
        // gesture vocabulary. The whole header row toggles, and this button is
        // what says so.
        Button(this.modelListOpen
          ? $r('app.string.provider_models_hide')
          : $r('app.string.provider_models_show'), { type: ButtonType.Normal })
          .fontSize(12)
          .fontColor(this.theme.accent)
          .backgroundColor(Color.Transparent)
          .padding({ left: 8, right: 8 })
          .onClick(() => {
            this.modelListOpen = !this.modelListOpen;
          });
      }
      .width('100%');

      if (this.modelListOpen) {
        // Clipped to its own box on purpose. Without this the last visible row
        // spills over the label of the field underneath it, which reads as a
        // rendering fault rather than as "there is more below" — and the field
        // being overlapped is the one control that names a model by hand.
        Column() {
          List({ space: 2 }) {
          ForEach(this.controller.discoveredModels, (model: ModelSummary) => {
            ListItem() {
              Row({ space: 8 }) {
                // Only the row the field already names is marked. The colour
                // reflects the user's own text; it is not the program proposing
                // a model. A probe that highlighted its first result would be
                // choosing for the user, and one highlighted row among eighty
                // reads exactly like that.
                Text(model.name)
                  .fontSize(13)
                  .fontColor(this.providerModel === model.id
                    ? this.theme.textOnAccent
                    : this.theme.textPrimary)
                  .layoutWeight(1)
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis });
                if (this.providerModel === model.id) {
                  Text($r('app.string.provider_model_current'))
                    .fontSize(11)
                    .fontColor(this.theme.textOnAccent);
                }
              }
              .width('100%')
              .padding({ left: 10, right: 10, top: 8, bottom: 8 })
              .borderRadius(8)
              .backgroundColor(this.providerModel === model.id
                ? this.theme.accent
                : this.theme.surfaceMuted)
              .onClick(() => this.chooseModel(model.id));
            }
          }, (model: ModelSummary) => model.id);
        }
        .width('100%')
        // The List takes the box it is given and scrolls inside it. Without a
        // weight it lays out at its full content height — all eighty-one rows —
        // and the parent's clip only hides what falls outside, while the rows
        // near the boundary still draw over the field below. A child that
        // overflows its box is not a scroll view, however much the parent
        // clips.
        .layoutWeight(1)
        .clip(true)
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring);
        }
        .width('100%')
        .height(216)
        .clip(true)
        // A visible edge, so the row cut by the boundary reads as a scroll
        // affordance rather than a rendering fault. The height is not tuned to
        // a whole number of rows on purpose: a row's height depends on the font
        // scale, so a box that happens to end on a boundary today ends
        // mid-row the moment the user has larger text. A defined edge is
        // honest at any size, an exact fit is not.
        .border({ width: 1, color: this.theme.outline, radius: 10 })
        .backgroundColor(this.theme.surface);
      }
    }
    .width('100%');
  }

  /**
   * What asking an address told us.
   *
   * The protocol and the model list are shown as two separate answers because
   * they are two separate questions. A gateway can list nothing and still speak,
   * and it can list eighty models and speak only the oldest protocol, so
   * reporting one as if it settled the other would be a lie the user then
   * configures around.
   */
  @Builder
  private exploredProvider() {
    Column({ space: 6 }) {
      Row({ space: 6 }) {
        Text($r('app.string.provider_protocol_found'))
          .fontSize(12)
          .fontColor(this.theme.textSecondary);
        Text(this.controller.exploration !== null && this.controller.exploration.protocol.length > 0
          ? this.controller.exploration.protocol
          : $r('app.string.provider_protocol_none'))
          .fontSize(12)
          .fontColor(this.theme.textPrimary);
      }
      // The listing sentence comes from a code, in this app's language. The
      // runtime's own English sentence about it is kept for the diagnostic log,
      // where a developer is the reader; on this screen it would be an
      // untranslated clause inside a translated paragraph.
      if (this.controller.exploration !== null) {
        Text(Copy.listingNote(this.controller.listingCode))
          .fontSize(12)
          .fontColor(this.theme.textSecondary)
          .width('100%');
      }
      if (this.controller.discoveredModels.length > 0) {
        this.modelPicker()
      }
      if (this.controller.providerNoticeCode.length > 0) {
        Text(Copy.errorText(this.controller.providerNoticeCode, ''))
          .fontSize(12)
          .fontColor(this.theme.danger)
          .width('100%');
      }
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%');
  }

  /**
   * Colour for the endpoint state line.
   *
   * Only failures are coloured. The earlier version had a green token for
   * success, which meant inventing a fourth status semantic the platform does
   * not have, and it meant a working endpoint was marked in a colour the rest of
   * the screen never used. The state text already says in words whether the probe
   * worked — "usable", "models found", "no address yet" — so success needs no
   * colour to be legible, and a line that goes red is unambiguous.
   */
  private endpointStateColor(): ResourceColor {
    const state: EndpointState =
      this.controller.endpointState(this.providerBaseUrl, this.providerKey);
    if (state === EndpointState.AuthFailed || state === EndpointState.NoSuchEndpoint ||
      state === EndpointState.NetworkError || state === EndpointState.ServerError ||
      state === EndpointState.RateLimited || state === EndpointState.NoUsableProtocol) {
      return this.theme.danger;
    }
    return this.theme.textSecondary;
  }

  /**
   * The diagnostic log, on its own page.
   *
   * This is the answer to "why did that not work", and it is the reason the log
   * was written before the rest of this flow: a failed probe with no record is
   * a failure the user can only describe and not diagnose. Every line arrives
   * already redacted, so this can be read on screen or copied into a bug report
   * without a filtering pass that could be got wrong.
   *
   * It is a page of its own rather than a disclosure inside the provider form,
   * for a reason that is about the reader. Beside the save button it was the
   * third thing competing for attention on one screen: the form is what a user
   * opens in order to change a setting, and the log is what they open after that
   * failed. Folding them together made both worse — the form grew a hundred
   * lines above its own commit control, and the log was a button labelled "show"
   * that gave no hint of what it would show.
   */
  @Builder
  private settingsDiagnosticsView() {
    Column() {
      PageHeader({
        theme: this.theme,
        title: $r('app.string.settings_category_diagnostics'),
        onBack: () => this.pathStack.pop()
      });

      Scroll() {
        Column({ space: 8 }) {
          // Neither button is emphasised, and both go dead when the log is
          // empty. This page has no primary action: the user came here to read,
          // and the two controls are ways of taking the log somewhere else or
          // throwing it away. An emphasised "clear log" on an empty log is also
          // the one button on this page that would destroy the evidence the
          // page exists to preserve.
          Row({ space: 8 }) {
            Button($r('app.string.provider_diagnostics_copy'), {
              buttonStyle: ButtonStyleMode.NORMAL
            })
              .fontSize(13)
              .layoutWeight(1)
              .enabled(this.controller.diagnostics.length > 0)
              .onClick(() => this.copyDiagnostics());
            Button($r('app.string.provider_diagnostics_clear'), {
              buttonStyle: ButtonStyleMode.NORMAL
            })
              .fontSize(13)
              .enabled(this.controller.diagnostics.length > 0)
              .onClick(() => this.clearDiagnostics());
          }
          .width('100%');

          if (this.diagnosticsCopied) {
            Text($r('app.string.provider_diagnostics_copied'))
              .fontSize(12)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          } else if (this.controller.diagnostics.length === 0) {
            Text($r('app.string.provider_diagnostics_empty'))
              .fontSize(13)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          } else {
            // Newest last, so the end of the list is where the answer is. The
            // whole log is shown rather than a tail: the line that explains a
            // failure is rarely the last one, and a log that only shows the end
            // hides the request that caused it.
            ForEach(this.controller.diagnostics, (entry: DiagnosticEntry) => {
              Text(entry.detail)
                .fontSize(11)
                .fontColor(
                  entry.kind === 'discovery' ? this.theme.textSecondary : this.theme.textPrimary
                )
                .width('100%');
            }, (entry: DiagnosticEntry) => `${entry.atMs}-${entry.stage}-${entry.message}`);
            Text($r('app.string.provider_diagnostics_note'))
              .fontSize(11)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
        }
        .width('100%')
        .padding(16);
      }
      .width('100%')
      .align(Alignment.Top)
      .layoutWeight(1);
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }

  /**
   * One row on the settings category list.
   *
   * A platform list row: label, optional current value as the secondary line,
   * and the platform chevron. The value line is what makes the first level
   * useful before anything is opened — "Provider & API — not configured" tells
   * the reader whether their next step is typing or debugging.
   */
  @Builder
  private settingsRow(label: ResourceStr, value: string, action: () => void) {
    Row({ space: 12 }) {
      Column({ space: 3 }) {
        Text(label)
          .fontSize(16)
          .fontColor(this.theme.textPrimary)
          .width('100%')
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis });
        if (value.length > 0) {
          Text(value)
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%')
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis });
        }
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1);
      SymbolGlyph($r('sys.symbol.chevron_right'))
        .fontSize(16)
        .fontColor([this.theme.textSecondary]);
    }
    .width('100%')
    .padding(16)
    .onClick(() => action());
  }

  /**
   * The provider form, on its own page.
   *
   * Address, key, ask, model, save — the order the work happens in, not a
   * cosmetic arrangement. A model field placed before the probe asks for the one
   * thing the user cannot know yet, and makes the endpoint un-saveable until
   * they have guessed the answer the runtime could have looked up.
   */
  @Builder
  private settingsProviderView() {
    Column() {
      PageHeader({
        theme: this.theme,
        title: $r('app.string.settings_category_provider'),
        onBack: () => this.pathStack.pop()
      });

      Scroll() {
        Column({ space: 12 }) {
          Text($r('app.string.provider_hint'))
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%');
          this.providerField('baseUrl', $r('app.string.provider_base_url'));
          this.providerField('apiKey', $r('app.string.provider_api_key'));
          if (this.controller.hasCredential) {
            Text($r('app.string.provider_key_stored'))
              .fontSize(12)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
          // An address alone is enough to ask. Refusing the button without a
          // key would also be a way of saying "not configured yet" before
          // anything has been typed, which is not the same message.
          //
          // NORMAL, not the default EMPHASIZED. Probing is a question, and Save
          // at the bottom of the page is the answer; two emphasized buttons on
          // one form is a page asking the reader to pick between them.
          Button(this.controller.exploringEndpoint
            ? $r('app.string.provider_exploring')
            : $r('app.string.provider_explore'), { buttonStyle: ButtonStyleMode.NORMAL })
            .fontSize(13)
            .width('100%')
            .enabled(!this.controller.exploringEndpoint && this.providerBaseUrl.trim().length > 0)
            .onClick(() => this.exploreProvider());
          Text($r('app.string.provider_explore_hint'))
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%');
          // The status line, always. Which state this is tells the user what to
          // do next, and a form that only shows one when it fails leaves them
          // unable to tell a working endpoint from an untested one.
          Text(Copy.endpointState(
            this.controller.endpointState(this.providerBaseUrl, this.providerKey),
            this.controller.discoveredModelCount
          ))
            .fontSize(12)
            .fontColor(this.endpointStateColor())
            .width('100%');
          if (this.controller.explored) {
            this.exploredProvider()
          }
          // The model comes last, because it is the outcome of the probe. It
          // stays editable in every state: a gateway with no listing route still
          // chats, and taking the text field away there would close the only
          // door the user has left.
          this.providerField('model', $r('app.string.provider_model'));
          Text($r('app.string.provider_model_hint'))
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%');
          if (this.controller.hasProvider) {
            // Re-probing is separate from saving. A user who suspects the stored
            // key is wrong needs to ask again without committing anything, and
            // folding that into the save button would make every re-check a
            // configuration change.
            Button($r('app.string.provider_reprobe'), { buttonStyle: ButtonStyleMode.NORMAL })
              .fontSize(13)
              .width('100%')
              .enabled(!this.controller.exploringEndpoint && this.providerBaseUrl.trim().length > 0)
              // This one asks again. Everywhere else the runtime answers from
              // what the endpoint already said, which is what keeps opening this
              // screen free — and which would also keep showing a stale answer
              // to a user who has just corrected their key.
              .onClick(() => this.exploreProvider(true));
          }
          Row({ space: 8 }) {
            // The one primary action on this page, and so the one button that
            // wears the emphasis fill.
            Button($r('app.string.provider_save'))
              .fontSize(13)
              .layoutWeight(1)
              .backgroundColor(this.theme.accent)
              .fontColor(this.theme.textOnAccent)
              .enabled(!this.controller.savingProvider)
              .onClick(() => this.saveProvider());
            if (this.controller.hasProvider) {
              // NORMAL for the same reason as probe: this row has exactly one
              // primary action in it, and it is Save.
              Button($r('app.string.provider_clear'), { buttonStyle: ButtonStyleMode.NORMAL })
                .fontSize(13)
                .onClick(() => this.clearProvider());
            }
          }
          .width('100%');
          if (this.controller.errorCode === 'provider_not_saved') {
            Text(Copy.errorText(this.controller.errorCode, this.controller.errorDetail))
              .fontSize(12)
              .fontColor(this.theme.danger)
              .width('100%');
          }
        }
        .alignItems(HorizontalAlign.Start)
        .width('100%')
        .padding(16);
      }
      .width('100%')
      .align(Alignment.Top)
      .layoutWeight(1);
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }

  /**
   * Appearance, on its own page.
   *
   * Radio buttons rather than a row of chips the app painted itself. Three
   * mutually exclusive options in a vertical settings list is what a radio
   * group is for, and the previous version was three `Text` elements with
   * hand-applied padding and radius: no press feedback, no selected semantics
   * for a screen reader, and a blue fill the app had chosen rather than one the
   * platform had.
   *
   * `checked` is passed in rather than inferred from a click, because the
   * selection can also change from the platform's own dark-mode switch; a group
   * that only knew what was tapped would show the wrong tick.
   */
  @Builder
  private settingsAppearanceView() {
    Column() {
      PageHeader({
        theme: this.theme,
        title: $r('app.string.settings_category_appearance'),
        onBack: () => this.pathStack.pop()
      });

      List() {
        ForEach(APPEARANCE_MODES, (mode: AppearanceMode) => {
          ListItem() {
            Row({ space: 12 }) {
              Text(Copy.appearanceMode(mode))
                .fontSize(16)
                .fontColor(this.theme.textPrimary)
                .layoutWeight(1);
              Radio({ value: mode, group: APPEARANCE_GROUP })
                .checked(this.controller.appearanceMode === mode)
                .onChange((checked: boolean) => {
                  if (checked) {
                    this.controller.setAppearance(mode);
                  }
                });
            }
            .width('100%')
            .padding(16);
          }
        }, (mode: AppearanceMode) => mode);
      }
      .width('100%')
      .layoutWeight(1);
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }

  /**
   * About, on its own page.
   *
   * Two rows of label and value. Nothing here is interactive, so nothing gets a
   * chevron: a row that looks tappable and is not is worse than a plain line.
   */
  @Builder
  private settingsAboutView() {
    Column() {
      PageHeader({
        theme: this.theme,
        title: $r('app.string.settings_category_about'),
        onBack: () => this.pathStack.pop()
      });

      List() {
        ListItem() {
          Column({ space: 2 }) {
            Text($r('app.string.settings_about_platform'))
              .fontSize(15)
              .fontColor(this.theme.textPrimary)
              .width('100%');
            Text(this.controller.platformSummary())
              .fontSize(12)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')
          .padding(16);
        }
        ListItem() {
          Column({ space: 2 }) {
            Text($r('app.string.settings_about_runtime'))
              .fontSize(15)
              .fontColor(this.theme.textPrimary)
              .width('100%');
            // The bridge reports which runtime; the words describing it are
            // copy, so they are resolved here where the locale applies.
            Text(`${this.controller.runtimeSummary()} · ${Copy.resolve(getContext(this), $r('app.string.settings_about_runtime_native'))}`)
              .fontSize(12)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')
          .padding(16);
        }
      }
      .width('100%')
      .layoutWeight(1);
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }

  /**
   * Settings, first level: the category list.
   *
   * Four rows and nothing else. This is the native settings pattern: a page that
   * names its categories and lets the reader choose one, rather than a page
   * where the configuration, the probe result, the model list and a log viewer
   * are all laid out at once and compete.
   *
   * There is deliberately no "Data & backup" row. The documentation lists the
   * category as planned work, `sujiu-runtime` exports nothing and imports
   * nothing, and a row that opens an empty page would be a feature the user can
   * see and cannot use. The category appears here when there is something behind
   * it — see the status table in `docs/UI_ARCHITECTURE.md`.
   */
  @Builder
  private settingsView() {
    Column() {
      PageHeader({ theme: this.theme, title: $r('app.string.settings_title'), onBack: () => this.pathStack.pop() });

      List() {
        ListItem() {
          this.settingsRow(
            $r('app.string.settings_category_provider'),
            this.providerSummary(),
            () => this.pathStack.pushPath({ name: RouteName.SettingsProvider })
          );
        }
        ListItem() {
          this.settingsRow($r('app.string.settings_category_appearance'), '', () => {
            this.pathStack.pushPath({ name: RouteName.SettingsAppearance });
          });
        }
        ListItem() {
          this.settingsRow($r('app.string.settings_category_diagnostics'), '', () => {
            this.pathStack.pushPath({ name: RouteName.SettingsDiagnostics });
          });
        }
        ListItem() {
          this.settingsRow($r('app.string.settings_category_about'), '', () => {
            this.pathStack.pushPath({ name: RouteName.SettingsAbout });
          });
        }
      }
      .width('100%')
      .layoutWeight(1);
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }

  /**
   * The provider category's current value, as one line.
   *
   * The host when there is one, because that is the thing a user recognises;
   * "configured" on its own does not say which endpoint, and "not configured"
   * is what a user needs to see before they decide whether to open the page at
   * all.
   */
  private providerSummary(): string {
    const stored: StoredEndpoint | null = this.controller.storedEndpoint;
    if (stored === null || stored.baseUrl.length === 0) {
      return Copy.resolve(
        this.getUIContext().getHostContext() as common.Context,
        $r('app.string.settings_provider_summary_none')
      );
    }
    return stored.baseUrl;
  }
}
