/*
 * The library: the four stored resource lists.
 *
 * This page answers one question — what do I have? — and nothing else. Which of
 * them a conversation uses is answered somewhere else, in the conversation's own
 * contents screen, and no row here can change that. Keeping the two apart is the
 * reason a persona bound to three chats is one persona rather than three copies
 * that drift apart the first time any of them is edited.
 *
 * Four separate lists rather than one combined list, because the domain has four
 * separate resources. A single list with a type column would have to invent a
 * common shape for a character, a persona, a world book and a prompt set, and the
 * fields that do not apply to one of them are exactly what a "one resource" type
 * ends up with.
 */
import { CharacterSummary, PersonaSummary, PromptProfileSummary, WorldBookSummary } from '../bridge/SujiuBridge';
import { ChatController } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';
import { SafeArea } from '../ui/SafeArea';

/**
 * The back arrow and title every pushed page shares.
 *
 * Exported rather than duplicated per file because a header that exists twice
 * drifts: one gets the safe-area padding fixed and the other keeps the bug, and
 * the page that still has the bug looks like a page whose header is under the
 * clock.
 */
@Component
export struct PageHeader {
  @Prop title: ResourceStr = '';
  @ObjectLink theme: AppTheme;
  onBack: () => void = () => {};

  build() {
    Row({ space: 8 }) {
      Text('←')
        .fontSize(20)
        .fontColor(this.theme.textPrimary)
        .width(40)
        .height(40)
        .textAlign(TextAlign.Center)
        .onClick(() => this.onBack());
      Text(this.title)
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor(this.theme.textPrimary)
        .layoutWeight(1)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis });
    }
    .width('100%')
    .padding({ top: SafeArea.topVp, left: 12, right: 12, bottom: 12 })
    .backgroundColor(this.theme.pageBackground);
  }
}

/**
 * One row in a resource list.
 *
 * The trailing count is the row's secondary fact, not decoration: "4 entries" is
 * what tells a user which of two world books to open, and a list that shows only
 * names makes them open both.
 */
@Component
struct ResourceRow {
  @Prop name: string = '';
  @Prop detail: ResourceStr = '';
  @ObjectLink theme: AppTheme;
  onOpen: () => void = () => {};

  build() {
    Row({ space: 12 }) {
      Column({ space: 3 }) {
        Text(this.name)
          .fontSize(15)
          .fontColor(this.theme.textPrimary)
          .width('100%')
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis });
        // Compared as a resource rather than measured, so a localized detail
        // line can be passed in directly instead of being resolved to a string
        // by every caller that has one.
        if (this.detail !== '') {
          Text(this.detail)
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%')
            .maxLines(2)
            .textOverflow({ overflow: TextOverflow.Ellipsis });
        }
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1);
    }
    .width('100%')
    .padding(16)
    .borderRadius(14)
    .backgroundColor(this.theme.surface)
    .onClick(() => this.onOpen());
  }
}

/**
 * The "new" and "import" controls at the top of a list.
 *
 * Import explains that it is not available here rather than being absent and
 * rather than being a control that does nothing. The issue the library answers
 * lists import as one of its entry points, so leaving it out would read as an
 * oversight and wiring it to an empty handler would read as a bug; a label that
 * answers when pressed is the only one of those three that is true.
 */
@Component
struct LibraryToolbar {
  @Prop newLabel: ResourceStr = '';
  @ObjectLink theme: AppTheme;
  @State private noteShown: boolean = false;
  onNew: () => void = () => {};

  build() {
    Column({ space: 8 }) {
      Row({ space: 12 }) {
        Text(this.newLabel)
          .fontSize(15)
          .fontColor(this.theme.textPrimary)
          .padding({ left: 16, right: 16, top: 10, bottom: 10 })
          .borderRadius(20)
          .backgroundColor(this.theme.surface)
          .onClick(() => this.onNew());
        Text($r('app.string.library_import'))
          .fontSize(15)
          .fontColor(this.theme.textSecondary)
          .padding({ left: 16, right: 16, top: 10, bottom: 10 })
          .onClick(() => {
            this.noteShown = !this.noteShown;
          });
      }
      .width('100%');

      if (this.noteShown) {
        Text($r('app.string.library_import_note'))
          .fontSize(12)
          .fontColor(this.theme.textSecondary)
          .width('100%');
      }
    }
    .width('100%');
  }
}

