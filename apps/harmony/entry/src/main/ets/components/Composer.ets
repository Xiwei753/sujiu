import { common } from '@kit.AbilityKit';
import { ChatController } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
import { SafeArea } from '../ui/SafeArea';
import { AppTheme } from '../ui/theme/Theme';

/**
 * `+ input Send` composer. The leading affordance is a capability menu, not a
 * permanent toolbar of runtime features.
 */
@Component
export struct Composer {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;

  /** Accessibility labels take plain strings, so resources are resolved. */
  private a11y(resource: Resource): string {
    return Copy.resolve(this.getUIContext().getHostContext() as common.Context, resource);
  }

  build() {
    Row({ space: 8 }) {
      // Fixed-size buttons drop the default label padding, otherwise the
      // label is squeezed out of the button and renders as a blank circle.
      // TEXTUAL style is the platform's name for an icon with no fill; the
      // default EMPHASIZED style paints a disc here, and a disabled
      // attachment button has no state that disc could be reporting.
      Button({ type: ButtonType.Circle, buttonStyle: ButtonStyleMode.TEXTUAL }) {
        SymbolGlyph($r('sys.symbol.plus'))
          .fontSize(20)
          .fontColor([this.theme.textSecondary]);
      }
        .width(40)
        .height(40)
        .enabled(false)
        .accessibilityText(this.a11y($r('app.string.a11y_add_attachment')));

      TextArea({ text: this.controller.draft, placeholder: $r('app.string.composer_placeholder') })
        .layoutWeight(1)
        .maxLines(6)
        .borderRadius(18)
        .backgroundColor(this.theme.surface)
        .fontColor(this.theme.textPrimary)
        .placeholderColor(this.theme.textSecondary)
        .onChange((value: string) => {
          this.controller.setDraft(value);
        });

      if (this.controller.busy) {
        // Stop replaces send in the same slot, so it keeps the same shape and
        // is named by its glyph rather than by a red field. Cancelling is not
        // a destructive action and must not look like one.
        Button({ type: ButtonType.Circle, buttonStyle: ButtonStyleMode.TEXTUAL }) {
          SymbolGlyph($r('sys.symbol.stop_circle'))
            .fontSize(20)
            .fontColor([this.theme.danger]);
        }
          .width(40)
          .height(40)
          .accessibilityText(this.a11y($r('app.string.a11y_stop')))
          .onClick(() => this.controller.stop());
      } else {
        // Send is the one primary action in the chat surface, so it is the one
        // place the emphasis colour is painted.
        Button({ type: ButtonType.Circle }) {
          SymbolGlyph($r('sys.symbol.arrow_up'))
            .fontSize(20)
            .fontColor([this.theme.textOnAccent]);
        }
          .width(40)
          .height(40)
          .backgroundColor(this.theme.accent)
          .accessibilityText(this.a11y($r('app.string.a11y_send')))
          .enabled(this.controller.canSend)
          .onClick(() => this.controller.send());
      }
    }
    .width('100%')
    .alignItems(VerticalAlign.Bottom)
    /**
     * The inset goes on the row, not on the composer.
     *
     * The composer's background still reaches the physical bottom edge — that
     * is what full screen bought, and it is why the band below the row is our
     * own colour rather than a strip the window set aside. What the inset buys
     * is that the navigation indicator, which is drawn over whatever it lands
     * on, lands on background instead of on the text field. Measured, that
     * difference is the pill sitting across the bottom edge of the input rather
     * than below it — tried the other way round and screenshotted it.
     *
     * So the bar gives way to the page, and the page still keeps its controls
     * clear. Those are different jobs and one inset cannot do both.
     */
    .padding({ left: 12, right: 12, top: 8, bottom: SafeArea.bottomVp + 8 })
    .backgroundColor(this.theme.pageBackground);
  }
}
