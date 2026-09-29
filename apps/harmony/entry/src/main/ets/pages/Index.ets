import { common } from '@kit.AbilityKit';
import { CharacterSummary } from '../bridge/SujiuBridge';
import { AppGraph } from '../app/Controller';
import { Composer } from '../components/Composer';
import { ConversationView } from '../components/ConversationView';
import { HistoryPanel } from '../components/HistoryPanel';
import { CharacterSelectorSheet, ContextSheet, ModelSelectorSheet } from '../components/SelectorSheets';
import { AppearanceMode } from '../platform/PlatformServices';
import { RouteName } from '../presentation/Navigator';

enum SheetKind {
  None = 'none',
  History = 'history',
  Models = 'models',
  Characters = 'characters',
  Context = 'context'
}

/** Width at which history becomes a permanent sidebar (layout promotion). */
const EXPANDED_WIDTH: number = 840;

const APPEARANCE_MODES: AppearanceMode[] = [
  AppearanceMode.System,
  AppearanceMode.Light,
  AppearanceMode.Dark
];

/**
 * Chat is the home surface. On a narrow window history and the selectors are
 * sheets; on an expanded window history is promoted to a permanent column.
 * Same screen, promoted layout.
 */
@Entry
@Component
struct Index {
  @State private graph: AppGraph = new AppGraph(getContext(this) as common.Context);
  @State private wideLayout: boolean = false;
  @State private showSheet: boolean = false;
  @State private sheet: SheetKind = SheetKind.None;

