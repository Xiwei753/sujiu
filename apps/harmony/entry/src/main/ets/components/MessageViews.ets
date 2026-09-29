import { common } from '@kit.AbilityKit';
import { ChatMessageItem, ToolCallItem, ToolCallStatus } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
import { AppTheme } from '../ui/theme/Theme';

/**
 * Disclosure block for tool activity and thinking.
 *
 * Collapsed by default. The expanded flag is local view state: the runtime
 * never needs to know whether the user opened it.
 */
@Component
export struct CollapsibleSection {
  @Prop title: ResourceStr = '';
  @Prop body: string = '';
  @ObjectLink theme: AppTheme;
  @State expanded: boolean = false;

  build() {
    Column() {
      Row({ space: 6 }) {
        Text(this.expanded ? '▾' : '▸').fontSize(14).fontColor(this.theme.textSecondary);
        Text(this.title).fontSize(13).fontColor(this.theme.textSecondary);
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.expanded = !this.expanded;
      });

      if (this.expanded) {
        Text(this.body)
          .fontSize(13)
          .fontColor(this.theme.textSecondary)
          .width('100%')
          .margin({ top: 6 });
      }
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .padding(10)
    .borderRadius(10)
    .backgroundColor(this.theme.surfaceMuted);
  }
}

@Component
export struct MessageItemView {
  @ObjectLink message: ChatMessageItem;
  @ObjectLink theme: AppTheme;
  onCopy: (message: ChatMessageItem) => void = () => {
  };

  private toolStatus(status: ToolCallStatus): string {
    return Copy.resolve(getContext(this) as common.Context, Copy.toolStatus(status) as Resource);
  }

  build() {
    Column({ space: 6 }) {
      if (this.message.thinking.length > 0) {
        CollapsibleSection({
          title: $r('app.string.message_thinking'),
          body: this.message.thinking,
          theme: this.theme
        });
      }
      if (this.message.hasToolCalls) {
        CollapsibleSection({
          title: $r('app.string.message_tools', this.message.toolCalls.length),
          body: this.message.toolCalls
            .map((call: ToolCallItem) => {
              const status: string = this.toolStatus(call.status);
              return call.summary.length > 0 ? `${call.toolName} — ${call.summary} (${status})` : `${call.toolName}: ${status}`;
            })
            .join('\n'),
          theme: this.theme
        });
      }
      if (this.message.text.trim().length > 0) {
        Text(this.message.text)
          .fontSize(16)
          .width('100%')
          .fontColor(this.theme.textPrimary);
      }
      Text($r('app.string.message_copy'))
        .fontSize(11)
        .fontColor(this.theme.textSecondary)
        .onClick(() => this.onCopy(this.message));
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .padding(this.message.isUser ? 12 : 0)
    .borderRadius(14)
    .backgroundColor(this.message.isUser ? this.theme.userBubble : '#00000000');
  }
}
