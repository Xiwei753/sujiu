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
  tagline: string;
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
   * payload. Only the second argument carries the event.
   */
  sendTurn(
    request: TurnRequest,
    onEvent: (unused: Object | null, payload: string) => void
  ): Promise<string>;
  cancelTurn(): void;
}
