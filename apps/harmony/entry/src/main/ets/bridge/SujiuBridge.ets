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

/**
 * Who is in a conversation, as the runtime reports it.
 *
 * The name is resolved here so a screen can render a participant without a
 * second lookup, and it is the character's current name rather than a snapshot of
 * it: a participant is a reference, and the reference resolves to whatever the
 * character says it is now.
 */
export interface ConversationParticipant {
  characterId: string;
  name: string;
  /** `character` or `narrator`. */
  role: string;
}

/**
 * One conversation, its messages, and what it binds.
 *
 * The bindings are ids and nothing more. They are reported on the snapshot
 * because "which persona is this chat using" is a question about the
 * conversation, but the answer is a pointer: editing the persona is somebody
 * else's job, in the library.
 */
export interface ConversationSnapshot {
  session: SessionSummary;
  messages: MessageSummary[];
  contextSources: ContextSourceSummary[];
  participants: ConversationParticipant[];
  /** Absent when this conversation uses no persona, which is a normal state. */
  personaId?: string;
  worldbookIds: string[];
  /** Absent when this conversation uses no saved prompt set. */
  promptProfileId?: string;
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
  /**
   * How the protocol probe came out, as a code to branch on.
   *
   * One of: `supported`, `no_usable_protocol`, `credentials_rejected`,
   * `no_such_endpoint`, `rate_limited`, `server_unavailable`, `network_error`,
   * `unreadable`, `undetermined`.
   *
   * This exists because `reason` is a sentence and a sentence cannot be
   * switched on. "Probe failed" as a single outcome is the state this replaces:
   * a wrong key, a wrong path and a switched-off router read identically, and
   * each of them sends the user to edit a different field.
   *
   * `undetermined` matters most of these. It means the walk could not tell, not
   * that it found nothing, and a screen that renders it as a failure is
   * inventing a conclusion the runtime declined to reach.
   */
  status: string;
  /** Why negotiation stopped, when it stopped without settling on anything. */
  reason: string;
  models: ModelDiscovery;
}

/**
 * An endpoint that is already configured.
 *
 * This exists so a settings form can open on what is saved rather than on empty
 * fields, which reads as "nothing is set up" when something is. It carries the
 * address and the chosen model and nothing else. There is no credential here and
 * there should never be one: the key lives in platform storage and is read
 * immediately before a turn, so a secret is never something a view holds.
 */