/** One entry on the library's front page: a section, its count, and a row. */
@Component
struct HubRow {
  @Prop label: ResourceStr = '';
  @Prop count: number = 0;
  @ObjectLink theme: AppTheme;
  onOpen: () => void = () => {};

  build() {
    Row({ space: 12 }) {
      Text(this.label)
        .fontSize(16)
        .fontColor(this.theme.textPrimary)
        .layoutWeight(1);
      Text(`${this.count}`)
        .fontSize(14)
        .fontColor(this.theme.textSecondary)
        .width(28)
        .textAlign(TextAlign.End);
      Text('›')
        .fontSize(18)
        .fontColor(this.theme.textSecondary)
        .width(12)
        .textAlign(TextAlign.End);
    }
    .width('100%')
    .padding(16)
    .borderRadius(14)
    .backgroundColor(this.theme.surface)
    .onClick(() => this.onOpen());
  }
}

@Component
export struct LibraryHub {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onBack: () => void = () => {};
  onOpenCharacters: () => void = () => {};
  onOpenPersonas: () => void = () => {};
  onOpenWorldBooks: () => void = () => {};
  onOpenPrompts: () => void = () => {};

  aboutToAppear(): void {
    // Reloaded on every appearance rather than once at launch, so returning from
    // an edit shows what was actually saved instead of what was there before.
    this.controller.loadLibrary();
  }

