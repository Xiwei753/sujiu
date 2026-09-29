import { CharacterSummary, ContextSourceSummary, ModelSummary } from '../bridge/SujiuBridge';
import { ChatController } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
import { AppTheme } from '../ui/theme/Theme';

/** Lightweight model picker: recents first, no separate management page. */
@Component
export struct ModelSelectorSheet {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onPick: (modelId: string) => void = () => {
  };

  build() {
    Column({ space: 4 }) {
      Text($r('app.string.model_sheet_title'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary)
        .width('100%')
        .padding({ left: 24, top: 16, bottom: 8 });

      ForEach(this.controller.models, (model: ModelSummary) => {
        Row() {
          Column({ space: 2 }) {
            Text(model.name).fontSize(16).fontColor(this.theme.textPrimary).width('100%');
            Text($r('app.string.model_sheet_meta', model.providerLabel, model.contextLabel))
              .fontSize(11)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
          .alignItems(HorizontalAlign.Start)
          .layoutWeight(1);

          if (this.controller.currentModel?.id === model.id) {
            Text($r('app.string.model_sheet_in_use'))
              .fontSize(11)
              .fontColor(this.theme.accent);
          }
        }
        .width('100%')
        .padding({ left: 24, right: 24, top: 10, bottom: 10 })
        .onClick(() => this.onPick(model.id));
      }, (model: ModelSummary) => model.id);
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%');
  }
}

/** Lightweight character picker with search plus an entry into the library. */
@Component
export struct CharacterSelectorSheet {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onPick: (characterId: string) => void = () => {
  };
  onOpenLibrary: () => void = () => {
  };

  build() {
    Column({ space: 4 }) {
      TextInput({
        placeholder: $r('app.string.character_sheet_search'),
        text: this.controller.characterQuery
      })
        .fontColor(this.theme.textPrimary)
        .placeholderColor(this.theme.textSecondary)
        .margin({ left: 24, right: 24, top: 16, bottom: 8 })
        .onChange((value: string) => this.controller.searchCharacters(value));

      ForEach(this.controller.characters, (character: CharacterSummary) => {
        Column({ space: 2 }) {
          Text(character.name)
            .fontSize(this.controller.currentCharacter?.id === character.id ? 15 : 16)
            .fontColor(this.theme.textPrimary)
            .width('100%');
          Text(character.description)
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .maxLines(2)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%');
          if (character.tags.length > 0) {
            Text(character.tags.join(' · '))
              .fontSize(11)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          }
        }
        .alignItems(HorizontalAlign.Start)
        .width('100%')
        .padding({ left: 24, right: 24, top: 10, bottom: 10 })
        .onClick(() => this.onPick(character.id));
      }, (character: CharacterSummary) => character.id);

      Button($r('app.string.character_sheet_open_library'))
        .width('100%')
        .margin({ top: 8, bottom: 16 })
        .backgroundColor('#00000000')
        .fontColor(this.theme.accent)
        .onClick(() => this.onOpenLibrary());
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%');
  }
}

/**
 * On-demand context browser.
 *
 * The list of context sources is a *list*, not a dump: the runtime decides what
 * to surface here, and the model decides what to actually read.
 */
@Component
export struct ContextSheet {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;

  build() {
    Column({ space: 4 }) {
      Text($r('app.string.context_sheet_title'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary)
        .width('100%')
        .padding({ left: 24, top: 16, bottom: 8 });

      if (this.controller.contextSources.length === 0) {
        Text($r('app.string.context_sheet_empty'))
          .fontSize(12)
          .fontColor(this.theme.textSecondary)
          .width('100%')
          .padding({ left: 24, right: 24, bottom: 12 });
      }

      ForEach(this.controller.contextSources, (source: ContextSourceSummary) => {
        Column({ space: 2 }) {
          Text(source.name).fontSize(16).fontColor(this.theme.textPrimary).width('100%');
          Text($r('app.string.context_source_meta', source.kindLabel, source.recordCount))
            .fontSize(11)
            .fontColor(this.theme.textSecondary)
            .width('100%');
        }
        .alignItems(HorizontalAlign.Start)
        .width('100%')
        .padding({ left: 24, right: 24, top: 8, bottom: 8 });
      }, (source: ContextSourceSummary) => source.id);
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%');
  }
}
