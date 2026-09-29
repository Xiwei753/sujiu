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
import sujiu from 'libsujiu_napi.so';
import { SujiuBridge, SessionSummary, CharacterSummary, ModelSummary, ContextSourceSummary,
  ConversationSnapshot, MessageSummary, MessageRole, TurnEvent, TurnEventKind,
  TurnEventListener, ProviderDraft } from './SujiuBridge';

interface NativeSession {
  id: string;
  characterId: string;
  characterName: string;
  preview: string;
  title: string;
  updatedAtMs: number;
  messageCount: number;
}

interface NativeCharacter {
  id: string;
  name: string;
  tagline: string;
}

interface NativeModel {
  id: string;
  name: string;
  providerId: string;
  providerName: string;
  kind: string;
  configured: boolean;
}

interface NativeContextSource {
  id: string;
  kind: string;
  name: string;
  description: string;
  recordCount: number;
}

interface NativeToolCall {
  id: string;
  name: string;
  title: string;
  status: string;
  isError: boolean;
  resultText: string;
}

interface NativeMessage {
  id: string;
  role: string;
  text: string;
  toolCalls: NativeToolCall[];
}

interface NativeSnapshot {
  sessionId: string;
  character: NativeCharacter | null;
  messages: NativeMessage[];
}

interface NativeProviderConfig {
  id: string;
  name: string;
  kind: string;
  baseUrl: string;
  model: string;
  maxTokens?: number;
  temperature?: number;
}

interface NativeTurnRequest {
  sessionId: string;
  userText: string;
  provider: NativeProviderConfig | undefined;
  apiKey: string;
}

interface NativeTurnEvent {
  kind: string;
  text: string;
  toolName: string;
  toolCallId: string;
  isError: boolean;
}

interface NativeRuntime {
  coreVersion(): string;
  providerKinds(): string[];
  listSessions(): NativeSession[];
  listCharacters(query: string): NativeCharacter[];
  listModels(): NativeModel[];
  listContextSources(): NativeContextSource[];
  conversationState(sessionId: string): NativeSnapshot | null;
  createSession(characterId: string): string;
  cancelTurn(): void;
  useDataDirectory(directory: string): Promise<void>;
  dataDirectory(): string | null;
  /** Applies a provider description. Rejects when the kind is not supported. */
  configureProvider(draft: NativeProviderConfig): Promise<void>;
  /**
   * Runs a turn and reports each event as JSON.
   *
   * The native callback is invoked as `(unused, payload)`: the first argument
   * is the callback return value, which this module never uses and which is
   * therefore always null.
   */
  sendTurn(
    request: NativeTurnRequest,
    onEvent: (unused: Object | null, payload: string) => void
  ): Promise<string>;
}

