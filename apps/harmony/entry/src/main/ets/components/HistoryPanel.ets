import { common } from '@kit.AbilityKit';
import { SessionSummary } from '../bridge/SujiuBridge';
import { ChatController, SessionGroup } from '../presentation/ChatController';
import { Copy } from '../ui/Copy';
import { SafeArea } from '../ui/SafeArea';
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

  /** Accessibility labels take plain strings, so resources are resolved. */
  private a11y(resource: Resource): string {
    return Copy.resolve(this.getUIContext().getHostContext() as common.Context, resource);
  }

  @Builder
  private sessionRow(session: SessionSummary) {
    Column({ space: 2 }) {
      Text(Copy.sessionTitle(session.title, session.characterName))
        .fontSize(session.id === this.controller.currentSessionId ? 15 : 14)
        .fontWeight(
          session.id === this.controller.currentSessionId ? FontWeight.Medium : FontWeight.Normal
        )
        .fontColor(this.theme.textPrimary)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .width('100%');
      Text(
        session.characterName.length > 0 ? session.characterName : $r('app.string.history_no_character')
      )
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
      Text(Copy.groupTitle(group.key))
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
        Text($r('app.string.history_title'))
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
          .accessibilityText(this.a11y($r('app.string.a11y_new_chat')))
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
        }, (group: SessionGroup) => group.key);
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Auto);
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    // Full screen means this panel reaches under the status bar and the
    // navigation indicator. The padding goes on the panel rather than around
    // it so the background still fills edge to edge and only the contents step
    // clear: a drawer whose title sits under the clock looks like a drawer with
    // no title.
    .padding({ top: SafeArea.topVp, bottom: SafeArea.bottomVp })
    .backgroundColor(this.theme.pageBackground);
  }
}
