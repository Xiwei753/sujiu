import { ChatController } from '../presentation/ChatController';

/**
 * `+ input Send` composer. The leading affordance is a capability menu, not a
 * permanent toolbar of runtime features.
 */
@Component
export struct Composer {
  @ObjectLink controller: ChatController;

  build() {
    Row({ space: 8 }) {
      Button('+')
        .width(40)
        .height(40)
        .borderRadius(20)
        .backgroundColor('#22FFFFFF')
        .fontColor('#E8E8F0')
        .enabled(false)
        .accessibilityText('Add attachment');

      TextArea({ text: this.controller.draft, placeholder: 'Message' })
        .layoutWeight(1)
        .maxLines(6)
        .borderRadius(18)
        .backgroundColor('#1AFFFFFF')
        .onChange((value: string) => {
          this.controller.setDraft(value);
        });

      if (this.controller.busy) {
        Button('■')
          .width(40)
          .height(40)
          .borderRadius(20)
          .backgroundColor('#22B3261E')
          .fontColor('#B3261E')
          .onClick(() => this.controller.stop());
      } else {
        Button('↑')
          .width(40)
          .height(40)
          .borderRadius(20)
          .backgroundColor('#3D5AFE')
          .fontColor('#FFFFFF')
          .enabled(this.controller.canSend)
          .onClick(() => this.controller.send());
      }
    }
    .width('100%')
    .alignItems(VerticalAlign.Bottom)
    .padding({ left: 12, right: 12, top: 8, bottom: 8 });
  }
}
