import {
  CharacterSummary,
  ContextSourceSummary,
  MessageRole,
  MessageSummary,
  ModelSummary,
  SessionSummary,
  SujiuBridge,
  TurnEvent,
  TurnEventKind
} from '../bridge/SujiuBridge';
import { AppearanceMode, PlatformServices } from '../platform/PlatformServices';

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

export function statusLabel(state: GenerationState): string {
  switch (state) {
    case GenerationState.Submitting:
      return 'Sending…';
    case GenerationState.Streaming:
      return 'Writing…';
    case GenerationState.WaitingForTool:
      return 'Waiting for tool…';
    case GenerationState.ExecutingTool:
      return 'Using a tool…';
    case GenerationState.ContinuingAfterTool:
      return 'Continuing…';
    default:
      return '';
  }
}

@Observed
export class ToolCallItem {
  id: string;
  toolName: string;
  statusLabel: string;
  summary: string = '';
  isError: boolean = false;

  constructor(id: string, toolName: string, statusLabel: string) {
    this.id = id;
    this.toolName = toolName;
    this.statusLabel = statusLabel;
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
  title: string;
  sessions: SessionSummary[];

  constructor(title: string, sessions: SessionSummary[]) {
    this.title = title;
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
  errorMessage: string = '';
  appearanceMode: AppearanceMode = AppearanceMode.System;

  private assistantOpen: boolean = false;
  private messageCounter: number = 0;

  constructor(bridge: SujiuBridge, platform: PlatformServices) {
    this.bridge = bridge;
    this.platform = platform;
  }

  get busy(): boolean {
    return isBusy(this.generationState);
  }

  get canSend(): boolean {
    return !this.busy && this.draft.trim().length > 0;
  }

  get statusText(): string {
    return statusLabel(this.generationState);
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

  newSession(): void {
    this.currentSessionId = '';
    this.messages = [];
    this.generationState = GenerationState.Idle;
    this.errorMessage = '';
  }

  setDraft(text: string): void {
    this.draft = text;
  }

  send(): void {
    const text: string = this.draft.trim();
    if (text.length === 0 || this.busy || this.currentSessionId.length === 0) {
      return;
    }
    this.assistantOpen = false;
    this.draft = '';
    this.errorMessage = '';
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
    this.appearanceMode = mode;
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
            item.toolStatusLabel ?? 'done'
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
      this.errorMessage = '';
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
        this.appendToAssistant((message: ChatMessageItem) => {
          message.thinking += event.text ?? '';
        });
        break;
      case TurnEventKind.ToolCallStarted:
        this.ensureAssistant();
        this.generationState = GenerationState.ExecutingTool;
        this.appendToAssistant((message: ChatMessageItem) => {
          message.toolCalls = message.toolCalls.concat([
            new ToolCallItem(event.callId ?? 'call', event.toolName ?? 'tool', 'running')
          ]);
        });
        break;
      case TurnEventKind.ToolCallFinished:
        this.generationState = GenerationState.ContinuingAfterTool;
        this.appendToAssistant((message: ChatMessageItem) => {
          message.toolCalls = message.toolCalls.map((call: ToolCallItem) => {
            if (call.id !== (event.callId ?? 'call')) {
              return call;
            }
            const updated = new ToolCallItem(call.id, call.toolName, event.isError ? 'failed' : 'done');
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
        this.errorMessage = event.message ?? 'The turn failed';
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
      new SessionGroup('Today', []),
      new SessionGroup('Yesterday', []),
      new SessionGroup('Earlier', [])
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
