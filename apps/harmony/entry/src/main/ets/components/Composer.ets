import { common } from '@kit.AbilityKit';
import { ChatController } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
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
      Button() {
        Text('+')
          .fontSize(20)
          .fontColor(this.theme.textSecondary);
      }
        .width(40)
        .height(40)
        .padding(0)
        .borderRadius(20)
        .backgroundColor(this.theme.surfaceMuted)
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
        Button() {
          Text('■')
            .fontSize(16)
            .fontColor(this.theme.danger);
        }
          .width(40)
          .height(40)
          .padding(0)
          .borderRadius(20)
          .backgroundColor(this.theme.dangerSurface)
          .accessibilityText(this.a11y($r('app.string.a11y_stop')))
          .onClick(() => this.controller.stop());
      } else {
        Button() {
          Text('↑')
            .fontSize(20)
            .fontColor(this.theme.textOnAccent);
        }
          .width(40)
          .height(40)
          .padding(0)
          .borderRadius(20)
          .backgroundColor(this.theme.accent)
          .accessibilityText(this.a11y($r('app.string.a11y_send')))
          .enabled(this.controller.canSend)
          .onClick(() => this.controller.send());
      }
    }
    .width('100%')
    .alignItems(VerticalAlign.Bottom)
    .padding({ left: 12, right: 12, top: 8, bottom: 8 })
    .backgroundColor(this.theme.pageBackground);
  }
}
