import {
  CharacterSummary,
  ContextSourceSummary,
  MessageRole,
  MessageSummary,
  ModelSummary,
  SessionSummary,
  ProviderDraft,
  SujiuBridge,
  TurnEvent,
  TurnEventKind
} from '../bridge/SujiuBridge';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { AppearanceMode, CredentialService, PlatformServices } from '../platform/PlatformServices';

/** Where the api key is filed in the platform credential store. */
const CREDENTIAL_NAME: string = 'provider_api_key';

/**
 * Turn lifecycle a chat surface renders. Sujiu states, not provider states.
 */
export enum GenerationState {
  Idle = 'Idle',
  Submitting = 'Submitting',
  Streaming = 'Streaming',
  WaitingForTool = 'WaitingForTool',
  ExecutingTool = 'ExecutingTool',
  ContinuingAfterTool = 'ContinuingAfterTool',
  Completed = 'Completed',
  Failed = 'Failed',
  Cancelled = 'Cancelled'
}

export function isBusy(state: GenerationState): boolean {
  return state === GenerationState.Submitting ||
    state === GenerationState.Streaming ||
    state === GenerationState.WaitingForTool ||
    state === GenerationState.ExecutingTool ||
    state === GenerationState.ContinuingAfterTool;
}

/** Lifecycle of one tool call, as a state rather than a sentence. */
export enum ToolCallStatus {
  Queued = 'queued',
  Running = 'running',
  Done = 'done',
  Failed = 'failed'
}

/** History buckets. The UI turns these into localized headings. */
export enum SessionGroupKey {
  Today = 'today',
  Yesterday = 'yesterday',
  Earlier = 'earlier'
}

@Observed
export class ToolCallItem {
  id: string;
  toolName: string;
  status: ToolCallStatus;
  summary: string = '';
  isError: boolean = false;

  constructor(id: string, toolName: string, status: ToolCallStatus) {
    this.id = id;
    this.toolName = toolName;
    this.status = status;
  }
}

@Observed
export class ChatMessageItem {
  id: string;
  role: MessageRole;
  text: string;
  thinking: string = '';
  toolCalls: ToolCallItem[] = [];

  constructor(id: string, role: MessageRole, text: string) {
    this.id = id;
    this.role = role;
    this.text = text;
  }

  get isUser(): boolean {
    return this.role === MessageRole.User;
  }

  get hasToolCalls(): boolean {
    return this.toolCalls.length > 0;
  }
}

@Observed
export class SessionGroup {
  key: SessionGroupKey;
  sessions: SessionSummary[];

  constructor(key: SessionGroupKey, sessions: SessionSummary[]) {
    this.key = key;
    this.sessions = sessions;
  }
}

/**
 * Presentation controller for the chat surface.
 *
 * Owns page state (open session, draft, generation state, drawer/sheet data)
 * and talks to the application bridge plus platform capabilities. It performs
 * no provider, tool or OS work of its own.
 */
@Observed
export class ChatController {
  private readonly bridge: SujiuBridge;
  private readonly platform: PlatformServices;

  sessionGroups: SessionGroup[] = [];
  messages: ChatMessageItem[] = [];
  characters: CharacterSummary[] = [];
  characterQuery: string = '';
  models: ModelSummary[] = [];
  contextSources: ContextSourceSummary[] = [];
  currentSessionId: string = '';
  currentCharacter: CharacterSummary | undefined = undefined;
  currentModel: ModelSummary | undefined = undefined;
  draft: string = '';
  generationState: GenerationState = GenerationState.Idle;
  errorCode: string = '';
  errorDetail: string = '';
  appearanceMode: AppearanceMode = AppearanceMode.System;

  /**
   * Provider setup. The description is state; the credential is not. The
   * controller asks the platform credential service for the key right before a
   * turn, so a secret never becomes presentation state and never reaches a view.
   */
  providerKinds: string[] = [];
  providerDraft: ProviderDraft | null = null;
  hasCredential: boolean = false;
  savingProvider: boolean = false;
  providerNoticeCode: string = '';

  /**
   * Raised when the appearance mode changes. Presentation does not decide what
   * that means for colors; the UI layer listens and re-themes itself.
   */
  onAppearanceChanged: () => void = () => {
  };

