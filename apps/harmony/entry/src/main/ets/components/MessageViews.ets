import { ChatMessageItem, ToolCallItem } from '../presentation/ChatController';

/**
 * Disclosure block for tool activity and thinking.
 *
 * Collapsed by default. The expanded flag is local view state: the runtime
 * never needs to know whether the user opened it.
 */
@Component
export struct CollapsibleSection {
  @Prop title: string = '';
  @Prop body: string = '';
  @State expanded: boolean = false;

  build() {
    Column() {
      Row({ space: 6 }) {
        Text(this.expanded ? '▾' : '▸').fontSize(14).fontColor('#888888');
        Text(this.title).fontSize(13).fontColor('#888888');
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.expanded = !this.expanded;
      })

      if (this.expanded) {
        Text(this.body)
          .fontSize(13)
          .fontColor('#888888')
          .width('100%')
          .margin({ top: 6 });
      }
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .padding(10)
    .borderRadius(10)
    .backgroundColor('#22FFFFFF');
  }
}

@Component
export struct MessageItemView {
  @ObjectLink message: ChatMessageItem;
  onCopy: (message: ChatMessageItem) => void = () => {
  };

  build() {
    Column({ space: 6 }) {
      if (this.message.thinking.length > 0) {
        CollapsibleSection({ title: 'Thinking', body: this.message.thinking });
      }
      if (this.message.hasToolCalls) {
        CollapsibleSection({
          title: `Tools (${this.message.toolCalls.length})`,
          body: this.message.toolCalls
            .map((call: ToolCallItem) => {
              return `${call.toolName}: ${call.statusLabel}${call.summary.length > 0 ? ' — ' + call.summary : ''}`;
            })
            .join('\n')
        });
      }
      if (this.message.text.trim().length > 0) {
        Text(this.message.text)
          .fontSize(16)
          .width('100%')
          .fontColor(this.message.isUser ? '#FFFFFF' : '#E8E8F0');
      }
      Text('Copy')
        .fontSize(11)
        .fontColor('#8A8A99')
        .onClick(() => this.onCopy(this.message));
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .padding(this.message.isUser ? 12 : 0)
    .borderRadius(14)
    .backgroundColor(this.message.isUser ? '#3D5AFE' : '#00000000');
  }
}
