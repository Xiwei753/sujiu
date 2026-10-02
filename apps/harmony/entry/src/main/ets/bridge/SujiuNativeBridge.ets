/*
 * Native application bridge.
 *
 * This is the HarmonyOS half of the bridge layer described in
 * docs/UI_ARCHITECTURE.md. It calls the shared Rust runtime through the sujiu
 * native module and normalizes the runtime turn events into the shared
 * SujiuBridge contract, so presentation and UI never learn anything about the
 * runtime, the provider or the FFI shape.
 *
 * The native module is produced by scripts/build-harmony-runtime.sh.
 */
import {
  create, SujiuRuntimeBridge, ParticipantDto, ParticipantRequestDto,
  SessionSummaryDto, CharacterSummaryDto, ModelSummaryDto, ContextSourceDto,
  MessageDto, ToolCallDto, ConversationSnapshotDto, PersonaDto, PersonaRequestDto,
  WorldBookDto, WorldBookEntryDto, WorldBookEntryRequestDto, WorldBookRequestDto, PromptProfileDto,
  PromptProfileRequestDto, CharacterRequestDto, ProviderConfigDto, TurnRequestDto,
  ModelDiscoveryDto, EndpointExplorationDto, DiagnosticEntryDto, StoredEndpointDto
} from 'libsujiu_napi.so';
import { SujiuBridge, SessionSummary, CharacterSummary, ModelSummary, ContextSourceSummary,
  ConversationSnapshot, MessageSummary, MessageRole, TurnEvent, TurnEventKind,
  TurnEventListener, ProviderDraft, EndpointExploration, ModelDiscovery,
  StoredEndpoint, DiagnosticEntry, PersonaSummary, PersonaDraft, WorldBookSummary,
  WorldBookEntry, WorldBookDraft, WorldBookEntryDraft, PromptProfileSummary,
  PromptProfileDraft, CharacterDraft, ParticipantBinding, ConversationParticipant } from './SujiuBridge';

/*
 * One turn event as the runtime delivers it.
 *
 * This is the one shape with no generated declaration, and the reason is worth
 * stating so nobody adds it back by hand later: a turn streams many events, so
 * `sendTurn` reports each one as a JSON string through a callback instead of
 * returning a typed value. There is nothing for napi-rs to generate a type
 * from, so the shape is spelled out here — once, next to the only function
 * that parses it, and deliberately named so it cannot be mistaken for a
 * generated DTO.
 */
interface RuntimeTurnEventPayload {
  kind: string;
  text: string;
  toolName: string;
  toolCallId: string;
  isError: boolean;
}

/**
 * Copies a caller-owned array into a plain one before it crosses into native.
 *
 * An array held in an ArkUI `@State` is a reactive proxy, not an `Array`.
 * `Array.isArray()` still answers true for it, the generated declaration still
 * types it as `Array<string>`, and the compiler accepts all of it — but napi
 * reads the object's real length field, finds none, and refuses the whole
 * payload with `Failed to get Array length`. Nothing local reports it: the call
 * simply never lands, so a save looks like it did nothing.
 *
 * That is not a hypothetical. Creating a persona on a device failed exactly this
 * way, and no compiler check, generated-declaration check or Rust unit test could
 * see it, because every one of them was looking at a value that was well-typed.
 *
 * So every array a caller owns goes through here on the way in. An array built
 * inside this file is already plain and needs nothing, but copying it is cheap
 * and there is then no list of sites to keep correct.
 */
function plainArray<T>(values: T[]): T[] {
  const copy: T[] = [];
  for (let i = 0; i < values.length; i++) {
    copy.push(values[i]);
  }
  return copy;
}

/**
 * Maps a runtime event kind onto the shared bridge kind.
 *
 * The runtime serializes its event kinds in snake_case, exactly as they are
 * spelled in `TurnEventKind` on the Rust side. That string is the wire contract,
 * so it must not be re-cased here.
 */