  aboutToAppear(): void {
    this.graph.start();
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
  private chatCanvas() {
    Column() {
      Row({ space: 4 }) {
        if (!this.wideLayout) {
          Text('☰')
            .fontSize(20)
            .width(40)
            .height(40)
            .textAlign(TextAlign.Center)
            .onClick(() => this.openSheet(SheetKind.History));
        }

        Text(this.graph.controller.currentCharacter?.name ?? 'New chat')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .onClick(() => this.openSheet(SheetKind.Characters));

        Text(this.graph.controller.currentModel?.name ?? 'Select model')
          .fontSize(12)
          .fontColor('#8A8A99')
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .onClick(() => this.openSheet(SheetKind.Models));

        Text('⋯')
          .fontSize(20)
          .width(40)
          .height(40)
          .textAlign(TextAlign.Center)
          .onClick(() => this.openSheet(SheetKind.Context));
      }
      .width('100%')
      .padding({ left: 8, right: 8, top: 6, bottom: 6 });

      ConversationView({ controller: this.graph.controller });
      Composer({ controller: this.graph.controller });
    }
    .width('100%')
    .height('100%');
  }

  @Builder
  private sheetContent() {
    if (this.sheet === SheetKind.Models) {
      ModelSelectorSheet({
        controller: this.graph.controller,
        onPick: (modelId: string) => {
          this.graph.controller.selectModel(modelId);
          this.closeSheet();
        }
      });
    } else if (this.sheet === SheetKind.Characters) {
      CharacterSelectorSheet({
        controller: this.graph.controller,
        onPick: (characterId: string) => {
          this.graph.controller.selectCharacter(characterId);
          this.closeSheet();
        },
        onOpenLibrary: () => {
          this.closeSheet();
          this.graph.navigator.push(RouteName.Library);
        }
      });
    } else if (this.sheet === SheetKind.Context) {
      ContextSheet({ controller: this.graph.controller });
    } else {
      HistoryPanel({
        controller: this.graph.controller,
        onPick: () => this.closeSheet()
      });
    }
  }

  build() {
    Column() {
      if (this.graph.navigator.current.name === RouteName.Library) {
        this.libraryView()
      } else if (this.graph.navigator.current.name === RouteName.CharacterDetail) {
        this.characterDetailView()
      } else if (this.graph.navigator.current.name === RouteName.Settings) {
        this.settingsView()
      } else if (this.wideLayout) {
        Row() {
          HistoryPanel({
            controller: this.graph.controller,
            onPick: () => {
            }
          })
            .width(300)
            .height('100%');

          this.chatCanvas()
            .layoutWeight(1);
        }
        .width('100%')
        .height('100%');
      } else {
        this.chatCanvas()
      }
    }
    .width('100%')
    .height('100%')
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      this.wideLayout = newArea.width >= EXPANDED_WIDTH;
    })
    .bindSheet($$this.showSheet, this.sheetContent(), {
      height: SheetSize.MEDIUM,
      showClose: true,
      dragBar: true
    });
  }

  @Builder
  private libraryView() {
    Column() {
      Row() {
        Text('←')
          .fontSize(20)
          .width(40)
          .onClick(() => this.graph.navigator.pop());
        Text('Characters')
          .fontSize(18)
          .fontWeight(FontWeight.Medium);
      }
      .width('100%')
      .padding(12);

      List({ space: 12 }) {
        ForEach(this.graph.controller.characters, (character: CharacterSummary) => {
          ListItem() {
            Row({ space: 12 }) {
              Text('👤').fontSize(20);
              Column({ space: 2 }) {
                Text(character.name).fontSize(15).width('100%');
                Text(character.tagline)
                  .fontSize(12)
                  .fontColor('#8A8A99')
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
            .backgroundColor('#14FFFFFF')
            .onClick(() => this.graph.navigator.push(RouteName.CharacterDetail, character.id));
          }
        }, (character: CharacterSummary) => character.id);
      }
      .width('100%')
      .layoutWeight(1)
      .padding(16);
    }
    .width('100%')
    .height('100%');
  }

  @Builder
  private characterDetailView() {
    Column({ space: 12 }) {
      Row() {
        Text('←')
          .fontSize(20)
          .width(40)
          .onClick(() => this.graph.navigator.pop());
        Text(this.graph.controller.characterById(this.graph.navigator.current.parameter)?.name ?? 'Character')
          .fontSize(18)
          .fontWeight(FontWeight.Medium);
      }
      .width('100%')
      .padding(12);

      Text(this.graph.controller.characterById(this.graph.navigator.current.parameter)?.tagline ?? '')
        .fontSize(15)
        .width('100%');

      Button('Chat with this character')
        .width('100%')
        .onClick(() => {
          this.graph.controller.selectCharacter(this.graph.navigator.current.parameter);
          this.graph.navigator.popToRoot();
        });
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .height('100%')
    .padding(16);
  }

  @Builder
  private settingsView() {
    Column() {
      Row() {
        Text('←')
          .fontSize(20)
          .width(40)
          .onClick(() => this.graph.navigator.pop());
        Text('Settings')
          .fontSize(18)
          .fontWeight(FontWeight.Medium);
      }
      .width('100%')
      .padding(12);

      List({ space: 8 }) {
        ListItem() {
          Text('APPEARANCE').fontSize(11).fontColor('#8A8A99').width('100%');
        }
        ListItem() {
          Row({ space: 8 }) {
            ForEach(APPEARANCE_MODES, (mode: AppearanceMode) => {
              Text(mode)
                .fontSize(13)
                .padding({ left: 14, right: 14, top: 8, bottom: 8 })
                .borderRadius(16)
                .backgroundColor(this.graph.controller.appearanceMode === mode ? '#3D5AFE' : '#1AFFFFFF')
                .fontColor(this.graph.controller.appearanceMode === mode ? '#FFFFFF' : '#E8E8F0')
                .onClick(() => this.graph.controller.setAppearance(mode));
            }, (mode: AppearanceMode) => mode);
          }
          .width('100%');
        }
        ListItem() {
          Text('ABOUT').fontSize(11).fontColor('#8A8A99').width('100%');
        }
        ListItem() {
          Column({ space: 2 }) {
            Text('Platform').fontSize(15).width('100%');
            Text(this.graph.controller.platformSummary())
              .fontSize(12)
              .fontColor('#8A8A99')
              .width('100%');
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%');
        }
        ListItem() {
          Column({ space: 2 }) {
            Text('Runtime').fontSize(15).width('100%');
            Text(this.graph.controller.runtimeSummary())
              .fontSize(12)
              .fontColor('#8A8A99')
              .width('100%');
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%');
        }
      }
      .width('100%')
      .layoutWeight(1)
      .padding(16);
    }
    .width('100%')
    .height('100%');
  }
}
