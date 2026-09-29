import { ChatController, ChatMessageItem, isBusy, statusLabel } from '../presentation/ChatController';
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
            Text(statusLabel(this.controller.generationState))
              .fontSize(12)
              .fontColor(this.theme.textSecondary);
          }
          .width('100%')
          .alignItems(VerticalAlign.Center);
        }
      }

      if (this.controller.errorMessage.length > 0) {
        ListItem() {
          Text(this.controller.errorMessage)
            .fontSize(13)
            .fontColor(this.theme.danger)
            .width('100%')
            .padding(12)
            .borderRadius(12)
            .backgroundColor(this.theme.dangerSurface);
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
