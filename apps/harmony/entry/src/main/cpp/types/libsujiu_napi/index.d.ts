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

/**
 * One character taking part in a conversation.
 *
 * A conversation is the root of a chat, and it holds zero, one or many
 * characters. Nothing here means "the main character": a tabletop is a
 * narrator plus several others, and it is stored the same way as a two-person
 * chat.
 */
export interface Participant {
  characterId: string;
  name: string;
  /** `character` or `narrator`, as a label a screen may show. */
  role: string;
}

export interface SessionSummary {
  id: string;
  /**
   * The first participant, kept for a screen that still shows one character.
   * Read `participants` for the conversation as it really is.
   */
  characterId: string;
  /** The first participant's name, for the same reason. */
  characterName: string;
  participants: Participant[];
  preview: string;
  title: string;
  updatedAtMs: number;
  messageCount: number;
}

/** One character a new conversation should include. */
export interface ParticipantRequest {
  characterId: string;
  /**
   * Optional: an unrecognised or missing label means an ordinary speaking
   * character. `narrator` covers a game master or narrator seat.
   */
  role?: string;
  displayName?: string;
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
  endpointId: string;
  /**
   * A word to show a person: the endpoint's own name, or a friendly one for an
   * address we recognise. It decides no protocol and no capability.
   */
  endpointLabel: string;
  configured: boolean;
}

export interface ContextSource {
  id: string;
  kind: string;
  name: string;
  description: string;
  recordCount: number;
}

/**
 * A persona, as a list row.
 *
 * A persona is a separate stored resource, not a column on a character. Its own
 * call is what lets a frontend list and delete one without opening a character,
 * which is what used to happen and is why personas went missing from the UI.
 */
export interface Persona {
  id: string;
  name: string;
  description: string;
  /** How many world books this persona brings of its own. */
  worldbookCount: number;
}

/**
 * A persona in full, for an editor.
 *
 * A different shape from `Persona` on purpose: an editor needs the bodies, and a
 * summary row cannot tell "empty" from "unchanged".
 */
export interface PersonaRequest {
  /** Omitted creates; an unknown id is refused rather than silently duplicated. */
  id?: string;
  name: string;
  description: string;
  userPrompt: string;
  worldbookIds: string[];
}

/** One lore entry. Lives inside a book; has no meaning outside one. */
export interface WorldBookEntry {
  id: string;
  name: string;
  /** Unshortened: truncating lore here is a presentation choice, not a bridge one. */
  content: string;
  /** Trigger words. Without one, the entry fires only when `constant` is set. */
  keys: string[];
  enabled: boolean;
  constant: boolean;
  /** `before_character`, `after_character` or `near_history`. */
  position: string;
}

export interface WorldBookEntryRequest {
  /** Omitted for a new entry. Present ids are kept, so record URIs stay valid. */
  id?: string;
  name: string;
  content: string;
  keys: string[];
  /** Omitted means enabled: an entry the author cannot see is worse than none. */
  enabled?: boolean;
  constant?: boolean;
  /** Unrecognised labels fall back to `after_character`. */
  position?: string;
}

/**
 * A world book, with its entries.
 *
 * An independent resource, meant to be bound to many characters, personas and
 * conversations at once. Binding carries an id, never a copy.
 */
export interface WorldBook {
  id: string;
  name: string;
  entries: WorldBookEntry[];
}

export interface WorldBookRequest {
  /** Omitted creates; an unknown id is refused rather than silently duplicated. */
  id?: string;
  name: string;
  entries: WorldBookEntryRequest[];
}

/**
 * A prompt profile, as a list row.
 *
 * The domain calls this a prompt profile; a screen may call it Prompts and never
 * mention the type name. The bodies are absent here, as they are for every other
 * resource row.
 */
export interface PromptProfile {
  id: string;
  name: string;
}

/**
 * A prompt profile in full, for an editor.
 *
 * Reusable conversation content — a system prompt, a user prompt, post-history
 * instructions, format rules — and not an app setting. Fixed segments already
 * inside a profile survive an edit made through this shape.
 */
export interface PromptProfileRequest {
  /** Omitted creates; an unknown id is refused rather than silently duplicated. */
  id?: string;
  name: string;
  systemPrompt: string;
  userPrompt: string;
  postHistoryInstructions: string;
  formatRules: string;
}

/**
 * A character in full, for an editor.
 *
 * Editing has to be possible, not only importing: a user who can add a character
 * but not correct its words would have to delete and recreate it, losing every
 * conversation bound to it.
 */
export interface CharacterRequest {
  /** Omitted creates; an unknown id is refused rather than silently duplicated. */
  id?: string;
  name: string;
  description: string;
  personality: string;
  scenario: string;
  firstMessage: string;
  alternateGreetings: string[];
  exampleDialogue: string;
  systemPrompt: string;
  postHistoryInstructions: string;
  worldbookIds: string[];
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
  /**
   * Which participant spoke, when the transcript says.
   *
   * A conversation can hold several characters, so `role: "assistant"` alone
   * does not say who is talking. Undefined means the step carries no
   * attribution: choosing who speaks next is a speaking-order policy, and the
   * runtime does not invent one.
   */
  speakerId?: string;
  text: string;
  toolCalls: ToolCall[];
}

export interface ConversationSnapshot {
  sessionId: string;
  /** The first participant, for a screen that still shows one character. */
  character?: CharacterSummary;
  participants: Participant[];
  /** The persona the user is playing in this conversation, if one is bound. */
  personaId?: string;
  /** The world books bound to this conversation directly. */
  worldbookIds: string[];
  /** The prompt profile this conversation compiles its prompt from, if any. */
  promptProfileId?: string;
  messages: Message[];
}

