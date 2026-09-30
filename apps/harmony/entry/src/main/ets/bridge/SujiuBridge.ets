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
  description: string;
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
 * What an endpoint turned out to be able to do.
 *
 * The model list and the protocol are two independent questions and are reported
 * as two independent answers: a gateway can list nothing and still speak the
 * newest protocol, and it can list fifty models and speak only the oldest one.
 * One of them failing says nothing about the other.
 */
export interface EndpointExploration {
  /** The protocol that was settled on, empty when none could be established. */
  protocol: string;
  /** The protocols confirmed to answer, best first. Normally a single entry. */
  protocols: string[];
  /** Why negotiation stopped, when it stopped without settling on anything. */
  reason: string;
  models: ModelDiscovery;
}

export interface ModelDiscovery {
  /** One of: unknown, available, unavailable, permission_denied, rate_limited, unreachable. */
  listing: string;
  /** A sentence for the person, saying what this outcome does and does not mean. */
  note: string;
  /**
   * Whether the user may type a model name instead of choosing from a list.
   *
   * This is true for every outcome. A gateway with no listing route is still
   * perfectly able to chat, so failing to enumerate models must never close the
   * only door the user has left.
   */
  manualEntryAllowed: boolean;
  models: ModelSummary[];
}

/**
 * Provider settings the user configured.
 *
 * This is a description, never a credential: the api key travels separately,
 * per turn, and the runtime does not keep it.
 *
 * There is no vendor field. A user supplies an address and a key and Sujiu works
 * out what is there, so naming a company here would be asking the user to decide
 * something the runtime settles by asking the endpoint.
 */
export interface ProviderDraft {
  id: string;
  name: string;
  baseUrl: string;
  /**
   * The chosen model, or unset before discovery has happened.
   *
   * Optional because the order of work is: type an address and a key, find out
   * what is there, and only then choose a model out of it. Requiring a model
   * here would put the choice before the question it is chosen from.
   */
  selectedModel?: string;
  /**
   * Optional sampling overrides. Left out, the runtime applies its own defaults,
   * so a platform does not have to restate provider semantics to describe an
   * endpoint.
   */
  maxTokens?: number;
  temperature?: number;
  /**
   * States that this endpoint requires a previous assistant tool call to be
   * replayed together with the reasoning that produced it.
   *
   * Optional on purpose: a known thinking-mode service is recognised from the
   * endpoint and model alone, so a settings form does not have to know that this
   * is a protocol requirement. Set it only to describe a capability the runtime
   * cannot infer, and never a wire field name.
   */
  replaysAssistantReasoning?: boolean;
}

export interface SujiuBridge {
  listSessions(): Promise<SessionSummary[]>;
  listCharacters(query?: string): Promise<CharacterSummary[]>;
  listModels(): Promise<ModelSummary[]>;
  listContextSources(): Promise<ContextSourceSummary[]>;
  conversationState(sessionId: string): Promise<ConversationSnapshot>;
  /** Creates an empty session so a brand new chat can send its first message. */
  createSession(characterId: string): Promise<SessionSummary>;
  /**
   * The wire protocols this runtime speaks, best first, from the runtime itself.
   *
   * This replaced a list of vendors. A frontend used to be handed a list of
   * companies to put in a dropdown, which asked the user to choose something
   * negotiation decides by asking the endpoint.
   */
  supportedProtocols(): Promise<string[]>;
  /**
   * Works out what an endpoint can do from an address and a key alone.
   *
   * Nothing is stored, and no model needs to be known: the settings screen asks
   * this first, shows what came back, and the user then does the one thing only
   * a person can do, which is pick a model.
   */
  discoverEndpoint(baseUrl: string, apiKey: string): Promise<EndpointExploration>;
  /** Hands a provider description to the runtime. Rejects a blank address. */
  configureProvider(draft: ProviderDraft): Promise<void>;
  /**
   * Runs the next turn against `draft` instead of the saved provider, without
   * saving it.
   *
   * Only for a genuine one-turn override. An ordinary turn sends no provider
   * at all and lets the runtime use what was already saved, because a resent
   * description is a restatement: a field the caller left out is not left
   * unchanged, it is the saved value being overridden with a default.
   */
  withProviderForNextTurn(draft: ProviderDraft): void;
  /** The api key for the next turn. The runtime never stores it. */
  setCredential(secret: string): void;
  /**
   * Attaches a directory the runtime may persist to.
   *
   * Optional: a bridge that keeps everything in memory reports false here and
   * the frontend carries on. Attaching a directory that already holds a
   * document restores it.
   */
  useDataDirectory(directory: string): Promise<boolean>;
  sendTurn(sessionId: string, userText: string, onEvent: TurnEventListener): void;
  cancelTurn(sessionId: string): void;
  runtimeSummary(): string;
}
