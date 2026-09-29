import { SessionSummary } from '../bridge/SujiuBridge';
import { ChatController, SessionGroup } from '../presentation/ChatController';

/**
 * History list, used both as the narrow-screen sheet and as the permanent
 * sidebar on wide screens. Grouping comes from presentation.
 */
@Component
export struct HistoryPanel {
  @ObjectLink controller: ChatController;
  onPick: () => void = () => {
  };

  build() {
    Column() {
      Row() {
        Text('Chats')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .layoutWeight(1);
        Button('+')
          .width(32)
          .height(32)
          .borderRadius(16)
          .backgroundColor('#22FFFFFF')
          .fontColor('#E8E8F0')
          .onClick(() => {
            this.controller.newSession();
            this.onPick();
          });
      }
      .width('100%')
      .padding({ left: 16, right: 12, top: 12, bottom: 8 });

      List({ space: 4 }) {
        ForEach(this.controller.sessionGroups, (group: SessionGroup) => {
          ListItem() {
            Text(group.title.toUpperCase())
              .fontSize(11)
              .fontColor('#8A8A99')
              .width('100%')
              .padding({ left: 16, top: 10, bottom: 4 });
          }
        }, (group: SessionGroup) => group.title);

        ForEach(this.controller.sessionGroups, (group: SessionGroup) => {
          ForEach(group.sessions, (session: SessionSummary) => {
            ListItem() {
              Column({ space: 2 }) {
                Text(session.title)
                  .fontSize(session.id === this.controller.currentSessionId ? 15 : 14)
                  .fontWeight(
                    session.id === this.controller.currentSessionId ? FontWeight.Medium : FontWeight.Normal
                  )
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .width('100%');
                Text(session.characterName)
                  .fontSize(11)
                  .fontColor('#8A8A99')
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
          }, (session: SessionSummary) => session.id);
        }, (group: SessionGroup) => group.title);
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Auto);
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start);
  }
}
