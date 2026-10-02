import { ContextSourceSummary, ModelSummary } from '../bridge/SujiuBridge';
import { ChatController } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
import { AppTheme } from '../ui/theme/Theme';

/**
 * Model picker, driven by whatever the endpoint actually offered.
 *
 * It shows the discovered list when a probe has produced one, and the configured
 * list otherwise. Listing only the configured models would offer one entry on a
 * fresh configuration, which reads as "there is nothing to switch to" and is
 * true of the wrong collection: a gateway listing eighty-one models has already
 * told us eighty-one, and that answer belongs where the user is already looking
 * for a model. The configured list remains the fallback because it is all
 * there is before anyone has asked.
 *
 * The rows are inside a `Scroll`, not laid out flat. This sheet is a
 * bottom sheet with a fixed height, and eighty-one rows in a column overflow it
 * silently — the tail is simply unreachable, which is worse than a list that
 * visibly continues. Same reasoning as the settings picker: a cap would hide
 * models that a scroll can show for free.
 *
 * The scroll fills the sheet rather than being given a fixed height. A fixed
 * height left a band of empty surface below the list, so the device showed six
 * models and then a blank strip where the sheet kept going. The height the
 * sheet has is the height the list should use; anything less is space taken
 * from the list and given to nothing.
 */
@Component
export struct ModelSelectorSheet {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onPick: (modelId: string) => void = () => {
  };

  /**
   * The rows to show. Both lists are `ModelSummary`, and the discovered one is
   * preferred so switching to a model the user has not saved yet works.
   *
   * A method rather than a `private get`, and that is not a style choice. The
   * declarative builder runs against a proxy that does not back getter members
   * on a struct, so `this.offered` reads as `undefined` at render time and
   * taking its `.length` crashes the sheet the moment it is opened. A method has
   * a prototype behind it and is called, not looked up, so it survives.
   */
  private offeredModels(): ModelSummary[] {
    const found: ModelSummary[] = this.controller.discoveredModels;
    return found.length > 0 ? found : this.controller.models;
  }

  build() {
    Column({ space: 4 }) {
      Text($r('app.string.model_sheet_title'))
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary)
        .width('100%')
        .padding({ left: 24, top: 16, bottom: 8 });

      Text($r('app.string.model_sheet_count', this.offeredModels().length))
        .fontSize(12)
        .fontColor(this.theme.textSecondary)
        .width('100%')
        .padding({ left: 24, right: 24, bottom: 6 });

      // The list scrolls; the two lines above it do not, so a long list never
      // scrolls its own title off the screen.
      Scroll() {
        Column({ space: 2 }) {
          ForEach(this.offeredModels(), (model: ModelSummary) => {
            Row() {
              Column({ space: 2 }) {
                Text(model.name)
                  .fontSize(16)
                  .fontColor(this.theme.textPrimary)
                  .width('100%')
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis });
                Text($r('app.string.model_sheet_meta', model.providerLabel, model.contextLabel))
                  .fontSize(11)
                  .fontColor(this.theme.textSecondary)
                  .width('100%')
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis });
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1);

              // Marked when the endpoint already names this model, which is
              // also what the settings picker marks. The colour reports the
              // stored configuration; it never suggests a model, because a
              // highlighted row reads as the program having chosen.
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
        .width('100%');
      }
      .width('100%')
      // Fills whatever the sheet has left after the title and the count. A fixed
      // height is what left the blank strip at the bottom, and it also cost the
      // list the models that strip could have shown.
      .layoutWeight(1)
      // Top-aligned on purpose. A Scroll centres content shorter than its own
      // box, so with one model on screen the single row floated in the middle of
      // an empty sheet instead of sitting under the count that describes it.
      .align(Alignment.Top)
      .clip(true)
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring);
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    // Without a height of its own the column is exactly as tall as its content,
    // and a layoutWeight child has nothing to divide up.
    .height('100%');
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
        .padding({ left: 24, top: 16, bottom: 4 });

      // Says what this list is before it says what is in it. A bare list of
      // source names gave the reader nothing to judge the names against.
      Text($r('app.string.context_sheet_intro'))
        .fontSize(12)
        .fontColor(this.theme.textSecondary)
        .width('100%')
        .padding({ left: 24, right: 24, bottom: 8 });

      if (this.controller.contextSources.length === 0) {
        Text($r('app.string.context_sheet_empty'))
          .fontSize(12)
          .fontColor(this.theme.textSecondary)
          .width('100%')
          .padding({ left: 24, right: 24, bottom: 12 });
      }

      // Scrolls for the same reason the model sheet does: a conversation with
      // many sources must not quietly lose the tail below the sheet's edge.
      Scroll() {
        Column({ space: 2 }) {
          ForEach(this.controller.contextSources, (source: ContextSourceSummary) => {
            Column({ space: 2 }) {
              Text(source.name).fontSize(16).fontColor(this.theme.textPrimary).width('100%');
              Text($r('app.string.context_source_meta', Copy.contextSourceKind(source.kind), source.recordCount))
                .fontSize(11)
                .fontColor(this.theme.textSecondary)
                .width('100%');
            }
            .alignItems(HorizontalAlign.Start)
            .width('100%')
            .padding({ left: 24, right: 24, top: 8, bottom: 8 });
          }, (source: ContextSourceSummary) => source.id);
        }
        .width('100%');
      }
      .width('100%')
      .layoutWeight(1)
      .align(Alignment.Top)
      .clip(true)
      .scrollBar(BarState.Auto);
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .height('100%');
  }
}