export interface StoredEndpoint {
  id: string;
  baseUrl: string;
  /** Absent until a model is chosen, which is a normal state, not an error. */
  selectedModel?: string;
  /** A word to show the person. It describes; it decides nothing. */
  label: string;
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

/**
 * One line of the diagnostic log.
 *
 * The runtime writes this log before anything else in the configuration flow,
 * because without it a failure is only observable as a failure: there is no way
 * to tell an address problem from a key problem from a protocol the endpoint
 * does not speak from a model list that is merely empty. The log is what makes
 * those four questions separable after the fact.
 *
 * Every entry is already redacted on the runtime side. A key never reaches this
 * type, so a platform that renders a line, or copies it into a bug report,
 * cannot leak one by being careless — there is nothing to leak.
 */
export interface DiagnosticEntry {
  /** `discovery` for capability and model probing, `chat` for a real turn. */
  kind: string;
  /** Which step this was, e.g. `probe_start`, `protocol_result`, `turn_failed`. */
  stage: string;
  /** A sentence describing the step. */
  message: string;
  /** When it happened, in milliseconds since the epoch. */
  atMs: number;
  /** The whole line as one already-formatted, already-redacted string. */
  detail: string;
}

/**
 * A stored persona, as a list row.
 *
 * A persona is a first-class resource, not a column on a character. It used to
 * be reachable only by opening a character, which made it invisible: a user
 * could not list them, name them, or delete one without touching a character.
 * The bodies are deliberately absent here — a row is what you scan, and the
 * editor is a separate call that returns them.
 */
export interface PersonaSummary {
  id: string;
  name: string;
  description: string;
  /** How many world books this persona brings of its own. */
  worldbookCount: number;
}

/**
 * A persona in full, for an editor.
 *
 * A different shape from the row, not a fuller one. An editor needs something to
 * prefill and something to distinguish "empty" from "unchanged", and a summary
 * row can give it neither.
 */
export interface PersonaDraft {
  /** Absent for a persona that does not exist yet, which means create. */
  id?: string;
  name: string;
  description: string;
  /** The persona's own words as the user. Goes to the model verbatim. */
  userPrompt: string;
  /** Referenced world books, by id. Sharing is the point; copying is not. */
  worldbookIds: string[];
}

/**
 * One lore entry inside a world book.
 *
 * An entry has no meaning outside the book that holds it, so the runtime sends
 * a whole book at a time: an editor cannot save one entry without the rest.
 */
export interface WorldBookEntry {
  id: string;
  name: string;
  /**
   * The entry's text, unshortened.
   *
   * A preview here would be a truncation rule baked into the bridge, and picking
   * the interesting sentence of a piece of lore is not this layer's decision.
   */
  content: string;
  /** Trigger words. An entry with none fires only when `constant` is set. */
  keys: string[];
  enabled: boolean;
  /** Injected whether or not a key matched. */
  constant: boolean;
  /** `before_character`, `after_character` or `near_history`. */
  position: string;
}

/**
 * One lore entry as an editor holds it before saving.
 *
 * `id` absent means a new entry, and `enabled` absent means the runtime's
 * default, which is enabled: an entry somebody just wrote and cannot see is
 * worse than one that works.
 */
export interface WorldBookEntryDraft {
  id?: string;
  name: string;
  content: string;
  keys: string[];
  enabled?: boolean;
  constant?: boolean;
  position?: string;
}

/**
 * A stored world book.
 *
 * An independent resource. One book is meant to be bound to many characters,
 * personas and conversations at once, which is why binding carries an id and
 * never a copy.
 */
export interface WorldBookSummary {
  id: string;
  name: string;
  entries: WorldBookEntry[];
}

export interface WorldBookDraft {
  /** Absent for a book that does not exist yet, which means create. */
  id?: string;
  name: string;
  entries: WorldBookEntryDraft[];
}

/**
 * A stored prompt profile, as a list row.
 *
 * The domain calls this a prompt profile because that is what it is. A screen
 * calls it Prompts and never says the type name: an internal Rust type name is
 * not something a user has to recognise to manage their own writing.
 *
 * The bodies are absent here, for the same reason a persona row omits its own.
 */
export interface PromptProfileSummary {
  id: string;
  name: string;
}

/**
 * A prompt profile in full, for an editor.
 *
 * This is reusable conversation content — a system prompt, a user prompt,
 * post-history instructions and format rules — and it is emphatically not an app
 * setting. Two conversations may bind the same profile, and changing it changes
 * both.
 */
export interface PromptProfileDraft {
  /** Absent for a profile that does not exist yet, which means create. */
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
 * Editing has to be possible, not only importing: a user who can add a
 * character but not correct its words has to delete and recreate it, and that
 * loses every conversation bound to it.
 */
export interface CharacterDraft {
  /** Absent for a character that does not exist yet, which means create. */
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
  /** Referenced world books, by id. */
  worldbookIds: string[];
}

/**
 * One character taking part in a conversation, and how.
 *
 * A participant is a reference to a stored character, not a copy of it. Editing
 * the character afterwards is visible in every conversation it is in.
 */
export interface ParticipantBinding {
  characterId: string;
  /** `character` or `narrator`. Left out, the runtime uses `character`. */
  role?: string;
  displayName?: string;
}

export interface SujiuBridge {
  listSessions(): Promise<SessionSummary[]>;
  listCharacters(query?: string): Promise<CharacterSummary[]>;
  listModels(): Promise<ModelSummary[]>;
  listContextSources(): Promise<ContextSourceSummary[]>;
  conversationState(sessionId: string): Promise<ConversationSnapshot>;
  /** Creates an empty session so a brand new chat can send its first message. */
  createSession(characterId: string): Promise<SessionSummary>;

  /** Every stored persona, as list rows. Its own call, not part of a character. */
  listPersonas(): Promise<PersonaSummary[]>;
  /** One persona in full, for an editor. Undefined when there is no such id. */
  persona(id: string): Promise<PersonaDraft | undefined>;
  /**
   * Creates or edits a persona and resolves to its id.
   *
   * `id` absent means create and the runtime allocates one; `id` present and
   * unknown means the resource this editor holds is gone, and the runtime refuses
   * rather than silently making a second persona under a new id. An editor that
   * ignores this failure would fork the user's writing without telling them.
   */
  savePersona(draft: PersonaDraft): Promise<string>;
  /** Deletes a persona and unbinds it everywhere. Resolves false when unknown. */
  deletePersona(id: string): Promise<boolean>;

  /** Every stored world book, each with its entries. */
  listWorldBooks(): Promise<WorldBookSummary[]>;
  /**
   * Creates or edits a world book and resolves to its id.
   *
   * Entry ids that already exist are kept. Allocating a fresh id for an entry
   * that was only edited would change the context record it is addressed by, so
   * a model holding an earlier reference to that lore could no longer read it.
   */
  saveWorldBook(draft: WorldBookDraft): Promise<string>;
  /** Deletes a world book and unbinds it everywhere. Resolves false when unknown. */
  deleteWorldBook(id: string): Promise<boolean>;