  private assistantOpen: boolean = false;
  private messageCounter: number = 0;

  private readonly credentials: CredentialService;

  constructor(bridge: SujiuBridge, platform: PlatformServices, credentials: CredentialService) {
    this.bridge = bridge;
    this.platform = platform;
    this.credentials = credentials;
  }

  get hasProvider(): boolean {
    return this.providerDraft !== null;
  }

  /** Loads the provider the user saved, so the model list matches reality. */
  async loadProvider(): Promise<void> {
    try {
      this.providerKinds = await this.bridge.supportedProviderKinds();
    } catch (error) {
      this.providerKinds = [];
    }
    this.hasCredential = this.credentials.hasSecret(CREDENTIAL_NAME);
  }

  /**
   * Turns the three fields a settings form actually collects into a provider
   * description. Everything else is decided here rather than by the view: the
   * kind must be one the runtime advertised, and sampling parameters are
   * provider semantics the runtime already defaults, so the form cannot set
   * values that disagree with how a turn really runs.
   */
  async saveProviderFromForm(baseUrl: string, model: string, secret: string): Promise<void> {
    if (this.providerKinds.length === 0 || baseUrl.length === 0 || model.length === 0) {
      return;
    }
    await this.saveProvider({
      id: 'provider-default',
      name: model,
      kind: this.providerKinds[0],
      baseUrl: baseUrl,
      model: model
    } as ProviderDraft, secret);
  }

  /**
   * Saves a provider description and, when given, the api key. The description
   * goes to the runtime; the key goes to the credential service. An unsupported
   * kind fails here instead of at the first turn.
   */
  async saveProvider(draft: ProviderDraft, secret: string): Promise<void> {
    if (this.savingProvider) {
      return;
    }
    this.savingProvider = true;
    this.providerNoticeCode = '';
    try {
      if (secret.length > 0) {
        this.credentials.saveSecret(CREDENTIAL_NAME, secret);
        this.hasCredential = true;
      }
      await this.bridge.configureProvider(draft);
      this.providerDraft = draft;
      this.models = await this.bridge.listModels();
    } catch (error) {
      // The UI shows a localized code, so the runtime detail is logged here.
      hilog.error(0x0000, 'Sujiu', 'saveProvider failed: %{public}s', (error as Error).message);
      this.fail('provider_not_saved', (error as Error).message);
    } finally {
      this.savingProvider = false;
    }
  }

  /** Forgets the provider and the stored key. */
  clearProvider(): void {
    this.credentials.deleteSecret(CREDENTIAL_NAME);
    this.hasCredential = false;
    this.providerDraft = null;
    this.models = [];
  }

  get busy(): boolean {
    return isBusy(this.generationState);
  }

  get canSend(): boolean {
    return !this.busy && this.draft.trim().length > 0;
  }

  platformSummary(): string {
    return this.platform.info.platformSummary();
  }

  runtimeSummary(): string {
    return this.bridge.runtimeSummary();
  }

  characterById(characterId: string): CharacterSummary | undefined {
    return this.characters.find((item: CharacterSummary) => item.id === characterId);
  }

  refresh(): void {
    this.bridge.listSessions().then((sessions: SessionSummary[]) => {
      this.sessionGroups = this.groupSessions(sessions);
      if (this.currentSessionId.length === 0 && sessions.length > 0) {
        this.loadConversation(sessions[0].id);
      }
    });
    this.bridge.listCharacters(this.characterQuery).then((characters: CharacterSummary[]) => {
      this.characters = characters;
    });
    this.bridge.listModels().then((models: ModelSummary[]) => {
      this.models = models;
      if (!this.currentModel && models.length > 0) {
        this.currentModel = models[0];
      }
    });
    this.bridge.listContextSources().then((sources: ContextSourceSummary[]) => {
      this.contextSources = sources;
    });
  }

  openSession(sessionId: string): void {
    this.loadConversation(sessionId);
  }