  build() {
    Column() {
      PageHeader({ theme: this.theme, title: $r('app.string.library_title'), onBack: () => this.onBack() });

      Scroll() {
        Column({ space: 10 }) {
          Text($r('app.string.library_intro'))
            .fontSize(12)
            .fontColor(this.theme.textSecondary)
            .width('100%')
            .margin({ bottom: 4 });

          HubRow({
            theme: this.theme,
            label: $r('app.string.library_section_characters'),
            count: this.controller.characters.length,
            onOpen: () => this.onOpenCharacters()
          });
          HubRow({
            theme: this.theme,
            label: $r('app.string.library_section_personas'),
            count: this.controller.personas.length,
            onOpen: () => this.onOpenPersonas()
          });
          HubRow({
            theme: this.theme,
            label: $r('app.string.library_section_world_books'),
            count: this.controller.worldBooks.length,
            onOpen: () => this.onOpenWorldBooks()
          });
          HubRow({
            theme: this.theme,
            label: $r('app.string.library_section_prompts'),
            count: this.controller.promptProfiles.length,
            onOpen: () => this.onOpenPrompts()
          });
        }
        .width('100%')
        .padding(16);
      }
      .layoutWeight(1)
      .width('100%');
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}

@Component
export struct CharacterLibrary {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onBack: () => void = () => {};
  onOpenCharacter: (id: string) => void = (_id: string) => {};
  onNew: () => void = () => {};

  aboutToAppear(): void {
    this.controller.loadLibrary();
    this.controller.searchCharacters(this.controller.characterQuery);
  }

  build() {
    Column() {
      PageHeader({ theme: this.theme, title: $r('app.string.library_section_characters'), onBack: () => this.onBack() });

      Column({ space: 12 }) {
        LibraryToolbar({
          theme: this.theme,
          newLabel: $r('app.string.library_new_character'),
          onNew: () => this.onNew()
        });

        if (this.controller.characters.length === 0) {
          Text($r('app.string.library_empty_characters'))
            .fontSize(13)
            .fontColor(this.theme.textSecondary)
            .width('100%');
        }

        List({ space: 10 }) {
          ForEach(this.controller.characters, (character: CharacterSummary) => {
            ListItem() {
              ResourceRow({
                theme: this.theme,
                name: character.name,
                detail: character.description,
                onOpen: () => this.onOpenCharacter(character.id)
              });
            }
          }, (character: CharacterSummary) => character.id);
        }
        .width('100%')
        .layoutWeight(1);
      }
      .width('100%')
      .layoutWeight(1)
      .padding({ left: 16, right: 16, bottom: SafeArea.bottomVp + 12 });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}

@Component
export struct PersonaLibrary {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onBack: () => void = () => {};
  onOpenPersona: (id: string) => void = (_id: string) => {};
  onNew: () => void = () => {};

  aboutToAppear(): void {
    this.controller.loadLibrary();
  }

  build() {
    Column() {
      PageHeader({ theme: this.theme, title: $r('app.string.library_section_personas'), onBack: () => this.onBack() });

      Column({ space: 12 }) {
        LibraryToolbar({
          theme: this.theme,
          newLabel: $r('app.string.library_new_persona'),
          onNew: () => this.onNew()
        });

        if (this.controller.personas.length === 0) {
          Text($r('app.string.library_empty_personas'))
            .fontSize(13)
            .fontColor(this.theme.textSecondary)
            .width('100%');
        }

        List({ space: 10 }) {
          ForEach(this.controller.personas, (persona: PersonaSummary) => {
            ListItem() {
              ResourceRow({
                theme: this.theme,
                name: persona.name,
                detail: persona.description,
                onOpen: () => this.onOpenPersona(persona.id)
              });
            }
          }, (persona: PersonaSummary) => persona.id);
        }
        .width('100%')
        .layoutWeight(1);
      }
      .width('100%')
      .layoutWeight(1)
      .padding({ left: 16, right: 16, bottom: SafeArea.bottomVp + 12 });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}

@Component
export struct WorldBookLibrary {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onBack: () => void = () => {};
  onOpenWorldBook: (id: string) => void = (_id: string) => {};
  onNew: () => void = () => {};

  aboutToAppear(): void {
    this.controller.loadLibrary();
  }

  build() {
    Column() {
      PageHeader({ theme: this.theme, title: $r('app.string.library_section_world_books'), onBack: () => this.onBack() });

      Column({ space: 12 }) {
        LibraryToolbar({
          theme: this.theme,
          newLabel: $r('app.string.library_new_world_book'),
          onNew: () => this.onNew()
        });

        if (this.controller.worldBooks.length === 0) {
          Text($r('app.string.library_empty_world_books'))
            .fontSize(13)
            .fontColor(this.theme.textSecondary)
            .width('100%');
        }

        List({ space: 10 }) {
          ForEach(this.controller.worldBooks, (book: WorldBookSummary) => {
            ListItem() {
              ResourceRow({
                theme: this.theme,
                name: book.name,
                detail: $r('app.string.library_entry_count', book.entries.length),
                onOpen: () => this.onOpenWorldBook(book.id)
              });
            }
          }, (book: WorldBookSummary) => book.id);
        }
        .width('100%')
        .layoutWeight(1);
      }
      .width('100%')
      .layoutWeight(1)
      .padding({ left: 16, right: 16, bottom: SafeArea.bottomVp + 12 });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}

@Component
export struct PromptLibrary {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  onBack: () => void = () => {};
  onOpenProfile: (id: string) => void = (_id: string) => {};
  onNew: () => void = () => {};

  aboutToAppear(): void {
    this.controller.loadLibrary();
  }

  build() {
    Column() {
      PageHeader({ theme: this.theme, title: $r('app.string.library_section_prompts'), onBack: () => this.onBack() });

      Column({ space: 12 }) {
        LibraryToolbar({
          theme: this.theme,
          newLabel: $r('app.string.library_new_prompt'),
          onNew: () => this.onNew()
        });

        if (this.controller.promptProfiles.length === 0) {
          Text($r('app.string.library_empty_prompts'))
            .fontSize(13)
            .fontColor(this.theme.textSecondary)
            .width('100%');
        }

        List({ space: 10 }) {
          ForEach(this.controller.promptProfiles, (profile: PromptProfileSummary) => {
            ListItem() {
              ResourceRow({
                theme: this.theme,
                name: profile.name,
                onOpen: () => this.onOpenProfile(profile.id)
              });
            }
          }, (profile: PromptProfileSummary) => profile.id);
        }
        .width('100%')
        .layoutWeight(1);
      }
      .width('100%')
      .layoutWeight(1)
      .padding({ left: 16, right: 16, bottom: SafeArea.bottomVp + 12 });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}