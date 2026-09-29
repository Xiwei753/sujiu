import { SessionSummary } from '../bridge/SujiuBridge';
import { ChatController, SessionGroup } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';

/**
 * History list, used as the narrow-screen drawer and as the permanent sidebar
 * on wide screens. Grouping comes from presentation; the group header and its
 * sessions are rendered as one block so they never drift apart.
 */
@Component
export struct HistoryPanel {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onPick: () => void = () => {
  };

  @Builder
  private sessionRow(session: SessionSummary) {
    Column({ space: 2 }) {
      Text(session.title)
        .fontSize(session.id === this.controller.currentSessionId ? 15 : 14)
        .fontWeight(
          session.id === this.controller.currentSessionId ? FontWeight.Medium : FontWeight.Normal
        )
        .fontColor(this.theme.textPrimary)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .width('100%');
      Text(session.characterName.length > 0 ? session.characterName : 'No character yet')
        .fontSize(11)
        .fontColor(this.theme.textSecondary)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .width('100%');
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .padding({ left: 16, right: 16, top: 8, bottom: 8 })
    .onClick(() => {
      this.controller.openSession(session.id);
      this.onPick();
    });
  }

  @Builder
  private groupBlock(group: SessionGroup) {
    Column() {
      Text(group.title.toUpperCase())
        .fontSize(11)
        .fontColor(this.theme.textSecondary)
        .width('100%')
        .padding({ left: 16, top: 10, bottom: 4 });
      ForEach(group.sessions, (session: SessionSummary) => {
        this.sessionRow(session);
      }, (session: SessionSummary) => session.id);
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start);
  }

  build() {
    Column() {
      Row() {
        Text('Chats')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.theme.textPrimary)
          .layoutWeight(1);
        // Custom content plus zero padding: a fixed-size Button keeps the
        // default label padding, which leaves no room for the label and
        // silently renders an empty circle.
        Button() {
          Text('+')
            .fontSize(18)
            .fontColor(this.theme.textPrimary);
        }
          .width(32)
          .height(32)
          .padding(0)
          .borderRadius(16)
          .backgroundColor(this.theme.surfaceMuted)
          .accessibilityText('New chat')
          .onClick(() => {
            this.controller.newSession();
            this.onPick();
          });
      }
      .width('100%')
      .padding({ left: 16, right: 12, top: 12, bottom: 8 });

      List({ space: 0 }) {
        ForEach(this.controller.sessionGroups, (group: SessionGroup) => {
          ListItem() {
            this.groupBlock(group);
          }
        }, (group: SessionGroup) => group.title);
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Auto);
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .backgroundColor(this.theme.pageBackground);
  }
}