function toKind(value: string): TurnEventKind {
  switch (value) {
    case 'turn_started':
      return TurnEventKind.Started;
    case 'text_delta':
      return TurnEventKind.TextDelta;
    case 'thinking_delta':
      return TurnEventKind.ThinkingDelta;
    case 'tool_call_requested':
      return TurnEventKind.ToolCallRequested;
    case 'tool_call_started':
      return TurnEventKind.ToolCallStarted;
    case 'tool_call_finished':
      return TurnEventKind.ToolCallFinished;
    case 'turn_completed':
      return TurnEventKind.Completed;
    case 'turn_cancelled':
      return TurnEventKind.Cancelled;
    case 'turn_failed':
      return TurnEventKind.Failed;
    default:
      // An unknown kind is a contract mismatch, not a failed turn. It has to be
      // visible: a silent fallback here once made every turn look like an
      // immediate failure with no detail, because the first event was read as
      // a failure instead of as the start of the turn.
      return TurnEventKind.Failed;
  }
}

function toRole(value: string): MessageRole {
  if (value === MessageRole.User) {
    return MessageRole.User;
  }
  if (value === MessageRole.System) {
    return MessageRole.System;
  }
  return MessageRole.Assistant;
}

/**
 * The one place a provider description becomes a native config.
 *
 * Both call sites use it because a hand-written copy is how a field goes
 * missing: the runtime merges a per-turn provider over the saved one, so a
 * field this function forgets to copy is not merely absent from the request,
 * it is the saved value being overridden with a default.
 */
function toNativeProvider(draft: ProviderDraft): ProviderConfigDto {
  return {
    id: draft.id,
    name: draft.name,
    baseUrl: draft.baseUrl,
    selectedModel: draft.selectedModel,
    maxTokens: draft.maxTokens,
    temperature: draft.temperature,
    replaysAssistantReasoning: draft.replaysAssistantReasoning
  } as ProviderConfigDto;
}

function toModelDiscovery(value: ModelDiscoveryDto): ModelDiscovery {
  const models: ModelSummary[] = [];
  for (let i = 0; i < value.models.length; i++) {
    models.push(toModel(value.models[i]));
  }
  return {
    listing: value.listing,
    note: value.note,
    manualEntryAllowed: value.manualEntryAllowed,
    models: models
  } as ModelDiscovery;
}

function toEndpointExploration(value: EndpointExplorationDto): EndpointExploration {
  return {
    protocol: value.protocol,
    protocols: value.protocols,
    status: value.status,
    reason: value.reason === undefined ? '' : value.reason,
    models: toModelDiscovery(value.models)
  } as EndpointExploration;
}

function toDiagnosticEntry(value: DiagnosticEntryDto): DiagnosticEntry {
  return {
    kind: value.kind,
    stage: value.stage,
    message: value.message,
    atMs: value.atMs,
    detail: value.detail
  } as DiagnosticEntry;
}

function toStoredEndpoint(value: StoredEndpointDto): StoredEndpoint {
  return {
    id: value.id,
    baseUrl: value.baseUrl,
    selectedModel: value.selectedModel,
    label: value.label
  } as StoredEndpoint;
}

function toSession(value: SessionSummaryDto): SessionSummary {
  return {
    id: value.id,
    title: value.title,
    characterId: value.characterId,
    characterName: value.characterName,
    preview: value.preview,
    updatedAt: Math.round(value.updatedAtMs)
  } as SessionSummary;
}

function toCharacter(value: CharacterSummaryDto): CharacterSummary {
  return { id: value.id, name: value.name, description: value.description, tags: [] } as CharacterSummary;
}

function toModel(value: ModelSummaryDto): ModelSummary {
  const state: string = value.configured ? 'ready' : 'no credential';
  return {
    id: value.id,
    name: value.name,
    providerLabel: value.endpointLabel,
    contextLabel: state
  } as ModelSummary;
}

function toContextSource(value: ContextSourceDto): ContextSourceSummary {
  return {
    id: value.id,
    name: value.name,
    kind: value.kind,
    recordCount: value.recordCount
  } as ContextSourceSummary;
}

/**
 * The one place a runtime failure is turned into a stable code.
 *
 * `entity_not_found` is its own code rather than sharing the generic one: an
 * editor saving a resource that was deleted from another screen has to be able
 * to say "that is gone", which is a different fact from "the write failed" and
 * a different thing for a user to do about it.
 */
