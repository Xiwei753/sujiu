import { ChatController, ChatMessageItem, isBusy, statusLabel } from '../presentation/ChatController';
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

  build() {
    List({ space: 12 }) {
      ForEach(this.controller.messages, (message: ChatMessageItem) => {
        ListItem() {
          MessageItemView({
            message: message,
            onCopy: (item: ChatMessageItem) => this.controller.copyMessage(item)
          });
        }
      }, (message: ChatMessageItem) => message.id);

      if (isBusy(this.controller.generationState)) {
        ListItem() {
          Row({ space: 8 }) {
            LoadingProgress().width(16).height(16);
            Text(statusLabel(this.controller.generationState))
              .fontSize(12)
              .fontColor('#8A8A99');
          }
          .width('100%')
          .alignItems(VerticalAlign.Center);
        }
      }

      if (this.controller.errorMessage.length > 0) {
        ListItem() {
          Text(this.controller.errorMessage)
            .fontSize(13)
            .fontColor('#B3261E')
            .width('100%')
            .padding(12)
            .borderRadius(12)
            .backgroundColor('#22B3261E');
        }
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: 16, right: 16, top: 12, bottom: 12 })
    .scrollBar(BarState.Auto);
  }
}