  /** Every stored prompt profile, as list rows. */
  listPromptProfiles(): Promise<PromptProfileSummary[]>;
  /** One prompt profile in full, for an editor. Undefined when there is no such id. */
  promptProfile(id: string): Promise<PromptProfileDraft | undefined>;
  /**
   * Creates or edits a prompt profile and resolves to its id.
   *
   * Fixed segments already inside the profile survive an edit. An editor that
   * only knows the four prompt bodies must not be able to erase prompt segments
   * it has no field for, by saving a name.
   */
  savePromptProfile(draft: PromptProfileDraft): Promise<string>;
  /** Deletes a prompt profile and unbinds it everywhere. Resolves false when unknown. */
  deletePromptProfile(id: string): Promise<boolean>;

  /** Creates or edits a character and resolves to its id. */
  saveCharacter(draft: CharacterDraft): Promise<string>;
  /**
   * One character in full, for an editor to prefill from.
   *
   * Not the summary from `listCharacters`. A summary has no scenario, no first
   * message and no prompt bodies, so an editor prefilled from one starts blank
   * over a character that is fully written — and saving that replaces everything
   * with whatever survived in three fields.
   *
   * Resolves to nothing for a blank or unknown id, which is exactly what a "new
   * character" form wants: an empty form.
   */
  character(id: string): Promise<CharacterDraft | undefined>;
  /** Deletes a character and unbinds it everywhere. Resolves false when unknown. */
  deleteCharacter(id: string): Promise<boolean>;

  /**
   * Replaces what one conversation binds: its participants, persona, world books
   * and prompt profile.
   *
   * This is the conversation half of the split, and it is the whole reason the
   * split exists. It changes which stored resources this chat *uses* and never
   * edits those resources, so one persona bound in two conversations stays one
   * persona and fixing it fixes both.
   *
   * The transcript is left alone. Changing who is in a room is not rewriting
   * what has already been said in it, and dropping steps to make the room look
   * consistent would delete a record the next request has to replay.
   */
  setConversationBindings(
    sessionId: string,
    participants: ParticipantBinding[],
    personaId?: string,
    worldbookIds?: string[],
    promptProfileId?: string
  ): Promise<void>;
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
   *
   * `refresh` is what a "ask again" control passes. Left out, the runtime
   * answers from what this endpoint already told it, which is free; that is why
   * an ordinary open of the screen costs nothing and a fresh launch still has a
   * model list. Asking again is a deliberate act, because an endpoint that has
   * been re-pointed or a key that has just been corrected will not be
   * re-read otherwise.
   */
  discoverEndpoint(baseUrl: string, apiKey: string, refresh?: boolean): Promise<EndpointExploration>;
  /**
   * What an endpoint told us before, without asking it again.
   *
   * Nothing here reaches the network, which is the difference from
   * `discoverEndpoint`. A launch reads this so a model list is there before
   * anybody asks for one, and a screen that wants the current truth asks for it
   * instead. Resolves to nothing when this endpoint has never been asked about,
   * which is an ordinary state and not a failure.
   */
  rememberedEndpoint(baseUrl: string): Promise<EndpointExploration | undefined>;
  /** Hands a provider description to the runtime. Rejects a blank address. */
  configureProvider(draft: ProviderDraft): Promise<void>;
  /**
   * Reads back what is already configured, or null when nothing is.
   *
   * The settings form opens on this. An empty form is not neutral: it says
   * "nothing is set up" about a session that has an endpoint, a model and a
   * conversation behind it.
   */
  storedEndpoint(): Promise<StoredEndpoint | null>;
  /**
   * The diagnostic log, newest last.
   *
   * `limit` takes the most recent N lines; leave it off for everything held.
   * Refusing a request because no limit was given would be perverse: the whole
   * log is what a user needs when they are about to report a problem.
   *
   * The two halves — `discovery` and `chat` — are kept apart on purpose and can
   * be asked for separately. Mixing them makes a protocol switch during a turn
   * look like a probe result, which is exactly the confusion the log exists to
   * remove.
   */
  diagnostics(limit?: number): Promise<DiagnosticEntry[]>;
  /** Only the capability and model probing half of the log. */
  discoveryDiagnostics(limit?: number): Promise<DiagnosticEntry[]>;
  /** The whole log as plain text, for pasting into a bug report. */
  diagnosticsText(): Promise<string>;
  /**
   * Empties the log.
   *
   * Only the log. Configuration, conversations and the stored endpoint are
   * untouched, because a user clearing a log to start a fresh investigation is
   * not asking to lose their setup.
   */
  clearDiagnostics(): Promise<void>;
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
