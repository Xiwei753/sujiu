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
  TurnEventListener, ProviderDraft, EndpointExploration, ModelDiscovery,
  StoredEndpoint, DiagnosticEntry } from './SujiuBridge';

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
  description: string;
}

interface NativeModel {
  id: string;
  name: string;
  endpointId: string;
  /**
   * A word to show a person. The endpoint's own name, or a friendly one for an
   * address we recognise. It decides nothing.
   */
  endpointLabel: string;
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
  baseUrl: string;
  selectedModel?: string;
  maxTokens?: number;
  temperature?: number;
  replaysAssistantReasoning?: boolean;
}

interface NativeModelDiscovery {
  listing: string;
  note: string;
  manualEntryAllowed: boolean;
  models: NativeModel[];
}

interface NativeEndpointExploration {
  protocol: string;
  protocols: string[];
  /** How the protocol probe came out, as a code. See `EndpointExploration.status`. */
  status: string;
  reason?: string;
  models: NativeModelDiscovery;
}

/**
 * The native shape of one diagnostic line.
 *
 * `detail` arrives fully rendered and fully redacted. Nothing here can contain
 * a key, which is what lets a platform show a line or paste it into a bug
 * report without a second filtering pass that could be got wrong.
 */
interface NativeDiagnosticEntry {
  kind: string;
  stage: string;
  message: string;
  atMs: number;
  detail: string;
}

/**
 * The native shape of an endpoint that is already configured.
 *
 * There is no credential field here, and there must not be one added. The key
 * lives in platform storage and is read immediately before a turn; a type that
 * carried it would put a secret one careless read away from a log line.
 */
interface NativeStoredEndpoint {
  id: string;
  baseUrl: string;
  selectedModel?: string;
  label: string;
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
  supportedProtocols(): string[];
  listSessions(): NativeSession[];
  listCharacters(query: string): NativeCharacter[];
  listModels(): NativeModel[];
  listContextSources(): NativeContextSource[];
  conversationState(sessionId: string): NativeSnapshot | null;
  createSession(characterId: string): string;
  cancelTurn(): void;
  useDataDirectory(directory: string): Promise<void>;
  dataDirectory(): string | null;
  /**
   * Applies a provider description. Throws when the address is blank.
   *
   * Declared as returning nothing because that is what it returns. It used to be
   * declared as a promise, and chaining `.then` onto a value that is not one
   * threw *after* the endpoint had been written — so every save reported a
   * failure for work that had succeeded. The type now says what is true.
   */
  configureProvider(draft: NativeProviderConfig): void;
  /**
   * Asks an endpoint what it can do. Needs an address and a key and nothing
   * else: no stored endpoint, no chosen model, no vendor named.
   */
  discoverEndpoint(baseUrl: string, apiKey: string): NativeEndpointExploration;
  /** The endpoint already configured, or null. Never carries a credential. */
  storedEndpoint(): NativeStoredEndpoint | null;
  /**
   * The diagnostic log, newest last. `limit` is optional and takes the most
   * recent N lines; omitted means everything held.
   */
  diagnostics(limit?: number): NativeDiagnosticEntry[];
  /** Only the `discovery` half, which is what a settings screen usually wants. */
  discoveryDiagnostics(limit?: number): NativeDiagnosticEntry[];
  /** The whole log as text, already redacted. */
  diagnosticsText(): string;
  /** Empties the log. Configuration and conversations are untouched. */
  clearDiagnostics(): void;
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

/**
 * The one place a provider description becomes a native config.
 *
 * Both call sites use it because a hand-written copy is how a field goes
 * missing: the runtime merges a per-turn provider over the saved one, so a
 * field this function forgets to copy is not merely absent from the request,
 * it is the saved value being overridden with a default.
 */
function toNativeProvider(draft: ProviderDraft): NativeProviderConfig {
  return {
    id: draft.id,
    name: draft.name,
    baseUrl: draft.baseUrl,
    selectedModel: draft.selectedModel,
    maxTokens: draft.maxTokens,
    temperature: draft.temperature,
    replaysAssistantReasoning: draft.replaysAssistantReasoning
  } as NativeProviderConfig;
}

function toModelDiscovery(value: NativeModelDiscovery): ModelDiscovery {
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

function toEndpointExploration(value: NativeEndpointExploration): EndpointExploration {
  return {
    protocol: value.protocol,
    protocols: value.protocols,
    status: value.status,
    reason: value.reason === undefined ? '' : value.reason,
    models: toModelDiscovery(value.models)
  } as EndpointExploration;
}

function toDiagnosticEntry(value: NativeDiagnosticEntry): DiagnosticEntry {
  return {
    kind: value.kind,
    stage: value.stage,
    message: value.message,
    atMs: value.atMs,
    detail: value.detail
  } as DiagnosticEntry;
}

function toStoredEndpoint(value: NativeStoredEndpoint): StoredEndpoint {
  return {
    id: value.id,
    baseUrl: value.baseUrl,
    selectedModel: value.selectedModel,
    label: value.label
  } as StoredEndpoint;
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
  return { id: value.id, name: value.name, description: value.description, tags: [] } as CharacterSummary;
}

function toModel(value: NativeModel): ModelSummary {
  const state: string = value.configured ? 'ready' : 'no credential';
  return {
    id: value.id,
    name: value.name,
    providerLabel: value.endpointLabel,
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

    // Read the override before the request is built: it applies to one turn
    // only, and leaving it set would silently re-apply it to the next one.
    const override: ProviderDraft | null = this.turnProviderOverride;
    this.turnProviderOverride = null;

    const request: NativeTurnRequest = {
      sessionId: sessionId,
      userText: userText,
      // No provider means "use the one the runtime already has". Sending the
      // saved draft again would restate it field by field, and every field
      // this bridge forgot to copy would be read as the value the user chose.
      provider: override === null ? undefined : toNativeProvider(override),
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
  discoverEndpoint(baseUrl: string, apiKey: string): Promise<EndpointExploration> {
    try {
      return Promise.resolve(
        toEndpointExploration(this.runtime.discoverEndpoint(baseUrl, apiKey))
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
      const rows: NativeDiagnosticEntry[] = this.runtime.diagnostics(limit);
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
      const rows: NativeDiagnosticEntry[] = this.runtime.discoveryDiagnostics(limit);
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