function toFailureCode(message: string): string {
  if (message.indexOf('no_provider_configured') === 0) {
    return 'no_provider_configured';
  }
  if (message.indexOf('missing_credential') === 0) {
    return 'missing_credential';
  }
  if (message.indexOf('session_not_found') === 0) {
    return 'session_not_found';
  }
  if (message.indexOf('entity_not_found') === 0) {
    return 'entity_not_found';
  }
  return 'turn_failed';
}

function toMessage(value: MessageDto): MessageSummary {
  const summary: MessageSummary = {
    id: value.id,
    role: toRole(value.role),
    text: value.text
  } as MessageSummary;
  const calls: ToolCallDto[] = value.toolCalls === undefined || value.toolCalls === null
    ? [] : value.toolCalls;
  if (calls.length > 0) {
    summary.toolName = calls[0].title;
    summary.toolStatus = calls[0].isError || calls[0].status === 'failed' ? 'failed' : 'done';
  }
  return summary;
}

function toParticipant(value: ParticipantDto): ConversationParticipant {
  return {
    characterId: value.characterId,
    name: value.name,
    role: value.role
  } as ConversationParticipant;
}

function toPersona(value: PersonaDto): PersonaSummary {
  return {
    id: value.id,
    name: value.name,
    description: value.description,
    worldbookCount: value.worldbookCount
  } as PersonaSummary;
}

function toPersonaDraft(value: PersonaRequestDto): PersonaDraft {
  return {
    id: value.id,
    name: value.name,
    description: value.description,
    userPrompt: value.userPrompt,
    worldbookIds: plainArray(value.worldbookIds)
  } as PersonaDraft;
}

function toNativePersonaDraft(value: PersonaDraft): PersonaRequestDto {
  return {
    id: value.id,
    name: value.name,
    description: value.description,
    userPrompt: value.userPrompt,
    worldbookIds: plainArray(value.worldbookIds)
  } as PersonaRequestDto;
}

function toWorldBookEntry(value: WorldBookEntryDto): WorldBookEntry {
  return {
    id: value.id,
    name: value.name,
    content: value.content,
    keys: plainArray(value.keys),
    enabled: value.enabled,
    constant: value.constant,
    position: value.position
  } as WorldBookEntry;
}

function toWorldBook(value: WorldBookDto): WorldBookSummary {
  const entries: WorldBookEntry[] = [];
  for (let i = 0; i < value.entries.length; i++) {
    entries.push(toWorldBookEntry(value.entries[i]));
  }
  return { id: value.id, name: value.name, entries: entries } as WorldBookSummary;
}

function toNativeWorldBook(value: WorldBookDraft): WorldBookRequestDto {
  const entries: WorldBookEntryRequestDto[] = [];
  for (let i = 0; i < value.entries.length; i++) {
    const entry: WorldBookEntryDraft = value.entries[i];
    entries.push({
      id: entry.id,
      name: entry.name,
      content: entry.content,
      keys: plainArray(entry.keys),
      enabled: entry.enabled,
      constant: entry.constant,
      position: entry.position
    } as WorldBookEntryRequestDto);
  }
  return { id: value.id, name: value.name, entries: entries } as WorldBookRequestDto;
}

function toPromptProfile(value: PromptProfileDto): PromptProfileSummary {
  return { id: value.id, name: value.name } as PromptProfileSummary;
}

function toPromptProfileDraft(value: PromptProfileRequestDto): PromptProfileDraft {
  return {
    id: value.id,
    name: value.name,
    systemPrompt: value.systemPrompt,
    userPrompt: value.userPrompt,
    postHistoryInstructions: value.postHistoryInstructions,
    formatRules: value.formatRules
  } as PromptProfileDraft;
}

function toNativePromptProfile(value: PromptProfileDraft): PromptProfileRequestDto {
  return {
    id: value.id,
    name: value.name,
    systemPrompt: value.systemPrompt,
    userPrompt: value.userPrompt,
    postHistoryInstructions: value.postHistoryInstructions,
    formatRules: value.formatRules
  } as PromptProfileRequestDto;
}

