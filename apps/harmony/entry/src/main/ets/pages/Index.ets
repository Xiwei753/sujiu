import { common } from '@kit.AbilityKit';
import { KeyboardAvoidMode } from '@kit.ArkUI';
import { CharacterSummary } from '../bridge/SujiuBridge';
import { AppGraph } from '../app/Controller';
import { Composer } from '../components/Composer';
import { ConversationView } from '../components/ConversationView';
import { HistoryPanel } from '../components/HistoryPanel';
import { CharacterSelectorSheet, ContextSheet, ModelSelectorSheet } from '../components/SelectorSheets';
import { AppearanceMode } from '../platform/PlatformServices';
import { ChatController } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';

enum SheetKind {
  None = 'none',
  Models = 'models',
  Characters = 'characters',
  Context = 'context',
  More = 'more'
}

/** Route names for ArkUI `Navigation`; the stack itself is platform owned. */
enum RouteName {
  Library = 'Library',
  CharacterDetail = 'CharacterDetail',
  Settings = 'Settings'
}

/** Width at which history becomes a permanent sidebar (layout promotion). */
const EXPANDED_WIDTH: number = 840;

const APPEARANCE_MODES: AppearanceMode[] = [
  AppearanceMode.System,
  AppearanceMode.Light,
  AppearanceMode.Dark
];

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
  private pathStack: NavPathStack = new NavPathStack();

  aboutToAppear(): void {
    this.graph.bind(this.getUIContext().getHostContext() as common.Context);
    // RESIZE, not the default OFFSET: with OFFSET the IME pushes the whole
    // page up, so the top bar slides under the status bar and the conversation
    // overlaps the system area. Resizing keeps the chat chrome in place.
    this.getUIContext().setKeyboardAvoidMode(KeyboardAvoidMode.RESIZE);
    this.graph.start();
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

  private openSheet(kind: SheetKind): void {
    this.sheet = kind;
    this.showSheet = true;
  }

  private closeSheet(): void {
    this.showSheet = false;
    this.sheet = SheetKind.None;
  }

  @Builder
  private topBar() {
    Row({ space: 4 }) {
      if (!this.wideLayout) {
        Text('☰')
          .fontSize(20)
          .fontColor(this.theme.textPrimary)
          .width(40)
          .height(40)
          .textAlign(TextAlign.Center)
          .accessibilityText('Chat history')
          .onClick(() => {
            this.drawerOpen = !this.drawerOpen;
          });
      }

      Text(this.controller.currentCharacter?.name ?? 'New chat')
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary)
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .onClick(() => this.openSheet(SheetKind.Characters));

      Text(this.controller.currentModel?.name ?? 'Select model')
        .fontSize(12)
        .fontColor(this.theme.textSecondary)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .onClick(() => this.openSheet(SheetKind.Models));

      Text('⋯')
        .fontSize(20)
        .fontColor(this.theme.textPrimary)
        .width(40)
        .height(40)
        .textAlign(TextAlign.Center)
        .accessibilityText('More')
        .onClick(() => this.openSheet(SheetKind.More));
    }
    .width('100%')
    .padding({ left: 8, right: 8, top: 6, bottom: 6 })
    .backgroundColor(this.theme.pageBackground);
  }

  @Builder
  private conversationColumn(weight: number = 0) {
    Column() {
      this.topBar();
      ConversationView({ controller: this.controller, theme: this.theme });
      Composer({ controller: this.controller, theme: this.theme });
    }
    .width('100%')
    .height('100%')
    .layoutWeight(weight)
    .backgroundColor(this.theme.pageBackground);
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
        onPick: (modelId: string) => {
          this.controller.selectModel(modelId);
          this.closeSheet();
        }
      });
    } else if (this.sheet === SheetKind.Characters) {
      CharacterSelectorSheet({
        controller: this.controller,
        theme: this.theme,
        onPick: (characterId: string) => {
          this.controller.selectCharacter(characterId);
          this.closeSheet();
        },
        onOpenLibrary: () => {
          this.closeSheet();
          this.pathStack.pushPath({ name: RouteName.Library });
        }
      });
    } else if (this.sheet === SheetKind.Context) {
      ContextSheet({ controller: this.controller, theme: this.theme });
    } else {
      this.moreSheet();
    }
  }

  /** Overflow menu: the few actions that must not live in the top bar. */
  @Builder
  private moreSheet() {
    Column({ space: 4 }) {
      Text('More')
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary)
        .width('100%')
        .padding({ left: 24, top: 16, bottom: 8 });

      Text('Context in this chat')
        .fontSize(16)
        .fontColor(this.theme.textPrimary)
        .width('100%')
        .padding({ left: 24, right: 24, top: 10, bottom: 10 })
        .onClick(() => this.openSheet(SheetKind.Context));

      Text('New chat')
        .fontSize(16)
        .fontColor(this.theme.textPrimary)
        .width('100%')
        .padding({ left: 24, right: 24, top: 10, bottom: 10 })
        .onClick(() => {
          this.closeSheet();
          this.controller.newSession();
        });

      Text('Settings')
        .fontSize(16)
        .fontColor(this.theme.textPrimary)
        .width('100%')
        .padding({ left: 24, right: 24, top: 10, bottom: 16 })
        .onClick(() => {
          this.closeSheet();
          this.pathStack.pushPath({ name: RouteName.Settings });
        });
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%');
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
      // The platform control button floats over the content area, which
      // would sit below the top bar. The top bar owns the toggle instead;
      // edge-swipe and the overlay scrim stay platform provided.
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
        this.libraryView()
      } else if (name === RouteName.CharacterDetail) {
        this.characterDetailView(param as string)
      } else {
        this.settingsView()
      }
    }
    .hideTitleBar(true)
    .backgroundColor(this.theme.pageBackground);
  }

  @Builder
  private pageHeader(title: string) {
    Row({ space: 8 }) {
      Text('←')
        .fontSize(20)
        .fontColor(this.theme.textPrimary)
        .width(40)
        .height(40)
        .textAlign(TextAlign.Center)
        .accessibilityText('Back')
        .onClick(() => this.pathStack.pop());
      Text(title)
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary);
    }
    .width('100%')
    .padding(12)
    .backgroundColor(this.theme.pageBackground);
  }

  @Builder
  private libraryView() {
    Column() {
      this.pageHeader('Characters');

      List({ space: 12 }) {
        ForEach(this.controller.characters, (character: CharacterSummary) => {
          ListItem() {
            Row({ space: 12 }) {
              Text('👤').fontSize(20);
              Column({ space: 2 }) {
                Text(character.name)
                  .fontSize(15)
                  .fontColor(this.theme.textPrimary)
                  .width('100%');
                Text(character.tagline)
                  .fontSize(12)
                  .fontColor(this.theme.textSecondary)
                  .maxLines(2)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .width('100%');
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1);
            }
            .width('100%')
            .padding(16)
            .borderRadius(14)
            .backgroundColor(this.theme.surface)
            .onClick(() => this.pathStack.pushPath({ name: RouteName.CharacterDetail, param: character.id }));
          }
        }, (character: CharacterSummary) => character.id);
      }
      .width('100%')
      .layoutWeight(1)
      .padding(16)
      .backgroundColor(this.theme.pageBackground);
    }
    .width('100%')
    .height('100%');
  }

  @Builder
  private characterDetailView(characterId: string) {
    Column({ space: 12 }) {
      this.pageHeader(this.controller.characterById(characterId)?.name ?? 'Character');

      Text(this.controller.characterById(characterId)?.tagline ?? '')
        .fontSize(15)
        .fontColor(this.theme.textPrimary)
        .width('100%');

      Button('Chat with this character')
        .width('100%')
        .backgroundColor(this.theme.accent)
        .fontColor(this.theme.textOnAccent)
        .onClick(() => {
          this.controller.selectCharacter(characterId);
          this.pathStack.clear();
        });
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .height('100%')
    .padding(16)
    .backgroundColor(this.theme.pageBackground);
  }

  @Builder
  private settingsView() {
    Column() {
      this.pageHeader('Settings');

      List({ space: 8 }) {
        ListItem() {
          Text('APPEARANCE')
            .fontSize(11)
            .fontColor(this.theme.textSecondary)
            .width('100%');
        }
        ListItem() {
          Row({ space: 8 }) {
            ForEach(APPEARANCE_MODES, (mode: AppearanceMode) => {
              Text(mode)
                .fontSize(13)
                .padding({ left: 14, right: 14, top: 8, bottom: 8 })
                .borderRadius(16)
                .backgroundColor(
                  this.controller.appearanceMode === mode
                    ? this.theme.accent
                    : this.theme.surfaceMuted
                )
                .fontColor(
                  this.controller.appearanceMode === mode
                    ? this.theme.textOnAccent
                    : this.theme.textPrimary
                )
                .onClick(() => this.controller.setAppearance(mode));
            }, (mode: AppearanceMode) => mode);
          }
          .width('100%');
        }
        ListItem() {
          Text('ABOUT')
            .fontSize(11)
            .fontColor(this.theme.textSecondary)
            .width('100%');
        }
        ListItem() {
          Column({ space: 2 }) {
            Text('Platform')
              .fontSize(15)
              .fontColor(this.theme.textPrimary)
              .width('100%');
            Text(this.controller.platformSummary())
              .fontSize(12)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%');
        }
        ListItem() {
          Column({ space: 2 }) {
            Text('Runtime')
              .fontSize(15)
              .fontColor(this.theme.textPrimary)
              .width('100%');
            Text(this.controller.runtimeSummary())
              .fontSize(12)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%');
        }
      }
      .width('100%')
      .layoutWeight(1)
      .padding(16)
      .backgroundColor(this.theme.pageBackground);
    }
    .width('100%')
    .height('100%');
  }
}