  /**
   * Starts a new chat by creating a real (empty) session first, so the very
   * first message has somewhere to go. Never leaves the page without a
   * sendable session id.
   */
  newSession(): void {
    if (this.busy) {
      return;
    }
    this.generationState = GenerationState.Submitting;
    const characterId: string = this.currentCharacter?.id ?? '';
    this.bridge.createSession(characterId).then((session: SessionSummary) => {
      this.currentSessionId = session.id;
      this.messages = [];
      this.generationState = GenerationState.Idle;
      this.clearError();
      this.assistantOpen = false;
      this.refreshSessions();
    }).catch(() => {
      this.generationState = GenerationState.Failed;
      this.fail('new_chat_failed');
    });
  }

  private fail(code: string, detail: string = ''): void {
    hilog.error(0x0000, 'Sujiu', 'failure %{public}s: %{public}s', code, detail);
    this.errorCode = code;
    this.errorDetail = detail;
  }

  private clearError(): void {
    this.errorCode = '';
    this.errorDetail = '';
  }

  private refreshSessions(): void {
    this.bridge.listSessions().then((sessions: SessionSummary[]) => {
      this.sessionGroups = this.groupSessions(sessions);
    });
  }

  setDraft(text: string): void {
    this.draft = text;
  }

  send(): void {
    const text: string = this.draft.trim();
    if (text.length === 0 || this.busy || this.currentSessionId.length === 0) {
      return;
    }
    // The key is fetched here so a view never holds it and the runtime gets it
    // per call.
    this.bridge.setCredential(this.credentials.loadSecret(CREDENTIAL_NAME));
    this.assistantOpen = false;
    this.draft = '';
    this.clearError();
    this.generationState = GenerationState.Submitting;
    const userMessage = new ChatMessageItem(this.nextId('user'), MessageRole.User, text);
    this.messages = this.messages.concat([userMessage]);
    this.bridge.sendTurn(this.currentSessionId, text, (event: TurnEvent) => this.applyTurnEvent(event));
  }

  stop(): void {
    if (this.currentSessionId.length === 0) {
      return;
    }
    this.bridge.cancelTurn(this.currentSessionId);
    this.assistantOpen = false;
    this.generationState = GenerationState.Cancelled;
  }

  selectCharacter(characterId: string): void {
    this.currentCharacter = this.characterById(characterId);
  }

  searchCharacters(query: string): void {
    this.characterQuery = query;
    this.bridge.listCharacters(query).then((characters: CharacterSummary[]) => {
      this.characters = characters;
    });
  }

  selectModel(modelId: string): void {
    this.currentModel = this.models.find((item: ModelSummary) => item.id === modelId);
  }

  setAppearance(mode: AppearanceMode): void {
    if (this.appearanceMode === mode) {
      return;
    }
    this.appearanceMode = mode;
    this.onAppearanceChanged();
  }

  copyMessage(message: ChatMessageItem): void {
    if (message.text.trim().length > 0) {
      this.platform.clipboard.copyText('Sujiu', message.text);
    }
  }

  private loadConversation(sessionId: string): void {
    this.bridge.conversationState(sessionId).then((snapshot) => {
      this.currentSessionId = snapshot.session.id;
      this.messages = snapshot.messages.map((item: MessageSummary) => {
        const mapped = new ChatMessageItem(item.id, item.role, item.text);
        if (item.toolName) {
          mapped.toolCalls = [new ToolCallItem(
            `tool-${item.id}`,
            item.toolName,
            item.toolStatus === 'failed' ? ToolCallStatus.Failed : ToolCallStatus.Done
          )];
        }
        return mapped;
      });
      this.contextSources = snapshot.contextSources;
      this.currentCharacter = this.characterById(snapshot.session.characterId);
      if (!this.currentModel && this.models.length > 0) {
        this.currentModel = this.models[0];
      }
      this.generationState = GenerationState.Idle;
      this.clearError();
    });
  }

