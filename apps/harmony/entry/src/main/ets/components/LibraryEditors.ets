/*
 * Editors for the four stored resources.
 *
 * The library owns these resources; a conversation only refers to them. Nothing
 * in this file writes a conversation, and nothing in a conversation screen edits
 * a resource. That split is the whole point of the page this file belongs to, and
 * it is easier to keep when the two halves live in different files.
 *
 * Each editor owns its own draft state and loads it in `aboutToAppear`, rather
 * than receiving a loaded object as a prop. The reason is the toolkit: a text
 * input's `text` is an initial value, so an input whose value is written from
 * outside — by opening the next resource, say — keeps showing the old one. Owning
 * the state means each editor is a fresh component per resource, and no value
 * ever arrives from anywhere but the user.
 */
import { CharacterDraft, WorldBookDraft, WorldBookEntryDraft, WorldBookSummary, PromptProfileDraft, PersonaDraft } from '../bridge/SujiuBridge';
import { ChatController } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';
import { SafeArea } from '../ui/SafeArea';
import { PageHeader } from './Library';

/**
 * One labelled input.
 *
 * `onValue` rather than two-way binding: the toolkit offers none, and a binding
 * faked with a hidden field writes back on every keystroke and loses IME
 * composition. Handing the new text up and letting the parent decide is what a
 * binding would do, minus the part that breaks candidate windows.
 */
@Component
struct EditorField {
  @Prop label: ResourceStr = '';
  @Prop value: string = '';
  @Prop tall: boolean = false;
  @Prop theme: AppTheme;
  onValue: (text: string) => void = (_text: string) => {};

  build() {
    Column({ space: 4 }) {
      Text(this.label)
        .fontSize(11)
        .fontColor(this.theme.textSecondary)
        .width('100%');
      if (this.tall) {
        TextArea({ text: this.value })
          .height(150)
          .fontSize(14)
          .fontColor(this.theme.textPrimary)
          .backgroundColor(this.theme.surface)
          .borderRadius(10)
          .onChange((text: string) => this.onValue(text));
      } else {
        TextInput({ text: this.value })
          .height(44)
          .fontSize(14)
          .fontColor(this.theme.textPrimary)
          .backgroundColor(this.theme.surface)
          .borderRadius(10)
          .onChange((text: string) => this.onValue(text));
      }
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%');
  }
}

/**
 * The save row at the bottom of an editor.
 *
 * Disabled while a write is in flight, because this button is the only thing
 * standing between a half-typed persona and a stored one, and a button that stays
 * live through a write lets a second tap queue a second create.
 *
 * Delete is a separate plain label rather than a second button, so the two
 * actions do not carry equal weight: saving is what an editor is for, and a
 * delete dressed as a button beside it is one mis-tap away from every save.
 */
@Component
struct EditorActions {
  @ObjectLink theme: AppTheme;
  @Prop busy: boolean = false;
  @Prop canDelete: boolean = false;
  onSave: () => void = () => {};
  onDelete: () => void = () => {};

  build() {
    Row({ space: 12 }) {
      if (this.canDelete) {
        Text($r('app.string.library_delete'))
          .fontSize(15)
          .fontColor(this.theme.dangerAccent)
          .padding({ top: 12, bottom: 12, left: 4, right: 4 })
          .onClick(() => this.onDelete());
      }
      Button($r('app.string.library_save'))
        .layoutWeight(1)
        .height(46)
        .backgroundColor(this.theme.accent)
        .fontColor(this.theme.textOnAccent)
        .enabled(!this.busy)
        .onClick(() => this.onSave());
    }
    .width('100%')
    .padding({ left: 16, right: 16, top: 12, bottom: SafeArea.bottomVp + 12 })
    .backgroundColor(this.theme.pageBackground);
  }
}

/** A short paragraph of explanation above a list of choices. */
@Component
struct EditorIntro {
  @Prop text: ResourceStr = '';
  @ObjectLink theme: AppTheme;

  build() {
    Text(this.text)
      .fontSize(12)
      .fontColor(this.theme.textSecondary)
      .width('100%');
  }
}

@Component
export struct CharacterEditor {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  @Prop characterId: string = '';
  onBack: () => void = () => {};