/**
 * What an endpoint turned out to be able to do.
 *
 * The model list and the protocol are two independent questions reported as two
 * independent answers: a gateway can list nothing and still speak the newest
 * protocol, and it can list fifty models and speak only the oldest one. One of
 * them failing says nothing about the other.
 */
export interface EndpointExploration {
  /** Empty when no protocol could be established; read `reason` then. */
  protocol: string;
  /**
   * The protocols confirmed to answer, best first. Normally a single entry:
   * negotiation stops at the first protocol that works, because probing the rest
   * would spend the user's own requests to learn about transports this build is
   * not going to use.
   */
  protocols: string[];
  /** Why negotiation stopped, when it stopped without settling on anything. */
  reason?: string;
  models: ModelDiscovery;
}

export interface ModelDiscovery {
  /**
   * One of `unknown`, `available`, `unavailable`, `permission_denied`,
   * `rate_limited`, `unreachable`.
   */
  listing: string;
  /** A sentence saying what this outcome does and does not mean. */
  note: string;
  /**
   * Whether the user may type a model name instead of choosing from a list.
   * True for every outcome: failing to enumerate models must never close the only
   * door the user has left.
   */
  manualEntryAllowed: boolean;
  models: ModelSummary[];
}

/**
 * A description of an endpoint.
 *
 * There is no vendor field. A user supplies an address and a key, and Sujiu works
 * out what is there, so naming a company here would be asking the user to decide
 * something the runtime settles by asking the endpoint.
 */
export interface ProviderConfig {
  id: string;
  name: string;
  baseUrl: string;
  /**
   * The chosen model, omitted before discovery has happened. Optional because
   * exploration runs first: the user is shown what an endpoint offers and then
   * picks a model out of it.
   */
  selectedModel?: string;
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
   * The wire protocols the runtime speaks, best first, e.g.
   * `['openai_responses', 'openai_chat_completions', 'anthropic_messages']`. This
   * replaced a list of vendors: a settings screen used to be handed a list of
   * companies to put in a dropdown, which asked the user to choose something
   * negotiation decides by asking the endpoint. Nothing here is a model to
   * offer, and no entry is a company.
   */
  supportedProtocols(): string[];
  listSessions(): SessionSummary[];
  listCharacters(query?: string): CharacterSummary[];
  listModels(): ModelSummary[];
  listContextSources(): ContextSource[];
  conversationState(sessionId: string): ConversationSnapshot;
  createSession(characterId: string): string;
  /**
   * Open a conversation with any number of characters.
   *
   * This is the form that needs no fiction about a main character: an empty
   * list is a conversation with nobody in it yet, one is the ordinary chat, and
   * several is a table. `createSession` is a shortcut into this one, not a
   * different storage model. Returns the new conversation id.
   */
  createConversation(
    participants: ParticipantRequest[],
    personaId?: string,
    worldbookIds?: string[],
    promptProfileId?: string
  ): string;
  /**
   * Replace what one conversation binds: participants, persona, world books and
   * prompt profile.
   *
   * This changes which stored resources a chat *uses* and never edits those
   * resources, so one persona bound in two conversations stays one persona. The
   * transcript is left alone: changing who is in a room is not rewriting what
   * has already been said in it.
   *
   * Throws when the conversation is unknown, and when a bound persona, character
   * or prompt profile does not exist. Naming a resource that is not there is a
   * mistake worth reporting, not a reason to store a dangling id.
   */
  setConversationBindings(
    sessionId: string,
    participants: ParticipantRequest[],
    personaId?: string,
    worldbookIds?: string[],
    promptProfileId?: string
  ): void;
  /**
   * Every stored persona, as list rows. Its own call rather than a column on a
   * character, because a persona is a separate entity.
   */
  listPersonas(): Persona[];
  /** One persona in full, or null when the id is unknown. */
  persona(id: string): PersonaRequest | null;
  /** Create or edit. Returns the id. Throws `entity_not_found` on a stale id. */
  savePersona(persona: PersonaRequest): string;
  /** Delete and unbind everywhere. False when the id was already unknown. */
  deletePersona(id: string): boolean;
  /** Every stored world book, each with its entries. */
  listWorldBooks(): WorldBook[];
  saveWorldBook(book: WorldBookRequest): string;
  deleteWorldBook(id: string): boolean;
  /** Every stored prompt profile, as list rows. */
  listPromptProfiles(): PromptProfile[];
  /** One prompt profile in full, or null when the id is unknown. */
  promptProfile(id: string): PromptProfileRequest | null;
  savePromptProfile(profile: PromptProfileRequest): string;
  deletePromptProfile(id: string): boolean;
  saveCharacter(character: CharacterRequest): string;
  /**
   * One character in full, or null when the id is unknown.
   *
   * `listCharacters` answers with summaries, and a summary carries no scenario,
   * no first message and no prompt bodies. An editor prefilled from one starts
   * blank over a character that is fully written.
   */
  character(id: string): CharacterRequest | null;
  deleteCharacter(id: string): boolean;
  /** Throws when `provider.baseUrl` is blank. Nothing else is validated here. */
  configureProvider(provider: ProviderConfig): void;
  /**
   * Works out what an endpoint can do from an address and a key alone.
   *
   * No endpoint has to be saved, no model has to be chosen, and no vendor is
   * named: this is the whole configuration flow, and the only thing left for the
   * user to decide afterwards is the model. `apiKey` is passed through and not
   * kept by the runtime.
   */
  discoverEndpoint(baseUrl: string, apiKey: string): EndpointExploration;
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
