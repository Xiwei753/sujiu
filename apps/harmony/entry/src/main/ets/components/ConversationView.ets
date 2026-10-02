import { ChatController, ChatMessageItem, isBusy } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
import { AppTheme } from '../ui/theme/Theme';
import { MessageItemView } from './MessageViews';

/**
 * The conversation canvas.
 *
 * Tool calls and thinking are collapsed by default; the busy indicator and the
 * failure message are the only generation feedback the chat surface needs.
 */
@Component
export struct ConversationView {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;

  build() {
    List({ space: 12 }) {
      ForEach(this.controller.messages, (message: ChatMessageItem) => {
        ListItem() {
          MessageItemView({
            message: message,
            theme: this.theme,
            onCopy: (item: ChatMessageItem): void => {
              this.controller.copyMessage(item);
            }
          });
        }
      }, (message: ChatMessageItem) => message.id);

      if (isBusy(this.controller.generationState)) {
        ListItem() {
          Row({ space: 8 }) {
            LoadingProgress().width(16).height(16);
            Text(Copy.generationState(this.controller.generationState))
              .fontSize(12)
              .fontColor(this.theme.textSecondary);
          }
          .width('100%')
          .alignItems(VerticalAlign.Center);
        }
      }

      if (this.controller.errorCode.length > 0) {
        ListItem() {
          // The failure is stated in words, so the platform emphasis colour is
          // enough. There is no error card to paint: that was a second palette
          // pretending to be a system surface.
          Row({ space: 8 }) {
            SymbolGlyph($r('sys.symbol.info_circle'))
              .fontSize(16)
              .fontColor([this.theme.danger]);
            Text(Copy.errorText(this.controller.errorCode, this.controller.errorDetail))
              .fontSize(13)
              .fontColor(this.theme.danger)
              .layoutWeight(1);
          }
          .width('100%');
        }
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: 16, right: 16, top: 12, bottom: 12 })
    .scrollBar(BarState.Auto)
    .backgroundColor(this.theme.pageBackground);
  }
}
