/**
 * Shape of the Sujiu native runtime module, as ArkTS sees it.
 *
 * The implementation is Rust: crates/sujiu-napi re-exports the sujiu-ffi
 * runtime as a napi module, which ArkTS imports as `libsujiu_napi.so`. The
 * compiler cannot read the Rust definitions, so this file is the single
 * hand-maintained description of the module's shape and has to be kept in step
 * with crates/sujiu-napi/src/bridge.rs.
 *
 * API 26 does not type check napi module imports yet: the compiler reports
 * "Currently module for 'libsujiu_napi.so' is not verified" and ignores this
 * file, so the bridge below is effectively untyped today. Treat the file as the
 * contract of record: it is what the compiler will check once the SDK enables
 * napi verification, and it is what a reader should compare against the Rust.
 *
 * These types describe the module, not Sujiu's own domain model. The domain
 * model lives in Rust.
 */

export interface SessionSummary {
  id: string;
  characterId: string;
  characterName: string;
  preview: string;
  title: string;
  updatedAtMs: number;
  messageCount: number;
}

export interface CharacterSummary {
  id: string;
  name: string;
  /**
   * The character's description as authored, passed through unshortened.
   * Deciding how much of it a row shows is a presentation choice.
   */
  description: string;
}

export interface ModelSummary {
  id: string;
  name: string;
  providerId: string;
  providerName: string;
  kind: string;
  configured: boolean;
}

export interface ContextSource {
  id: string;
  kind: string;
  name: string;
  description: string;
  recordCount: number;
}

export interface ToolCall {
  id: string;
  name: string;
  title: string;
  status: string;
  isError: boolean;
  resultText: string;
}

export interface Message {
  id: string;
  role: string;
  text: string;
  toolCalls: ToolCall[];
}

export interface ConversationSnapshot {
  sessionId: string;
  character?: CharacterSummary;
  messages: Message[];
}

/** Only the OpenAI-compatible kind is implemented in the runtime today. */
export interface ProviderConfig {
  id: string;
  name: string;
  kind: string;
  baseUrl: string;
  model: string;
  maxTokens?: number;
  temperature?: number;
  /**
   * States that this endpoint requires a previous assistant tool call to be
   * replayed together with the reasoning that produced it. Optional on purpose:
   * most endpoints do not need it, and a known thinking-mode service is
   * recognised from `baseUrl` and `model` without it. Send it only to describe
   * a capability, never a protocol field name.
   */
  replaysAssistantReasoning?: boolean;
}

/**
 * One normalized turn event, serialized by the runtime.
 *
 * `kind` is a snake_case string and is the wire contract, not an implementation
 * detail of the field names: `turn_started`, `text_delta`, `thinking_delta`,
 * `tool_call_requested`, `tool_call_started`, `tool_call_finished`,
 * `turn_completed`, `turn_failed`, `turn_cancelled`. The other fields are
 * camelCase and are omitted when the event does not carry them, so read only
 * what the kind implies:
 *
 * - a text event carries `text` as one delta, not the whole answer;
 * - a tool event carries `toolName` and `toolCallId`, and the finished one also
 *   carries `text` as a short result summary plus `isError`;
 * - a failure carries `text` describing what went wrong.
 */
export interface TurnEvent {
  kind: string;
  text?: string;
  toolName?: string;
  toolCallId?: string;
  isError?: boolean;
}

export interface TurnRequest {
  sessionId: string;
  userText: string;
  /**
   * The runtime owns the app-level prompt. The request is decoded with unknown
   * fields rejected, so passing `appSystemPrompt` here is an error, not a no-op.
   */
  provider?: ProviderConfig;
  apiKey?: string;
}

export const coreVersion: () => string;

export const create: () => SujiuRuntimeBridge;

export class SujiuRuntimeBridge {
  coreVersion(): string;
  /**
   * Provider kinds the runtime can actually drive, e.g.
   * `['openai_compatible']`. A settings screen must offer only these: a kind
   * that is not listed is rejected by `configureProvider` and would fail every
   * turn.
   */
  providerKinds(): string[];
  listSessions(): SessionSummary[];
  listCharacters(query?: string): CharacterSummary[];
  listModels(): ModelSummary[];
  listContextSources(): ContextSource[];
  conversationState(sessionId: string): ConversationSnapshot;
  createSession(characterId: string): string;
  /** Throws when `provider.kind` is not in `providerKinds()`. */
  configureProvider(provider: ProviderConfig): void;
  /**
   * Runs a turn and reports every normalized event as JSON.
   *
   * The native callback receives two arguments: the callback return value,
   * which this module never uses and is therefore always null, and the
   * payload, which is one serialized `TurnEvent`. Only the second argument
   * carries the event. Every kind from `turn_started` onwards arrives, and a
   * turn always ends with exactly one of `turn_completed`, `turn_failed` or
   * `turn_cancelled`.
   */
  sendTurn(
    request: TurnRequest,
    onEvent: (unused: Object | null, payload: string) => void
  ): Promise<string>;
  cancelTurn(): void;
  useDataDirectory(directory: string): Promise<void>;
  dataDirectory(): string | null;
}