function toCharacterDraft(value: CharacterRequestDto): CharacterDraft {
  return {
    id: value.id,
    name: value.name,
    description: value.description,
    personality: value.personality,
    scenario: value.scenario,
    firstMessage: value.firstMessage,
    alternateGreetings: value.alternateGreetings,
    exampleDialogue: value.exampleDialogue,
    systemPrompt: value.systemPrompt,
    postHistoryInstructions: value.postHistoryInstructions,
    worldbookIds: value.worldbookIds
  } as CharacterDraft;
}

function toNativeCharacter(value: CharacterDraft): CharacterRequestDto {
  return {
    id: value.id,
    name: value.name,
    description: value.description,
    personality: value.personality,
    scenario: value.scenario,
    firstMessage: value.firstMessage,
    alternateGreetings: plainArray(value.alternateGreetings),
    exampleDialogue: value.exampleDialogue,
    systemPrompt: value.systemPrompt,
    postHistoryInstructions: value.postHistoryInstructions,
    worldbookIds: plainArray(value.worldbookIds)
  } as CharacterRequestDto;
}

function toNativeParticipant(value: ParticipantBinding): ParticipantRequestDto {
  return {
    characterId: value.characterId,
    role: value.role,
    displayName: value.displayName
  } as ParticipantRequestDto;
}

export class SujiuNativeBridge implements SujiuBridge {
  private readonly runtime: SujiuRuntimeBridge;
  private draft: ProviderDraft | null = null;
  private apiKey: string = '';
  private running: boolean = false;
  /**
   * A provider description for exactly the next turn, or null.
   *
   * The saved provider is already in the runtime, so an ordinary turn does not
   * resend it. Resending meant rebuilding a config from whatever fields this
   * bridge happened to copy, and the runtime merges a per-turn provider over
   * the saved one, so any field left out of the copy silently overrode the
   * saved value with a default. A partial copy is a claim about the provider,
   * not a partial update.
   */
  private turnProviderOverride: ProviderDraft | null = null;

  constructor() {
    this.runtime = create();
  }

  /**
   * Which runtime this process is actually talking to.
   *
   * Only the identity and the version. The "native runtime" half of the About
   * row is a *description*, and it has to come from the resource table: when
   * this method baked the English words into its return value, a Chinese user
   * read `native runtime` on a screen whose every other line was translated.
   */
  runtimeSummary(): string {
    try {
      return `sujiu-runtime ${this.runtime.coreVersion()}`;
    } catch (error) {
      return 'sujiu-runtime';
    }
  }

  listSessions(): Promise<SessionSummary[]> {
    try {
      const rows: SessionSummaryDto[] = this.runtime.listSessions();
      const result: SessionSummary[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toSession(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`list_sessions failed: ${(error as Error).message}`));
    }
  }

  listCharacters(query: string): Promise<CharacterSummary[]> {
    try {
      const rows: CharacterSummaryDto[] = this.runtime.listCharacters(query);
      const result: CharacterSummary[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toCharacter(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`list_characters failed: ${(error as Error).message}`));
    }
  }

  listModels(): Promise<ModelSummary[]> {
    try {
      const rows: ModelSummaryDto[] = this.runtime.listModels();
      const result: ModelSummary[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toModel(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`list_models failed: ${(error as Error).message}`));
    }
  }

  listContextSources(): Promise<ContextSourceSummary[]> {
    try {
      const rows: ContextSourceDto[] = this.runtime.listContextSources();
      const result: ContextSourceSummary[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toContextSource(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`list_context_sources failed: ${(error as Error).message}`));
    }
  }