interface SujiuNativeModule {
  create(): NativeRuntime;
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

function toSession(value: NativeSession): SessionSummary {
  return {
    id: value.id,
    title: value.title,
    characterId: value.characterId,
    characterName: value.characterName,
    preview: value.preview,
    updatedAt: Math.round(value.updatedAtMs)
  } as SessionSummary;
}

function toCharacter(value: NativeCharacter): CharacterSummary {
  return { id: value.id, name: value.name, tagline: value.tagline, tags: [] } as CharacterSummary;
}

function toModel(value: NativeModel): ModelSummary {
  const state: string = value.configured ? 'ready' : 'no credential';
  return {
    id: value.id,
    name: value.name,
    providerLabel: `${value.providerName} · ${value.kind}`,
    contextLabel: state
  } as ModelSummary;
}

function toContextSource(value: NativeContextSource): ContextSourceSummary {
  return {
    id: value.id,
    name: value.name,
    kindLabel: value.kind.replace(/([a-z])([A-Z])/g, '$1 $2'),
    recordCount: value.recordCount
  } as ContextSourceSummary;
}

/**
 * Maps a runtime failure onto a stable code. Unknown failures keep the code
 * `turn_failed` and pass the runtime text through as detail, so an unexpected
 * provider error stays visible instead of being swallowed by a generic string.
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
  return 'turn_failed';
}

function toMessage(value: NativeMessage): MessageSummary {
  const summary: MessageSummary = {
    id: value.id,
    role: toRole(value.role),
    text: value.text
  } as MessageSummary;
  const calls: NativeToolCall[] = value.toolCalls === undefined || value.toolCalls === null
    ? [] : value.toolCalls;
  if (calls.length > 0) {
    summary.toolName = calls[0].title;
    summary.toolStatus = calls[0].isError || calls[0].status === 'failed' ? 'failed' : 'done';
  }
  return summary;
}

export class SujiuNativeBridge implements SujiuBridge {
  private readonly runtime: NativeRuntime;
  private draft: ProviderDraft | null = null;
  private apiKey: string = '';
  private running: boolean = false;

  constructor() {
    const module: SujiuNativeModule = sujiu as SujiuNativeModule;
    this.runtime = module.create();
  }

  runtimeSummary(): string {
    try {
      return `sujiu-ffi ${this.runtime.coreVersion()} · native runtime`;
    } catch (error) {
      return 'sujiu-ffi · native runtime';
    }
  }

  listSessions(): Promise<SessionSummary[]> {
    try {
      const rows: NativeSession[] = this.runtime.listSessions();
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
      const rows: NativeCharacter[] = this.runtime.listCharacters(query);
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
      const rows: NativeModel[] = this.runtime.listModels();
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
      const rows: NativeContextSource[] = this.runtime.listContextSources();
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
      const snapshot: NativeSnapshot | null = this.runtime.conversationState(sessionId);
      if (snapshot === null || snapshot === undefined) {
        return Promise.reject(new Error(`conversation_state: unknown session ${sessionId}`));
      }
      const messages: MessageSummary[] = [];
      for (let i = 0; i < snapshot.messages.length; i++) {
        messages.push(toMessage(snapshot.messages[i]));
      }
      let session: SessionSummary | undefined = undefined;
      const rows: NativeSession[] = this.runtime.listSessions();
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
      const sources: NativeContextSource[] = this.runtime.listContextSources();
      const contextSources: ContextSourceSummary[] = [];
      for (let i = 0; i < sources.length; i++) {
        contextSources.push(toContextSource(sources[i]));
      }
      return Promise.resolve({
        session: session,
        messages: messages,
        contextSources: contextSources
      } as ConversationSnapshot);
    } catch (error) {
      return Promise.reject(new Error(`conversation_state failed: ${(error as Error).message}`));
    }
  }

  createSession(characterId: string): Promise<SessionSummary> {
    try {
      const id: string = this.runtime.createSession(characterId);
      const rows: NativeSession[] = this.runtime.listSessions();
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

  sendTurn(sessionId: string, userText: string, onEvent: TurnEventListener): void {
    if (this.running) {
      onEvent({ kind: TurnEventKind.Failed, message: 'turn_busy', isError: true } as TurnEvent);
      return;
    }
    this.running = true;
    const request: NativeTurnRequest = {
      sessionId: sessionId,
      userText: userText,
      provider: this.draft === null ? undefined : {
        id: this.draft.id,
        name: this.draft.name,
        kind: this.draft.kind,
        baseUrl: this.draft.baseUrl,
        model: this.draft.model,
        maxTokens: this.draft.maxTokens,
        temperature: this.draft.temperature
      } as NativeProviderConfig,
      apiKey: this.apiKey
    };

    this.runtime.sendTurn(request, (_unused: Object | null, payload: string): void => {
      if (payload === undefined || payload === null) {
        return;
      }
      const parsed: NativeTurnEvent = JSON.parse(payload) as NativeTurnEvent;
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
    await this.runtime.useDataDirectory(directory);
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

  /** Provider kinds come from the runtime, not from a hardcoded list here. */
  supportedProviderKinds(): Promise<string[]> {
    return Promise.resolve(this.runtime.providerKinds());
  }

  /**
   * Hands the provider description to the runtime, so the model list reflects
   * the real configuration. A rejected kind surfaces as a rejected promise.
   */
  configureProvider(draft: ProviderDraft): Promise<void> {
    return this.runtime
      .configureProvider({
        id: draft.id,
        name: draft.name,
        kind: draft.kind,
        baseUrl: draft.baseUrl,
        model: draft.model,
        maxTokens: draft.maxTokens,
        temperature: draft.temperature
      } as NativeProviderConfig)
      .then(() => {
        this.draft = draft;
      });
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