  private applyTurnEvent(event: TurnEvent): void {
    switch (event.kind) {
      case TurnEventKind.Started:
        this.ensureAssistant();
        break;
      case TurnEventKind.TextDelta:
        this.ensureAssistant();
        this.generationState = GenerationState.Streaming;
        this.appendToAssistant((message: ChatMessageItem) => {
          message.text += event.text ?? '';
        });
        break;
      case TurnEventKind.ThinkingDelta:
        this.ensureAssistant();
        if (this.generationState !== GenerationState.Submitting) {
          this.generationState = GenerationState.Streaming;
        }
        this.appendToAssistant((message: ChatMessageItem) => {
          message.thinking += event.text ?? '';
        });
        break;
      case TurnEventKind.ToolCallRequested:
        // The model asked for a tool; the runtime has not run it yet.
        this.ensureAssistant();
        this.generationState = GenerationState.WaitingForTool;
        this.appendToAssistant((message: ChatMessageItem) => {
          message.toolCalls = message.toolCalls.concat([
            new ToolCallItem(
              event.callId ?? 'call',
              event.toolName ?? 'tool',
              ToolCallStatus.Queued
            )
          ]);
        });
        break;
      case TurnEventKind.ToolCallStarted:
        this.ensureAssistant();
        this.generationState = GenerationState.ExecutingTool;
        this.appendToAssistant((message: ChatMessageItem) => {
          message.toolCalls = message.toolCalls.map((call: ToolCallItem) => {
            if (call.id !== (event.callId ?? 'call')) {
              return call;
            }
            return new ToolCallItem(call.id, call.toolName, ToolCallStatus.Running);
          });
        });
        break;
      case TurnEventKind.ToolCallFinished:
        this.generationState = GenerationState.ContinuingAfterTool;
        this.appendToAssistant((message: ChatMessageItem) => {
          message.toolCalls = message.toolCalls.map((call: ToolCallItem) => {
            if (call.id !== (event.callId ?? 'call')) {
              return call;
            }
            const updated = new ToolCallItem(
              call.id,
              call.toolName,
              event.isError ? ToolCallStatus.Failed : ToolCallStatus.Done
            );
            updated.summary = event.summary ?? '';
            updated.isError = event.isError ?? false;
            return updated;
          });
        });
        break;
      case TurnEventKind.Completed:
        this.assistantOpen = false;
        this.generationState = GenerationState.Completed;
        break;
      case TurnEventKind.Failed:
        this.assistantOpen = false;
        this.generationState = GenerationState.Failed;
        this.fail(event.message ?? '', event.text ?? '');
        break;
      case TurnEventKind.Cancelled:
        this.assistantOpen = false;
        this.generationState = GenerationState.Cancelled;
        break;
      default:
        break;
    }
  }

  private ensureAssistant(): void {
    if (this.assistantOpen) {
      return;
    }
    this.assistantOpen = true;
    this.messages = this.messages.concat([
      new ChatMessageItem(this.nextId('assistant'), MessageRole.Assistant, '')
    ]);
  }

  private appendToAssistant(transform: (message: ChatMessageItem) => void): void {
    if (this.messages.length === 0) {
      return;
    }
    const last = this.messages[this.messages.length - 1];
    transform(last);
    const updated = this.messages.slice(0, this.messages.length - 1);
    this.messages = updated.concat([last]);
  }

  private nextId(prefix: string): string {
    this.messageCounter += 1;
    return `${prefix}-${this.messageCounter}`;
  }

  private groupSessions(sessions: SessionSummary[]): SessionGroup[] {
    const now: number = Date.now();
    const dayMillis: number = 24 * 60 * 60 * 1000;
    const startOfToday: number = now - (now % dayMillis);
    const groups: SessionGroup[] = [
      new SessionGroup(SessionGroupKey.Today, []),
      new SessionGroup(SessionGroupKey.Yesterday, []),
      new SessionGroup(SessionGroupKey.Earlier, [])
    ];
    const sorted: SessionSummary[] = sessions.slice().sort((a: SessionSummary, b: SessionSummary) => b.updatedAt - a.updatedAt);
    sorted.forEach((session: SessionSummary) => {
      if (session.updatedAt >= startOfToday) {
        groups[0].sessions.push(session);
      } else if (session.updatedAt >= startOfToday - dayMillis) {
        groups[1].sessions.push(session);
      } else {
        groups[2].sessions.push(session);
      }
    });
    return groups.filter((group: SessionGroup) => group.sessions.length > 0);
  }
}