  conversationState(sessionId: string): Promise<ConversationSnapshot> {
    try {
      const snapshot: ConversationSnapshotDto | null = this.runtime.conversationState(sessionId);
      if (snapshot === null || snapshot === undefined) {
        return Promise.reject(new Error(`conversation_state: unknown session ${sessionId}`));
      }
      const messages: MessageSummary[] = [];
      for (let i = 0; i < snapshot.messages.length; i++) {
        messages.push(toMessage(snapshot.messages[i]));
      }
      let session: SessionSummary | undefined = undefined;
      const rows: SessionSummaryDto[] = this.runtime.listSessions();
      for (let i = 0; i < rows.length; i++) {
        if (rows[i].id === snapshot.sessionId) {
          session = toSession(rows[i]);
          break;
        }
      }
      if (session === undefined) {
        session = {
          id: snapshot.sessionId,
          title: '',
          characterId: '',
          characterName: '',
          preview: '',
          updatedAt: 0
        } as SessionSummary;
      }
      const sources: ContextSourceDto[] = this.runtime.listContextSources();
      const contextSources: ContextSourceSummary[] = [];
      for (let i = 0; i < sources.length; i++) {
        contextSources.push(toContextSource(sources[i]));
      }
      const participants: ConversationParticipant[] = [];
      const bound: ParticipantDto[] = snapshot.participants === undefined || snapshot.participants === null
        ? [] : snapshot.participants;
      for (let i = 0; i < bound.length; i++) {
        participants.push(toParticipant(bound[i]));
      }
      return Promise.resolve({
        session: session,
        messages: messages,
        contextSources: contextSources,
        participants: participants,
        personaId: snapshot.personaId === null || snapshot.personaId === undefined
          ? undefined : snapshot.personaId,
        // Copied on the way out as well as on the way in. This list ends up in
        // observable controller state and is handed straight back when the user
        // changes something else about the conversation, so it has to be a plain
        // array by the time it goes back rather than whatever state made it.
        worldbookIds: plainArray(snapshot.worldbookIds),
        promptProfileId: snapshot.promptProfileId === null || snapshot.promptProfileId === undefined
          ? undefined : snapshot.promptProfileId
      } as ConversationSnapshot);
    } catch (error) {
      return Promise.reject(new Error(`conversation_state failed: ${(error as Error).message}`));
    }
  }

  createSession(characterId: string): Promise<SessionSummary> {
    try {
      const id: string = this.runtime.createSession(characterId);
      const rows: SessionSummaryDto[] = this.runtime.listSessions();
      for (let i = 0; i < rows.length; i++) {
        if (rows[i].id === id) {
          return Promise.resolve(toSession(rows[i]));
        }
      }
      return Promise.reject(new Error(`create_session: ${id} missing from the session list`));
    } catch (error) {
      return Promise.reject(new Error(`create_session failed: ${(error as Error).message}`));
    }
  }

  listPersonas(): Promise<PersonaSummary[]> {
    try {
      const rows: PersonaDto[] = this.runtime.listPersonas();
      const result: PersonaSummary[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toPersona(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`list_personas failed: ${(error as Error).message}`));
    }
  }

  /**
   * One persona in full.
   *
   * An unknown id stays an empty answer rather than becoming a rejection: the
   * caller is an editor deciding what to show, and "this persona is gone" is
   * something it renders, not an exception it has to guess the meaning of.
   */
  persona(id: string): Promise<PersonaDraft | undefined> {
    try {
      const value: PersonaRequestDto | null = this.runtime.persona(id);
      return Promise.resolve(value === null || value === undefined ? undefined : toPersonaDraft(value));
    } catch (error) {
      return Promise.reject(new Error(`persona failed: ${(error as Error).message}`));
    }
  }

  savePersona(draft: PersonaDraft): Promise<string> {
    try {
      return Promise.resolve(this.runtime.savePersona(toNativePersonaDraft(draft)));
    } catch (error) {
      return Promise.reject(new Error(`save_persona failed: ${(error as Error).message}`));
    }
  }

  deletePersona(id: string): Promise<boolean> {
    try {
      return Promise.resolve(this.runtime.deletePersona(id));
    } catch (error) {
      return Promise.reject(new Error(`delete_persona failed: ${(error as Error).message}`));
    }
  }

  listWorldBooks(): Promise<WorldBookSummary[]> {
    try {
      const rows: WorldBookDto[] = this.runtime.listWorldBooks();
      const result: WorldBookSummary[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toWorldBook(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`list_world_books failed: ${(error as Error).message}`));
    }
  }

  saveWorldBook(draft: WorldBookDraft): Promise<string> {
    try {
      return Promise.resolve(this.runtime.saveWorldBook(toNativeWorldBook(draft)));
    } catch (error) {
      return Promise.reject(new Error(`save_world_book failed: ${(error as Error).message}`));
    }
  }

