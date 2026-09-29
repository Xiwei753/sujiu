/*
 * Application bridge contract for the HarmonyOS frontend.
 *
 * Provider neutral on purpose: the UI never sees a provider event shape, a
 * tool-call envelope or an HTTP detail. When `sujiu-ffi` grows the
 * conversation surface, only the bridge implementation changes.
 */

export interface SessionSummary {
  id: string;
  title: string;
  characterId: string;
  characterName: string;
  preview: string;
  updatedAt: number;
}

export interface CharacterSummary {
  id: string;
  name: string;
  tagline: string;
  tags: string[];
}

export interface ModelSummary {
  id: string;
  name: string;
  providerLabel: string;
  contextLabel: string;
}

export interface ContextSourceSummary {
  id: string;
  name: string;
  kindLabel: string;
  recordCount: number;
}

export enum MessageRole {
  User = 'user',
  Assistant = 'assistant',
  System = 'system'
}

export interface MessageSummary {
  id: string;
  role: MessageRole;
  text: string;
  toolName?: string;
  toolStatus?: string;
}

export interface ConversationSnapshot {
  session: SessionSummary;
  messages: MessageSummary[];
  contextSources: ContextSourceSummary[];
}

export enum TurnEventKind {
  Started = 'started',
  TextDelta = 'text_delta',
  ThinkingDelta = 'thinking_delta',
  /** The model asked for a tool; the runtime has not started it yet. */
  ToolCallRequested = 'tool_call_requested',
  /** The runtime is running the tool now. */
  ToolCallStarted = 'tool_call_started',
  ToolCallFinished = 'tool_call_finished',
  Completed = 'turn_completed',
  Failed = 'turn_failed',
  Cancelled = 'turn_cancelled'
}

/** Normalized turn event; every provider event is mapped onto this shape. */
export interface TurnEvent {
  kind: TurnEventKind;
  text?: string;
  callId?: string;
  toolName?: string;
  summary?: string;
  isError?: boolean;
  message?: string;
}

export type TurnEventListener = (event: TurnEvent) => void;

/**
 * Provider settings the user configured.
 *
 * This is a description, never a credential: the api key travels separately,
 * per turn, and the runtime does not keep it.
 */
export interface ProviderDraft {
  id: string;
  name: string;
  kind: string;
  baseUrl: string;
  model: string;
  maxTokens: number;
  temperature: number;
}

export interface SujiuBridge {
  listSessions(): Promise<SessionSummary[]>;
  listCharacters(query?: string): Promise<CharacterSummary[]>;
  listModels(): Promise<ModelSummary[]>;
  listContextSources(): Promise<ContextSourceSummary[]>;
  conversationState(sessionId: string): Promise<ConversationSnapshot>;
  /** Creates an empty session so a brand new chat can send its first message. */
  createSession(characterId: string): Promise<SessionSummary>;
  /** Provider kinds this runtime can actually drive, from the runtime itself. */
  supportedProviderKinds(): Promise<string[]>;
  /** Hands a provider description to the runtime. Rejects an unsupported kind. */
  configureProvider(draft: ProviderDraft): Promise<void>;
  /** The api key for the next turn. The runtime never stores it. */
  setCredential(secret: string): void;
  sendTurn(sessionId: string, userText: string, onEvent: TurnEventListener): void;
  cancelTurn(sessionId: string): void;
  runtimeSummary(): string;
}