  @State private resourceId: string = '';
  @State private name: string = '';
  @State private description: string = '';
  @State private personality: string = '';
  @State private scenario: string = '';
  @State private firstMessage: string = '';
  @State private alternateGreetings: string = '';
  @State private exampleDialogue: string = '';
  @State private systemPrompt: string = '';
  @State private postHistory: string = '';

  aboutToAppear(): void {
    const wanted: string = this.characterId;
    if (wanted.length === 0) {
      return;
    }
    this.controller.loadCharacterDraft(wanted).then((draft: CharacterDraft | undefined) => {
      if (draft === undefined) {
        return;
      }
      this.resourceId = draft.id ?? '';
      this.name = draft.name;
      this.description = draft.description;
      this.personality = draft.personality;
      this.scenario = draft.scenario;
      this.firstMessage = draft.firstMessage;
      this.alternateGreetings = draft.alternateGreetings.join('\n');
      this.exampleDialogue = draft.exampleDialogue;
      this.systemPrompt = draft.systemPrompt;
      this.postHistory = draft.postHistoryInstructions;
    });
  }

  build() {
    Column() {
      PageHeader({
        title: this.resourceId.length > 0 ? this.name : $r('app.string.library_section_characters'),
        theme: this.theme,
        onBack: () => this.onBack()
      });

      Scroll() {
        Column({ space: 12 }) {
          EditorField({ label: $r('app.string.library_field_name'), theme: this.theme, value: this.name, onValue: (t: string) => this.name = t });
          EditorField({ label: $r('app.string.library_field_description'), theme: this.theme, value: this.description, tall: true, onValue: (t: string) => this.description = t });
          EditorField({ label: $r('app.string.library_field_personality'), theme: this.theme, value: this.personality, tall: true, onValue: (t: string) => this.personality = t });
          EditorField({ label: $r('app.string.library_field_scenario'), theme: this.theme, value: this.scenario, tall: true, onValue: (t: string) => this.scenario = t });
          EditorField({ label: $r('app.string.library_field_first_message'), theme: this.theme, value: this.firstMessage, tall: true, onValue: (t: string) => this.firstMessage = t });
          EditorField({ label: $r('app.string.library_field_alternate_greetings'), theme: this.theme, value: this.alternateGreetings, tall: true, onValue: (t: string) => this.alternateGreetings = t });
          EditorField({ label: $r('app.string.library_field_example_dialogue'), theme: this.theme, value: this.exampleDialogue, tall: true, onValue: (t: string) => this.exampleDialogue = t });
          EditorField({ label: $r('app.string.library_field_system_prompt'), theme: this.theme, value: this.systemPrompt, tall: true, onValue: (t: string) => this.systemPrompt = t });
          EditorField({ label: $r('app.string.library_field_post_history'), theme: this.theme, value: this.postHistory, tall: true, onValue: (t: string) => this.postHistory = t });
        }
        .width('100%')
        .padding(16);
      }
      .layoutWeight(1)
      .width('100%');

      EditorActions({
        theme: this.theme,
        busy: this.controller.libraryBusy,
        onSave: () => {
          this.controller.saveCharacter({
            id: this.resourceId,
            name: this.name,
            description: this.description,
            personality: this.personality,
            scenario: this.scenario,
            firstMessage: this.firstMessage,
            // One greeting per line. A greeting is prose, and a comma is a
            // perfectly normal thing to write inside one, so a comma-delimited
            // field is a field that will eventually split a greeting in half.
            alternateGreetings: this.alternateGreetings.split('\n').filter(
              (line: string) => line.trim().length > 0),
            exampleDialogue: this.exampleDialogue,
            systemPrompt: this.systemPrompt,
            postHistoryInstructions: this.postHistory,
            worldbookIds: []
          } as CharacterDraft);
          this.onBack();
        }
      });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}

@Component
export struct PersonaEditor {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  @Prop personaId: string = '';
  onBack: () => void = () => {};

  @State private resourceId: string = '';
  @State private name: string = '';
  @State private description: string = '';
  @State private userPrompt: string = '';

  aboutToAppear(): void {
    const wanted: string = this.personaId;
    if (wanted.length === 0) {
      return;
    }
    this.controller.loadPersonaDraft(wanted).then((draft: PersonaDraft | undefined) => {
      if (draft === undefined) {
        return;
      }
      this.resourceId = draft.id ?? '';
      this.name = draft.name;
      this.description = draft.description;
      this.userPrompt = draft.userPrompt;
    });
  }

  build() {
    Column() {
      PageHeader({
        title: this.resourceId.length > 0 ? this.name : $r('app.string.library_section_personas'),
        theme: this.theme,
        onBack: () => this.onBack()
      });

      Scroll() {
        Column({ space: 12 }) {
          EditorIntro({ theme: this.theme, text: $r('app.string.library_persona_intro') });
          EditorField({ label: $r('app.string.library_field_name'), theme: this.theme, value: this.name, onValue: (t: string) => this.name = t });
          EditorField({ label: $r('app.string.library_field_description'), theme: this.theme, value: this.description, tall: true, onValue: (t: string) => this.description = t });
          EditorField({ label: $r('app.string.library_field_user_prompt'), theme: this.theme, value: this.userPrompt, tall: true, onValue: (t: string) => this.userPrompt = t });
        }
        .width('100%')
        .padding(16);
      }
      .layoutWeight(1)
      .width('100%');

      EditorActions({
        theme: this.theme,
        busy: this.controller.libraryBusy,
        canDelete: this.resourceId.length > 0,
        onSave: () => {
          this.controller.savePersona({
            id: this.resourceId,
            name: this.name,
            description: this.description,
            userPrompt: this.userPrompt,
            worldbookIds: []
          } as PersonaDraft);
          this.onBack();
        },
        onDelete: () => {
          this.controller.deletePersona(this.resourceId);
          this.onBack();
        }
      });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}

@Component
export struct PromptEditor {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  @Prop profileId: string = '';
  onBack: () => void = () => {};

  @State private resourceId: string = '';
  @State private name: string = '';
  @State private systemPrompt: string = '';
  @State private userPrompt: string = '';
  @State private postHistory: string = '';
  @State private formatRules: string = '';

  aboutToAppear(): void {
    const wanted: string = this.profileId;
    if (wanted.length === 0) {
      return;
    }
    this.controller.loadPromptProfileDraft(wanted).then((draft: PromptProfileDraft | undefined) => {
      if (draft === undefined) {
        return;
      }
      this.resourceId = draft.id ?? '';
      this.name = draft.name;
      this.systemPrompt = draft.systemPrompt;
      this.userPrompt = draft.userPrompt;
      this.postHistory = draft.postHistoryInstructions;
      this.formatRules = draft.formatRules;
    });
  }

  build() {
    Column() {
      PageHeader({
        title: this.resourceId.length > 0 ? this.name : $r('app.string.library_section_prompts'),
        theme: this.theme,
        onBack: () => this.onBack()
      });

      Scroll() {
        Column({ space: 12 }) {
          EditorIntro({ theme: this.theme, text: $r('app.string.library_prompt_intro') });
          EditorField({ label: $r('app.string.library_field_name'), theme: this.theme, value: this.name, onValue: (t: string) => this.name = t });
          EditorField({ label: $r('app.string.library_field_system_prompt'), theme: this.theme, value: this.systemPrompt, tall: true, onValue: (t: string) => this.systemPrompt = t });
          EditorField({ label: $r('app.string.library_field_user_prompt'), theme: this.theme, value: this.userPrompt, tall: true, onValue: (t: string) => this.userPrompt = t });
          EditorField({ label: $r('app.string.library_field_post_history'), theme: this.theme, value: this.postHistory, tall: true, onValue: (t: string) => this.postHistory = t });
          EditorField({ label: $r('app.string.library_field_format_rules'), theme: this.theme, value: this.formatRules, tall: true, onValue: (t: string) => this.formatRules = t });
        }
        .width('100%')
        .padding(16);
      }
      .layoutWeight(1)
      .width('100%');

      EditorActions({
        theme: this.theme,
        busy: this.controller.libraryBusy,
        canDelete: this.resourceId.length > 0,
        onSave: () => {
          this.controller.savePromptProfile({
            id: this.resourceId,
            name: this.name,
            systemPrompt: this.systemPrompt,
            userPrompt: this.userPrompt,
            postHistoryInstructions: this.postHistory,
            formatRules: this.formatRules
          } as PromptProfileDraft);
          this.onBack();
        },
        onDelete: () => {
          this.controller.deletePromptProfile(this.resourceId);
          this.onBack();
        }
      });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}

/**
 * A world book and its entries.
 *
 * Entries are edited inline rather than in a child component, for one concrete
 * reason: a child that receives an entry as a prop keeps showing the value it was
 * built with, so the parent has to be told about every keystroke and then rebuild
 * the row to show a character the user already typed. Holding the array here and
 * writing back on change is the version with no such round trip.
 *
 * An entry's injection position is carried through untouched rather than offered
 * as a control. A position picker belongs in a full editor and this round is not
 * one, but the value still round-trips, so saving a book nobody was trying to
 * reposition does not quietly move everybody's lore.
 */
@Component
export struct WorldBookEditor {
  @ObjectLink controller: ChatController;
  @ObjectLink theme: AppTheme;
  @Prop worldBookId: string = '';
  onBack: () => void = () => {};

  @State private resourceId: string = '';
  @State private name: string = '';
  @State private entries: WorldBookEntryDraft[] = [];

  aboutToAppear(): void {
    const wanted: string = this.worldBookId;
    if (wanted.length === 0) {
      return;
    }
    const book: WorldBookSummary | undefined = this.controller.worldBookById(wanted);
    if (book === undefined) {
      return;
    }
    this.resourceId = book.id;
    this.name = book.name;
    const loaded: WorldBookEntryDraft[] = [];
    for (let i = 0; i < book.entries.length; i++) {
      const entry = book.entries[i];
      loaded.push({
        id: entry.id,
        name: entry.name,
        content: entry.content,
        keys: entry.keys,
        enabled: entry.enabled,
        constant: entry.constant,
        position: entry.position
      } as WorldBookEntryDraft);
    }
    this.entries = loaded;
  }

  /**
   * One field of one entry, rewritten in place.
   *
   * Written as four small functions rather than one that takes a field name,
   * because ArkTS forbids indexed access on an object and a generic version of
   * this needs it. The rebuild of the whole array is the point of the call: a
   * nested field written directly would not be observed, so the edit would be
   * visible in the model and not on the screen.
   */
  private editEntryName(index: number, text: string): void {
    this.entries = this.entries.map((entry: WorldBookEntryDraft, at: number) => {
      if (at !== index) {
        return entry;
      }
      return {
        id: entry.id, name: text, content: entry.content, keys: entry.keys,
        enabled: entry.enabled, constant: entry.constant, position: entry.position
      } as WorldBookEntryDraft;
    });
  }

  private editEntryContent(index: number, text: string): void {
    this.entries = this.entries.map((entry: WorldBookEntryDraft, at: number) => {
      if (at !== index) {
        return entry;
      }
      return {
        id: entry.id, name: entry.name, content: text, keys: entry.keys,
        enabled: entry.enabled, constant: entry.constant, position: entry.position
      } as WorldBookEntryDraft;
    });
  }

  /**
   * Keywords are short tokens the author types one after another, and a keyword
   * containing a comma is not a thing anybody writes, so commas are a safe
   * delimiter here — unlike the greetings in a character, which are prose.
   */
  private editEntryKeys(index: number, text: string): void {
    const keys: string[] = text.split(',').map((k: string) => k.trim()).filter(
      (k: string) => k.length > 0);
    this.entries = this.entries.map((entry: WorldBookEntryDraft, at: number) => {
      if (at !== index) {
        return entry;
      }
      return {
        id: entry.id, name: entry.name, content: entry.content, keys: keys,
        enabled: entry.enabled, constant: entry.constant, position: entry.position
      } as WorldBookEntryDraft;
    });
  }

  private editEntryEnabled(index: number, on: boolean): void {
    this.entries = this.entries.map((entry: WorldBookEntryDraft, at: number) => {
      if (at !== index) {
        return entry;
      }
      return {
        id: entry.id, name: entry.name, content: entry.content, keys: entry.keys,
        enabled: on, constant: entry.constant, position: entry.position
      } as WorldBookEntryDraft;
    });
  }

  private editEntryConstant(index: number, on: boolean): void {
    this.entries = this.entries.map((entry: WorldBookEntryDraft, at: number) => {
      if (at !== index) {
        return entry;
      }
      return {
        id: entry.id, name: entry.name, content: entry.content, keys: entry.keys,
        enabled: entry.enabled, constant: on, position: entry.position
      } as WorldBookEntryDraft;
    });
  }

  build() {
    Column() {
      PageHeader({
        title: this.resourceId.length > 0 ? this.name : $r('app.string.library_section_world_books'),
        theme: this.theme,
        onBack: () => this.onBack()
      });

      Scroll() {
        Column({ space: 12 }) {
          EditorIntro({ theme: this.theme, text: $r('app.string.library_world_book_intro') });
          EditorField({ label: $r('app.string.library_field_name'), theme: this.theme, value: this.name, onValue: (t: string) => this.name = t });

          ForEach(this.entries, (entry: WorldBookEntryDraft, index: number) => {
            Column({ space: 8 }) {
              EditorField({
                label: $r('app.string.library_field_name'),
                theme: this.theme,
                value: entry.name,
                onValue: (t: string) => this.editEntryName(index, t)
              });
              EditorField({
                label: $r('app.string.library_field_content'),
                theme: this.theme,
                value: entry.content,
                tall: true,
                onValue: (t: string) => this.editEntryContent(index, t)
              });
              EditorField({
                label: $r('app.string.library_field_keys'),
                theme: this.theme,
                value: entry.keys.join(', '),
                onValue: (t: string) => this.editEntryKeys(index, t)
              });
              Row({ space: 18 }) {
                Row({ space: 6 }) {
                  Toggle({ type: ToggleType.Switch, isOn: entry.enabled })
                    .width(40)
                    .onChange((on: boolean) => this.editEntryEnabled(index, on));
                  Text($r('app.string.library_field_enabled'))
                    .fontSize(13)
                    .fontColor(this.theme.textSecondary);
                }
                Row({ space: 6 }) {
                  Toggle({ type: ToggleType.Switch, isOn: entry.constant })
                    .width(40)
                    .onChange((on: boolean) => this.editEntryConstant(index, on));
                  Text($r('app.string.library_field_constant'))
                    .fontSize(13)
                    .fontColor(this.theme.textSecondary);
                }
              }
              .width('100%');

              Text($r('app.string.library_remove_entry'))
                .fontSize(14)
                .fontColor(this.theme.dangerAccent)
                .padding({ top: 4, bottom: 4 })
                .onClick(() => {
                  this.entries = this.entries.filter((_item: WorldBookEntryDraft, at: number) => at !== index);
                });
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)
            .padding(14)
            .borderRadius(12)
            .backgroundColor(this.theme.surface);
          }, (_entry: WorldBookEntryDraft, index: number) => `entry-${index}`);

          Text($r('app.string.library_add_entry'))
            .fontSize(15)
            .fontColor(this.theme.textPrimary)
            .padding(12)
            .onClick(() => {
              this.entries = this.entries.concat([{
                name: '',
                content: '',
                keys: [],
                enabled: true,
                constant: false,
                position: 'after_character'
              } as WorldBookEntryDraft]);
            });
        }
        .width('100%')
        .padding(16);
      }
      .layoutWeight(1)
      .width('100%');

      EditorActions({
        theme: this.theme,
        busy: this.controller.libraryBusy,
        canDelete: this.resourceId.length > 0,
        onSave: () => {
          this.controller.saveWorldBook({
            id: this.resourceId,
            name: this.name,
            entries: this.entries
          } as WorldBookDraft);
          this.onBack();
        },
        onDelete: () => {
          this.controller.deleteWorldBook(this.resourceId);
          this.onBack();
        }
      });
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.theme.pageBackground);
  }
}