  deleteWorldBook(id: string): Promise<boolean> {
    try {
      return Promise.resolve(this.runtime.deleteWorldBook(id));
    } catch (error) {
      return Promise.reject(new Error(`delete_world_book failed: ${(error as Error).message}`));
    }
  }

  listPromptProfiles(): Promise<PromptProfileSummary[]> {
    try {
      const rows: PromptProfileDto[] = this.runtime.listPromptProfiles();
      const result: PromptProfileSummary[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toPromptProfile(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`list_prompt_profiles failed: ${(error as Error).message}`));
    }
  }

  promptProfile(id: string): Promise<PromptProfileDraft | undefined> {
    try {
      const value: PromptProfileRequestDto | null = this.runtime.promptProfile(id);
      return Promise.resolve(
        value === null || value === undefined ? undefined : toPromptProfileDraft(value));
    } catch (error) {
      return Promise.reject(new Error(`prompt_profile failed: ${(error as Error).message}`));
    }
  }

  savePromptProfile(draft: PromptProfileDraft): Promise<string> {
    try {
      return Promise.resolve(this.runtime.savePromptProfile(toNativePromptProfile(draft)));
    } catch (error) {
      return Promise.reject(new Error(`save_prompt_profile failed: ${(error as Error).message}`));
    }
  }

  deletePromptProfile(id: string): Promise<boolean> {
    try {
      return Promise.resolve(this.runtime.deletePromptProfile(id));
    } catch (error) {
      return Promise.reject(new Error(`delete_prompt_profile failed: ${(error as Error).message}`));
    }
  }

  saveCharacter(draft: CharacterDraft): Promise<string> {
    try {
      return Promise.resolve(this.runtime.saveCharacter(toNativeCharacter(draft)));
    } catch (error) {
      return Promise.reject(new Error(`save_character failed: ${(error as Error).message}`));
    }
  }

  character(id: string): Promise<CharacterDraft | undefined> {
    try {
      const value: CharacterRequestDto | null = this.runtime.character(id);
      return Promise.resolve(value === null || value === undefined ? undefined : toCharacterDraft(value));
    } catch (error) {
      return Promise.reject(new Error(`character failed: ${(error as Error).message}`));
    }
  }

  deleteCharacter(id: string): Promise<boolean> {
    try {
      return Promise.resolve(this.runtime.deleteCharacter(id));
    } catch (error) {
      return Promise.reject(new Error(`delete_character failed: ${(error as Error).message}`));
    }
  }

  setConversationBindings(
    sessionId: string,
    participants: ParticipantBinding[],
    personaId?: string,
    worldbookIds?: string[],
    promptProfileId?: string
  ): Promise<void> {
    try {
      const bound: ParticipantRequestDto[] = [];
      for (let i = 0; i < participants.length; i++) {
        bound.push(toNativeParticipant(participants[i]));
      }
      // `undefined` is not the same answer as an empty list here: the generated
      // declaration reads a missing argument as "leave it alone", so it is
      // passed through rather than copied into an empty array.
      const copiedWorldbookIds: string[] | undefined =
        worldbookIds === undefined ? undefined : plainArray(worldbookIds);
      // Declared as returning nothing and wrapped the same way `configureProvider`
      // is. Both write synchronously, and chaining a `.then` onto a value that is
      // not a promise reports failure for work that already succeeded.
      this.runtime.setConversationBindings(
        sessionId, bound, personaId, copiedWorldbookIds, promptProfileId);
      return Promise.resolve();
    } catch (error) {
      return Promise.reject(new Error(`set_conversation_bindings failed: ${(error as Error).message}`));
    }
  }

