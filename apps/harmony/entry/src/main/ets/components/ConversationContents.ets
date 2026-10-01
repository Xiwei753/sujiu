/*
 * What one conversation uses.
 *
 * This is the other half of the library split, and it exists as its own screen
 * because the two questions are different questions. The library answers "what
 * have I written?"; this answers "which of those is this chat using?". A screen
 * that did both would have to copy resources into the conversation to edit them,
 * and a copy is a second persona that stops being the first one the moment
 * either is changed.
 *
 * So nothing here writes a resource. Every control here is a choice among things
 * that already exist, and every choice is sent as an id.
 *
 * The transcript is not touched by anything on this screen. Changing who is in
 * the room is not rewriting what has already been said in it.
 */
import { CharacterSummary, PersonaSummary, PromptProfileSummary, WorldBookSummary } from '../bridge/SujiuBridge';
import { ChatController } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';
import { SafeArea } from '../ui/SafeArea';

/** A section label inside the binding page. */
@Component
struct BindingSection {
  @Prop label: ResourceStr = '';
  @ObjectLink theme: AppTheme;

  build() {
    Text(this.label)
      .fontSize(11)
      .fontColor(this.theme.textSecondary)
      .width('100%')
      .margin({ top: 8 });
  }
}

/**
 * One pickable stored resource, with its bound state.
 *
 * A checkmark rather than a filled-in look, and the name comes from the stored
 * resource. Binding resolves ids to names for display and for nothing else: if
 * this component held a copy of a persona, the two would be able to disagree.
 */
@Component
struct BindingRow {
  @Prop name: string = '';
  @Prop bound: boolean = false;
  @Prop detail: ResourceStr = '';
  @ObjectLink theme: AppTheme;
  onToggle: () => void = () => {};

  build() {
    Row({ space: 12 }) {
      Column({ space: 3 }) {
        Text(this.name)
          .fontSize(15)
          .fontColor(this.theme.textPrimary)
          .width('100%')
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis });
        if (this.detail !== '') {
          Text(this.detail)
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%')
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis });
        }
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1);
      Text(this.bound ? '✓' : '')
        .fontSize(16)
        .fontColor(this.theme.textPrimary)
        .width(24)
        .textAlign(TextAlign.Center);
    }
    .width('100%')
    .padding(14)
    .borderRadius(12)
    .backgroundColor(this.theme.surface)
    .onClick(() => this.onToggle());
  }
}

@Component
export struct ConversationContents {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onBack: () => void = () => {};

  @State private participantIds: string[] = [];
  @State private personaId: string = '';
  @State private worldBookIds: string[] = [];
  @State private promptProfileId: string = '';

  aboutToAppear(): void {
    // The stored bindings are read, not the controller's defaults: the runtime is
    // the only thing that knows what this conversation actually uses, and a
    // screen prefilled from anything else would offer to change a choice the user
    // never made.
    this.controller.loadLibrary();
    this.participantIds = this.controller.conversationParticipants.map(
      (item) => item.characterId);
    this.personaId = this.controller.conversationPersonaId;
    this.worldBookIds = this.controller.conversationWorldbookIds;
    this.promptProfileId = this.controller.conversationPromptProfileId;
  }

  private toggle(list: string[], id: string): string[] {
    return list.includes(id) ? list.filter((item: string) => item !== id) : list.concat([id]);
  }

  /** Single choice: picking one clears the other, because there is only one. */
  private chooseOne(current: string, id: string): string {
    return current === id ? '' : id;
  }

  private apply(): void {
    this.controller.bindResources(
      this.participantIds,
      this.personaId,
      this.worldBookIds,
      this.promptProfileId);
    this.onBack();
  }

  build() {
    Column() {
      Row({ space: 8 }) {
        Text('←')
          .fontSize(20)
          .fontColor(this.theme.textPrimary)
          .width(40)
          .height(40)
          .textAlign(TextAlign.Center)
          .onClick(() => this.onBack());
        Text($r('app.string.binding_title'))
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.theme.textPrimary)
          .layoutWeight(1);
      }
      .width('100%')
      .padding({ top: SafeArea.topVp, left: 12, right: 12, bottom: 12 })
      .backgroundColor(this.theme.pageBackground);

      Scroll() {
        Column({ space: 8 }) {
          Text($r('app.string.binding_intro'))
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%');

          BindingSection({ theme: this.theme, label: $r('app.string.binding_participants') });
          if (this.controller.characters.length === 0) {
            Text($r('app.string.binding_empty'))
              .fontSize(13)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          } else {
            ForEach(this.controller.characters, (character: CharacterSummary) => {
              BindingRow({
                theme: this.theme,
                name: character.name,
                bound: this.participantIds.includes(character.id),
                onToggle: () => {
                  this.participantIds = this.toggle(this.participantIds, character.id);
                }
              });
            }, (character: CharacterSummary) => character.id);
          }

          BindingSection({ theme: this.theme, label: $r('app.string.binding_persona') });
          if (this.controller.personas.length === 0) {
            Text($r('app.string.binding_empty'))
              .fontSize(13)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          } else {
            ForEach(this.controller.personas, (persona: PersonaSummary) => {
              BindingRow({
                theme: this.theme,
                name: persona.name,
                bound: this.personaId === persona.id,
                onToggle: () => {
                  this.personaId = this.chooseOne(this.personaId, persona.id);
                }
              });
            }, (persona: PersonaSummary) => persona.id);
          }

          BindingSection({ theme: this.theme, label: $r('app.string.binding_world_books') });
          if (this.controller.worldBooks.length === 0) {
            Text($r('app.string.binding_empty'))
              .fontSize(13)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          } else {
            ForEach(this.controller.worldBooks, (book: WorldBookSummary) => {
              BindingRow({
                theme: this.theme,
                name: book.name,
                detail: $r('app.string.library_entry_count', book.entries.length),
                bound: this.worldBookIds.includes(book.id),
                onToggle: () => {
                  this.worldBookIds = this.toggle(this.worldBookIds, book.id);
                }
              });
            }, (book: WorldBookSummary) => book.id);
          }

          BindingSection({ theme: this.theme, label: $r('app.string.binding_prompts') });
          if (this.controller.promptProfiles.length === 0) {
            Text($r('app.string.binding_empty'))
              .fontSize(13)
              .fontColor(this.theme.textSecondary)
              .width('100%');
          } else {
            ForEach(this.controller.promptProfiles, (profile: PromptProfileSummary) => {
              BindingRow({
                theme: this.theme,
                name: profile.name,
                bound: this.promptProfileId === profile.id,
                onToggle: () => {
                  this.promptProfileId = this.chooseOne(this.promptProfileId, profile.id);
                }
              });
            }, (profile: PromptProfileSummary) => profile.id);
          }
        }
        .width('100%')
        .padding(16);
      }
      .layoutWeight(1)
      .width('100%');

      Row() {
        Button($r('app.string.binding_apply'))
          .width('100%')
          .height(46)
          .backgroundColor(this.theme.accent)
          .fontColor(this.theme.textOnAccent)
          .enabled(!this.controller.libraryBusy)
          .onClick(() => this.apply());
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 12, bottom: SafeArea.bottomVp + 12 })
      .backgroundColor(this.theme.pageBackground);
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}