import { ChatController } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';

/**
 * `+ input Send` composer. The leading affordance is a capability menu, not a
 * permanent toolbar of runtime features.
 */
@Component
export struct Composer {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;

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
        .accessibilityText('Add attachment');

      TextArea({ text: this.controller.draft, placeholder: 'Message' })
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