  sendTurn(sessionId: string, userText: string, onEvent: TurnEventListener): void {
    if (this.running) {
      onEvent({ kind: TurnEventKind.Failed, message: 'turn_busy', isError: true } as TurnEvent);
      return;
    }
    this.running = true;

    // Read the override before the request is built: it applies to one turn
    // only, and leaving it set would silently re-apply it to the next one.
    const override: ProviderDraft | null = this.turnProviderOverride;
    this.turnProviderOverride = null;

    const request: TurnRequestDto = {
      sessionId: sessionId,
      userText: userText,
      // No provider means "use the one the runtime already has". Sending the
      // saved draft again would restate it field by field, and every field
      // this bridge forgot to copy would be read as the value the user chose.
      provider: override === null ? undefined : toNativeProvider(override),
      apiKey: this.apiKey
    };

    this.runtime.sendTurn(request, (_error: Error | null, payload: string): void => {
      if (payload === undefined || payload === null) {
        return;
      }
      const parsed: RuntimeTurnEventPayload = JSON.parse(payload) as RuntimeTurnEventPayload;
      const kind: TurnEventKind = toKind(parsed.kind);
      if (kind === TurnEventKind.Failed) {
        // Rust reports what went wrong; the UI localizes a code and keeps the
        // runtime text as detail. An unrecognized kind keeps its raw spelling
        // as the detail, so a contract mismatch is diagnosable instead of
        // presenting as an empty failure.
        const text: string = parsed.kind === 'turn_failed' || parsed.text === undefined
          ? (parsed.text ?? '')
          : `unknown event kind: ${parsed.kind}`;
        onEvent({
          kind: kind,
          message: toFailureCode(text),
          text: text,
          isError: true
        } as TurnEvent);
        return;
      }
      onEvent({
        kind: kind,
        text: parsed.text,
        summary: parsed.text,
        callId: parsed.toolCallId,
        toolName: parsed.toolName,
        isError: parsed.isError
      } as TurnEvent);
    }).catch((error: Error): void => {
      onEvent({
        kind: TurnEventKind.Failed,
        message: toFailureCode(error.message),
        text: error.message,
        isError: true
      } as TurnEvent);
    }).finally((): void => {
      this.running = false;
    });
  }

  async useDataDirectory(directory: string): Promise<boolean> {
    // The native call returns void, so it is invoked directly and not awaited.
    // The local interface used to declare it as a promise, and awaiting a value
    // that was never one is the same mistake as chaining `.then` onto it: the
    // work has already happened by the time the type says it has not. The
    // generated declaration now says void, and the compiler enforces it.
    this.runtime.useDataDirectory(directory);
    return true;
  }

  cancelTurn(_sessionId: string): void {
    this.running = false;
    try {
      this.runtime.cancelTurn();
    } catch (error) {
      // The runtime is already idle, so there is nothing to cancel.
    }
  }

  /** Wire protocols this runtime speaks, best first, from the runtime itself. */
  supportedProtocols(): Promise<string[]> {
    return Promise.resolve(this.runtime.supportedProtocols());
  }

  /**
   * Works out what an endpoint can do from an address and a key alone.
   *
   * The key is passed straight through and not kept, and nothing here is
   * written down: the settings screen asks this before anything is saved, so
   * the model list it shows is what the endpoint actually offered rather than
   * what an earlier choice happened to leave behind.
   */
  rememberedEndpoint(baseUrl: string): Promise<EndpointExploration | undefined> {
    try {
      const native: EndpointExplorationDto | null = this.runtime.rememberedEndpoint(baseUrl);
      // The generated declaration says `| null` because that is what napi-rs
      // hands back for an `Option` of `None`, so the guard reads the same way
      // the runtime actually behaves. It used to be written against a local
      // interface that said `undefined` instead, which compiled and then mapped
      // a null straight into the converter; the symptom was an endpoint with
      // nothing remembered reporting "cannot read property protocol of null",
      // pointing at the mapper rather than at the guard.
      //
      // Nothing stays nothing rather than becoming a promise of a blank answer,
      // because "never asked" and "asked and found nothing" are different
      // answers and a caller has to be able to tell them apart.
      if (native === null) {
        return Promise.resolve(undefined);
      }
      return Promise.resolve(toEndpointExploration(native));
    } catch (error) {
      return Promise.reject(new Error(`remembered_endpoint failed: ${(error as Error).message}`));
    }
  }

  discoverEndpoint(baseUrl: string, apiKey: string, refresh: boolean = false): Promise<EndpointExploration> {
    try {
      return Promise.resolve(
        toEndpointExploration(this.runtime.discoverEndpoint(baseUrl, apiKey, refresh))
      );
    } catch (error) {
      return Promise.reject(new Error(`discover_endpoint failed: ${(error as Error).message}`));
    }
  }

  /**
   * Hands the provider description to the runtime, so the model list reflects
   * the real configuration. A blank address surfaces as a rejected promise.
   */
  configureProvider(draft: ProviderDraft): Promise<void> {
    try {
      // The native call is synchronous and returns nothing, so this must not
      // chain onto it. Reading `.then` off the result made every save report
      // a failure while the endpoint had in fact been written, which is the
      // worst shape a bug can take: the user is told their work was lost and
      // it was not.
      this.runtime.configureProvider(toNativeProvider(draft));
      this.draft = draft;
      return Promise.resolve();
    } catch (error) {
      return Promise.reject(new Error(`configure_provider failed: ${(error as Error).message}`));
    }
  }

  /**
   * Reads back the endpoint already configured, so a settings form can open on it.
   *
   * An empty form is not neutral. Opened on an endpoint that has a model and a
   * conversation behind it, it reads as "nothing is set up", and the natural
   * reaction to that is to set it up again.
   */
  storedEndpoint(): Promise<StoredEndpoint | null> {
    try {
      const stored = this.runtime.storedEndpoint();
      return Promise.resolve(stored === null ? null : toStoredEndpoint(stored));
    } catch (error) {
      return Promise.reject(new Error(`stored_endpoint failed: ${(error as Error).message}`));
    }
  }

  /**
   * The diagnostic log, newest last.
   *
   * The runtime caps what it holds, so a limit is a request for fewer lines
   * rather than a way to get more. Omitted means everything, which is the right
   * default when someone is about to read a failure back to themselves.
   */
  diagnostics(limit?: number): Promise<DiagnosticEntry[]> {
    try {
      const rows: DiagnosticEntryDto[] = this.runtime.diagnostics(limit);
      const result: DiagnosticEntry[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toDiagnosticEntry(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(new Error(`diagnostics failed: ${(error as Error).message}`));
    }
  }

  /**
   * Only the capability and model probing half.
   *
   * A settings screen asks for this rather than the whole log so a protocol
   * switch during a chat turn is not read as part of the answer to the question
   * the user is currently looking at.
   */
  discoveryDiagnostics(limit?: number): Promise<DiagnosticEntry[]> {
    try {
      const rows: DiagnosticEntryDto[] = this.runtime.discoveryDiagnostics(limit);
      const result: DiagnosticEntry[] = [];
      for (let i = 0; i < rows.length; i++) {
        result.push(toDiagnosticEntry(rows[i]));
      }
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(
        new Error(`discovery_diagnostics failed: ${(error as Error).message}`)
      );
    }
  }

  /**
   * The whole log as one block of text, already redacted.
   *
   * This is what a user pastes into a bug report, so it is the entire log and
   * not a summary: the line that explains a failure is rarely the last one.
   */
  diagnosticsText(): Promise<string> {
    try {
      return Promise.resolve(this.runtime.diagnosticsText());
    } catch (error) {
      return Promise.reject(new Error(`diagnostics_text failed: ${(error as Error).message}`));
    }
  }

  /**
   * Empties the log and nothing else. Someone clearing a log to investigate
   * afresh has not asked to lose their endpoint or their conversations, and a
   * clear that did that would be a trap rather than a convenience.
   */
  clearDiagnostics(): Promise<void> {
    try {
      this.runtime.clearDiagnostics();
      return Promise.resolve();
    } catch (error) {
      return Promise.reject(new Error(`clear_diagnostics failed: ${(error as Error).message}`));
    }
  }

  /**
   * Runs the next turn against `draft` instead of the saved provider, without
   * saving it. This is a genuine override, so the description is sent whole.
   */
  withProviderForNextTurn(draft: ProviderDraft): void {
    this.turnProviderOverride = draft;
  }

  providerDraft(): ProviderDraft | null {
    return this.draft;
  }

  /**
   * The api key for the next turn. It is passed to the runtime per call and
   * never stored here; the platform credential service keeps the copy.
   */
  setCredential(secret: string): void {
    this.apiKey = secret;
  }
}
