import {
  CharacterDraft,
  CharacterSummary,
  ContextSourceSummary,
  ConversationParticipant,
  DiagnosticEntry,
  EndpointExploration,
  MessageRole,
  MessageSummary,
  ModelSummary,
  ParticipantBinding,
  PersonaDraft,
  PersonaSummary,
  PromptProfileDraft,
  PromptProfileSummary,
  SessionSummary,
  ProviderDraft,
  StoredEndpoint,
  SujiuBridge,
  TurnEvent,
  TurnEventKind,
  WorldBookDraft,
  WorldBookSummary
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

/**
 * Where a settings screen is in the one flow that matters: type an address, give
 * a key, find out what is there, pick a model.
 *
 * These are states rather than sentences because each one tells the user which
 * field to edit next, and a single "probe failed" message cannot: a wrong key
 * and a wrong address are both failures and share no remedy. Anything that
 * collapses them leaves the user guessing which of the two fields they got
 * wrong, which is the same as having no diagnostic at all.
 *
 * The three pre-probe states come first in the flow and are not errors. Not
 * having typed an address yet is how the screen starts.
 */
export enum EndpointState {
  /** No address yet. The first thing to ask for, and not a fault. */
  NeedAddress = 'NeedAddress',
  /** Address given, no key. Nothing can be learned without one. */
  NeedKey = 'NeedKey',
  /** Address and key given, nothing asked yet. */
  Ready = 'Ready',
  /** Asking the endpoint now. */
  Probing = 'Probing',
  /** A protocol was settled on. The endpoint can be used. */
  Usable = 'Usable',
  /** Settled on, and a model list came back. */
  ModelsFound = 'ModelsFound',
  /**
   * A protocol was settled on but the model list did not come back.
   *
   * Not a failure. A gateway with no listing route still holds a conversation,
   * so this state keeps the endpoint configurable and only changes the
   * advice: type the model name instead of choosing one.
   */
  ListingUnavailable = 'ListingUnavailable',
  /** 401/403. The key is the thing to change. */
  AuthFailed = 'AuthFailed',
  /** The address names a route the endpoint does not have. The address is wrong. */
  NoSuchEndpoint = 'NoSuchEndpoint',
  /** The request never landed. The network, not the configuration. */
  NetworkError = 'NetworkError',
  /** The endpoint answered and is unwell. Waiting is the only fix. */
  ServerError = 'ServerError',
  /** Rate limited right now. Also a wait. */
  RateLimited = 'RateLimited',
  /** Every protocol answered and none of them work here. */
  NoUsableProtocol = 'NoUsableProtocol',
  /** The walk could not tell. Deliberately not reported as a failure. */
  Undetermined = 'Undetermined',
  /** The endpoint answered something this build cannot read. */
  Unreadable = 'Unreadable'
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
   * The stored resources a library page lists.
   *
   * Four separate lists, matching four separate domain entities. They are not
   * folded into one "library" array because a shape that returned all of them at
   * once would read as one resource again, and would force a page showing four
   * sections to hold every prompt body in order to render the character list.
   */
  personas: PersonaSummary[] = [];
  worldBooks: WorldBookSummary[] = [];
  promptProfiles: PromptProfileSummary[] = [];

  /**
   * What the open conversation currently binds.
   *
   * Ids and references, never copies. This is the other half of the split from
   * the three lists above: those answer "what do I have", these answer "what does
   * this chat use". The same persona bound in two conversations is one persona,
   * and editing it in the library is visible in both.
   */
  conversationParticipants: ConversationParticipant[] = [];
  conversationPersonaId: string = '';
  conversationWorldbookIds: string[] = [];
  conversationPromptProfileId: string = '';

  /** Whether a library write is in flight, so an editor can disable its save. */
  libraryBusy: boolean = false;

  /**
   * Provider setup. The description is state; the credential is not. The
   * controller asks the platform credential service for the key right before a
   * turn, so a secret never becomes presentation state and never reaches a view.
   */
  protocols: string[] = [];
  providerDraft: ProviderDraft | null = null;
  hasCredential: boolean = false;
  savingProvider: boolean = false;
  providerNoticeCode: string = '';

  /**
   * What the last exploration of an address and a key found, and whether one has
   * been run. Held separately from the saved provider because exploration happens
   * first: the user is choosing a model out of what came back, not out of what an
   * earlier choice left behind.
   */
  exploringEndpoint: boolean = false;
  explored: boolean = false;
  exploration: EndpointExploration | null = null;
  endpointNote: string = '';

  /**
   * How the address and the key currently stand, for a settings screen.
   *
   * Recomputed on demand rather than stored, because it is a function of the
   * form, the probe and the answer — and a cached copy of that is stale the
   * moment the user types, which is exactly when it would be read.
   */
  endpointState(baseUrl: string, secret: string): EndpointState {
    const address: string = baseUrl.trim();
    const key: string = secret.trim();
    if (address.length === 0) {
      return EndpointState.NeedAddress;
    }
    // A stored key counts. Otherwise a user who configured an endpoint and
    // reopened settings to change the model would be told to type a key they
    // had already given, which is both false and unhelpful.
    if (key.length === 0 && !this.hasCredential) {
      return EndpointState.NeedKey;
    }
    if (this.exploringEndpoint) {
      return EndpointState.Probing;
    }
    if (!this.explored || this.exploration === null) {
      return EndpointState.Ready;
    }

    const status: string = this.exploration.status;
    const settled: boolean = status === 'supported';
    if (!settled) {
      switch (status) {
        case 'credentials_rejected':
          return EndpointState.AuthFailed;
        case 'no_such_endpoint':
          return EndpointState.NoSuchEndpoint;
        case 'rate_limited':
          return EndpointState.RateLimited;
        case 'server_unavailable':
          return EndpointState.ServerError;
        case 'network_error':
          return EndpointState.NetworkError;
        case 'no_usable_protocol':
          return EndpointState.NoUsableProtocol;
        case 'undetermined':
          return EndpointState.Undetermined;
        default:
          return EndpointState.Unreadable;
      }
    }

    // A protocol settled. The model list is a separate question with a
    // separate answer, and its absence is not a failure of the endpoint.
    if (this.exploration.models.listing === 'available' && this.models.length > 0) {
      return EndpointState.ModelsFound;
    }
    return EndpointState.ListingUnavailable;
  }

  /**
   * How many models discovery found, for a count in the status line.
   *
   * Reported rather than assumed: "found 3" and "found 0" are different facts,
   * and a screen that only says "no models" when there are none cannot tell a
   * user that the list it is showing is the whole list.
   */
  get discoveredModelCount(): number {
    return this.exploration === null ? 0 : this.exploration.models.models.length;
  }

  /**
   * The models this endpoint actually listed, in the order it listed them.
   *
   * Deliberately not `this.models`. That is the chat model list — what has been
   * configured, filtered for the conversation header — and it stays at one entry
   * when exactly one model is saved. Reading it as "the models you can choose
   * from" shows the already-configured model and nothing else, which looks
   * like a probe that found one model and is the difference between a settings
   * screen that offers the endpoint's choice and one that cannot.
   *
   * Probing deliberately does not write into `this.models` either. An
   * unconfigured endpoint's catalogue is not a conversation's model list, and
   * merging the two makes an unrelated conversation start offering models from
   * an address the user has not finished configuring.
   *
   * The whole list is returned, with no cap. There used to be a limit of twelve
   * here, which existed only because the models were rendered as chips down the
   * page and the ones past it pushed the save button out of reach. They are
   * rendered now in a bounded, scrollable list, so every model the endpoint
   * reported is reachable by scrolling — which is strictly more than the cap
   * ever offered, since a model past the cap could not be chosen at all.
   */
  get discoveredModels(): ModelSummary[] {
    return this.exploration === null ? [] : this.exploration.models.models;
  }

  /**
   * Whether a model may be typed instead of chosen.
   *
   * Always true, and deliberately not derived from the outcome. A gateway with
   * no listing route is still perfectly able to chat, so failing to enumerate
   * models must never take away the only door the user has left. The runtime
   * reports the same thing and the two agreeing is the point: neither side gets
   * to close it on its own.
   */
  get manualModelEntryAllowed(): boolean {
    if (this.exploration === null) {
      return true;
    }
    return this.exploration.models.manualEntryAllowed;
  }

  /**
   * The listing outcome as a code, so a screen can say it in its own language.
   *
   * The runtime also sends an English sentence about it, and that sentence is
   * for the diagnostic log and for a developer reading one. Printing it inside
   * a localised form produces the worst of both: an untranslated clause in the
   * middle of a translated sentence, which is harder to read than either
   * language alone. A code is the part the runtime owns; the wording is the
   * platform's, and a platform that ships three languages has to be the one
   * choosing.
   */
  get listingCode(): string {
    return this.exploration === null ? 'unknown' : this.exploration.models.listing;
  }

  /**
   * What is already configured, so a form can open on it.
   *
   * The address and the chosen model are configuration, and a person opening
   * settings to change one of them should see the other rather than an empty
   * field. The credential is deliberately absent from this and cannot be
   * requested: it lives in platform storage and is read immediately before a
   * turn, so that a secret is never something a view could print.
   */
  storedEndpoint: StoredEndpoint | null = null;

  /**
   * The diagnostic log, newest last.
   *
   * Every line is already redacted in the runtime, so a settings screen can
   * render these directly and a bug report can quote them. The two halves are
   * kept distinguishable by `kind`, because a protocol switch during a turn
   * and a probe result read as the same event unless something says which is
   * which.
   */
  diagnostics: DiagnosticEntry[] = [];

  /**
   * Raised when the appearance mode changes. Presentation does not decide what
   * that means for colors; the UI layer listens and re-themes itself.
   */
  onAppearanceChanged: () => void = () => {
  };

  /**
   * Raised when a turn reaches any of its three end states: completed, failed or
   * cancelled. A callback rather than a flag, because whether a turn is still
   * running is presentation's business while what should happen to the process
   * around it belongs to the platform layer that owns the lifecycle.
   */
  onTurnSettled: () => void = () => {
  };

  /**
   * The single place a turn is declared over.
   *
   * A turn ends in exactly three ways and each of them is a real ending: it
   * finished, it failed, or the person stopped it. A fourth possibility — the
   * runtime simply never answered — is not an ending, and it is why the agent
   * loop still commits whatever it had when a turn stops for any reason. So the
   * three are reported together rather than letting each of them forget the
   * others, which is how a background hold ends up outliving the turn that
   * asked for it.
   */
  private settleTurn(): void {
    this.onTurnSettled();
  }

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
      this.protocols = await this.bridge.supportedProtocols();
    } catch (error) {
      this.protocols = [];
    }
    try {
      // Read back what is saved, so the form opens on it. An endpoint that was
      // never configured leaves this null, which is a normal state and not a
      // reason to hide the form.
      this.storedEndpoint = await this.bridge.storedEndpoint();
    } catch (error) {
      this.storedEndpoint = null;
    }
    this.hasCredential = this.credentials.hasSecret(CREDENTIAL_NAME);

    // Read back what this endpoint told us last time, so the model list is
    // there before anybody asks for one. This asks nothing: a launch that
    // quietly re-probed would cost every user a round trip every time they
    // opened the app. It is a restore, not a probe, so it does not flash a
    // "probing" state that no probe is running.
    await this.restoreRememberedEndpoint();
  }

  /**
   * Asks an endpoint what it can do, before anything has been saved.
   *
   * The order of work matters: the user types an address and a key, this reports
   * which models are there and which protocol answers, and only then is a model
   * chosen. Requiring a model first would put the choice in front of the question
   * it is chosen from, and would make the endpoint unsaveable until the user had
   * guessed the very answer the runtime could have looked up.
   */
  /**
   * Ask an endpoint what it can do, or ask it again.
   *
   * `refresh` is the re-probe control, and it is the only thing that sets it.
   * Left false, the runtime answers from what this endpoint already said, which
   * is what makes opening the settings screen free and a fresh launch still have
   * a model list. Set true after the user has changed something the last answer
   * cannot know about: a key, a gateway's routing, or simply suspicion that
   * what is on screen is old.
   */
  /**
   * Put the remembered exploration on screen, if there is one.
   *
   * Silent when there is nothing to restore. An endpoint nobody has asked about
   * yet is the normal state of a fresh configuration, and inventing an
   * "explored" flag for it would claim a probe happened.
   */
  private async restoreRememberedEndpoint(): Promise<void> {
    const stored: StoredEndpoint | null = this.storedEndpoint;
    if (stored === null || stored.baseUrl.trim().length === 0) {
      return;
    }
    try {
      const remembered: EndpointExploration | undefined =
        await this.bridge.rememberedEndpoint(stored.baseUrl.trim());
      if (remembered === undefined) {
        return;
      }
      this.exploration = remembered;
      this.endpointNote = remembered.models.note;
      this.explored = true;
    } catch (error) {
      hilog.error(0x0000, 'Sujiu', 'rememberedEndpoint failed: %{public}s', (error as Error).message);
    }
  }

  async exploreEndpoint(baseUrl: string, secret: string, refresh: boolean = false): Promise<void> {
    if (this.exploringEndpoint || baseUrl.trim().length === 0) {
      return;
    }
    this.exploringEndpoint = true;
    this.providerNoticeCode = '';
    this.explored = false;
    try {
      // A stored key is used when the field is empty, so a user who configured
      // an endpoint and came back to change the model can re-probe without
      // retyping a secret they cannot see. The typed one always wins: it is
      // what the user is trying right now.
      const key: string = secret.trim().length > 0
        ? secret.trim()
        : this.credentials.loadSecret(CREDENTIAL_NAME);
      const found: EndpointExploration = await this.bridge.discoverEndpoint(baseUrl.trim(), key, refresh);
      this.exploration = found;
      this.endpointNote = found.models.note;
      // A typed model is the user's own words, so it is kept whatever the
      // listing said. Losing it because a gateway has no listing route would
      // punish the user for the gateway's shortcoming.
      if (found.models.models.length === 0 && !found.models.manualEntryAllowed) {
        this.providerNoticeCode = 'provider_no_models';
      }
      this.explored = true;
      await this.loadDiagnostics();
    } catch (error) {
      hilog.error(0x0000, 'Sujiu', 'discoverEndpoint failed: %{public}s', (error as Error).message);
      this.fail('provider_not_saved', (error as Error).message);
    } finally {
      this.exploringEndpoint = false;
    }
  }

  /**
   * Reads the diagnostic log, newest last.
   *
   * Loaded after every probe and every turn rather than on demand, because a
   * log nobody has to ask for is the only kind anyone reads when something goes
   * wrong. It is not shown until a settings screen asks to see it; the cost is
   * a few hundred small strings that the runtime already had in memory.
   */
  async loadDiagnostics(limit: number = 0): Promise<void> {
    try {
      this.diagnostics = await this.bridge.diagnostics(limit);
    } catch (error) {
      hilog.error(0x0000, 'Sujiu', 'diagnostics failed: %{public}s', (error as Error).message);
      this.diagnostics = [];
    }
  }

  /** Only the capability and model probing half, for a settings screen. */
  async loadDiscoveryDiagnostics(limit: number = 0): Promise<void> {
    try {
      this.diagnostics = await this.bridge.discoveryDiagnostics(limit);
    } catch (error) {
      hilog.error(0x0000, 'Sujiu', 'discovery diagnostics failed: %{public}s', (error as Error).message);
      this.diagnostics = [];
    }
  }

  /** The whole log as text, for pasting into a bug report. Already redacted. */
  async diagnosticsText(): Promise<string> {
    try {
      return await this.bridge.diagnosticsText();
    } catch (error) {
      hilog.error(0x0000, 'Sujiu', 'diagnostics text failed: %{public}s', (error as Error).message);
      return '';
    }
  }

  /**
   * Puts the whole log on the clipboard, for a bug report.
   *
   * Goes through the platform clipboard rather than being handed to a view,
   * because the text is exactly the thing a view should not be holding: it is
   * unbounded, it is only useful somewhere else, and a view that held it could
   * render it in a screenshot by accident.
   *
   * Resolves false when there is nothing to copy, so the caller can decline to
   * claim success it did not have.
   */
  async copyDiagnostics(): Promise<boolean> {
    const text: string = await this.diagnosticsText();
    if (text.length === 0) {
      return false;
    }
    this.platform.clipboard.copyText('Sujiu', text);
    return true;
  }

  /** Empties the log. Configuration and conversations are untouched. */
  clearDiagnostics(): void {
    this.bridge.clearDiagnostics().then(() => {
      this.diagnostics = [];
    }).catch((error: Error): void => {
      hilog.error(0x0000, 'Sujiu', 'clear diagnostics failed: %{public}s', error.message);
    });
  }

  /**
   * Turns the three fields a settings form actually collects into a provider
   * description. Everything else is decided here rather than by the view: no
   * vendor is named, and sampling parameters are provider semantics the runtime
   * already defaults, so the form cannot set values that disagree with how a
   * turn really runs.
   *
   * The address is the only required field. A model is not, because the flow
   * is: probe, find out what is there, then choose. Making the model a
   * precondition here put the choice in front of the question it is chosen from
   * and made the endpoint un-saveable until the user had guessed the very
   * answer the runtime could look up. An endpoint with no model saved is a
   * normal state — it cannot chat yet, and the user is told so — not a refusal.
   */
  async saveProviderFromForm(baseUrl: string, model: string, secret: string): Promise<void> {
    const address: string = baseUrl.trim();
    if (address.length === 0) {
      this.fail('provider_need_address');
      return;
    }
    const chosen: string = model.trim();
    await this.saveProvider({
      id: 'endpoint-default',
      // The name is for display and defaults to the address when no model has
      // been chosen yet, so an endpoint saved before the model step still has
      // something to show in a list.
      name: chosen.length > 0 ? chosen : address,
      baseUrl: address,
      selectedModel: chosen.length > 0 ? chosen : undefined
    } as ProviderDraft, secret);
  }

  /**
   * Saves a provider description and, when given, the api key. The description
   * goes to the runtime; the key goes to the credential service. A blank address
   * fails here instead of at the first turn.
   */
  async saveProvider(draft: ProviderDraft, secret: string): Promise<boolean> {
    if (this.savingProvider) {
      // Reported as a failure rather than a silent no-op. A caller that
      // changes what is on screen afterwards has to know whether this reached
      // the runtime, and returning true here would be a lie the caller cannot
      // detect.
      return false;
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
      return true;
    } catch (error) {
      // The UI shows a localized code, so the runtime detail is logged here.
      hilog.error(0x0000, 'Sujiu', 'saveProvider failed: %{public}s', (error as Error).message);
      this.fail('provider_not_saved', (error as Error).message);
      return false;
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
    this.loadLibrary();
  }

  /**
   * Reloads the four resource lists a library page shows.
   *
   * Called from `refresh` so that saving or deleting a resource is followed by
   * the list it belongs to being correct. Reading a list that a write has already
   * invalidated is how a deleted world book keeps showing up as bound.
   */
  loadLibrary(): void {
    this.bridge.listPersonas().then((rows: PersonaSummary[]) => {
      this.personas = rows;
    }).catch((error: Error) => {
      this.fail('library_unavailable', error.message);
    });
    this.bridge.listWorldBooks().then((rows: WorldBookSummary[]) => {
      this.worldBooks = rows;
    }).catch((error: Error) => {
      this.fail('library_unavailable', error.message);
    });
    this.bridge.listPromptProfiles().then((rows: PromptProfileSummary[]) => {
      this.promptProfiles = rows;
    }).catch((error: Error) => {
      this.fail('library_unavailable', error.message);
    });
  }

  personaById(id: string): PersonaSummary | undefined {
    return this.personas.find((item: PersonaSummary) => item.id === id);
  }

  worldBookById(id: string): WorldBookSummary | undefined {
    return this.worldBooks.find((item: WorldBookSummary) => item.id === id);
  }

  promptProfileById(id: string): PromptProfileSummary | undefined {
    return this.promptProfiles.find((item: PromptProfileSummary) => item.id === id);
  }

  /**
   * Whether a conversation binds this resource, for a checkmark in the picker.
   *
   * Compares ids and resolves the name for display separately. Comparing names
   * would make two personas that share a name indistinguishable, and renaming one
   * would silently unbind it.
   */
  isPersonaBound(id: string): boolean {
    return this.conversationPersonaId === id;
  }

  isWorldBookBound(id: string): boolean {
    return this.conversationWorldbookIds.includes(id);
  }

  isPromptProfileBound(id: string): boolean {
    return this.conversationPromptProfileId === id;
  }

  isCharacterBound(characterId: string): boolean {
    return this.conversationParticipants.some(
      (item: ConversationParticipant) => item.characterId === characterId);
  }

  /**
   * The four things one conversation binds, written to the runtime.
   *
   * Empty answers are sent as empty, not as "leave it alone". A binding screen
   * where unticking the last world book cannot unbind it is a screen whose
   * picture and the stored conversation disagree, and the model is the one that
   * has to live with the difference.
   *
   * Nothing here edits a resource. That is the boundary: this chooses among
   * stored things, and the library is where they are written.
   */
  bindResources(
    participants: string[],
    personaId: string,
    worldbookIds: string[],
    promptProfileId: string
  ): void {
    const sessionId: string = this.currentSessionId;
    if (sessionId.length === 0) {
      this.fail('session_not_found');
      return;
    }
    const bindings: ParticipantBinding[] = participants.map((characterId: string) => {
      return { characterId: characterId } as ParticipantBinding;
    });
    this.libraryBusy = true;
    this.bridge.setConversationBindings(
      sessionId,
      bindings,
      personaId.length === 0 ? undefined : personaId,
      worldbookIds,
      promptProfileId.length === 0 ? undefined : promptProfileId
    ).then(() => {
      this.libraryBusy = false;
      this.clearError();
      // Read the bindings back rather than assuming them. The runtime decides
      // what a participant resolves to, and a page that showed the pre-write
      // selection would be reporting an answer it never received.
      this.loadConversation(sessionId);
      this.loadLibrary();
    }).catch((error: Error) => {
      this.libraryBusy = false;
      this.fail('bind_failed', error.message);
    });
  }

  /**
   * Saves any stored resource and reloads the lists afterwards.
   *
   * One place for four kinds on purpose. A save that left the lists alone would
   * make a resource look unsaved on the next screen, and four hand-written copies
   * of that logic is four chances to forget the reload.
   */
  private writeLibrary<T>(work: () => Promise<T>, onDone: (value: T) => void): void {
    this.libraryBusy = true;
    this.clearError();
    work().then((value: T) => {
      this.libraryBusy = false;
      onDone(value);
      this.loadLibrary();
      this.refreshSessions();
    }).catch((error: Error) => {
      this.libraryBusy = false;
      this.fail('save_failed', error.message);
    });
  }

  /** The names a binding screen shows, resolved from ids, in the ids' order. */
  worldBookNames(ids: string[]): string[] {
    const names: string[] = [];
    for (let i = 0; i < ids.length; i++) {
      const book: WorldBookSummary | undefined = this.worldBookById(ids[i]);
      names.push(book === undefined ? ids[i] : book.name);
    }
    return names;
  }

  /**
   * One character in full, for an editor to prefill from.
   *
   * Resolves to nothing when the id is unknown or blank, which is the same
   * answer a "new character" editor wants: an empty form. An editor that had to
   * special-case the difference would end up carrying a half-built character for
   * a resource that is not there.
   */
  loadCharacterDraft(id: string): Promise<CharacterDraft | undefined> {
    if (id.length === 0) {
      return Promise.resolve(undefined);
    }
    return this.bridge.character(id);
  }

  loadPersonaDraft(id: string): Promise<PersonaDraft | undefined> {
    if (id.length === 0) {
      return Promise.resolve(undefined);
    }
    return this.bridge.persona(id);
  }

  loadPromptProfileDraft(id: string): Promise<PromptProfileDraft | undefined> {
    if (id.length === 0) {
      return Promise.resolve(undefined);
    }
    return this.bridge.promptProfile(id);
  }

  saveCharacter(draft: CharacterDraft): void {
    this.writeLibrary(
      () => this.bridge.saveCharacter(draft),
      (_id: string) => {
        this.characters = [];
        this.searchCharacters(this.characterQuery);
      });
  }

  savePersona(draft: PersonaDraft): void {
    this.writeLibrary(
      () => this.bridge.savePersona(draft),
      (_id: string) => {
        this.characters = [];
        this.searchCharacters(this.characterQuery);
      });
  }

  saveWorldBook(draft: WorldBookDraft): void {
    this.writeLibrary(() => this.bridge.saveWorldBook(draft), (_id: string) => {
    });
  }

  savePromptProfile(draft: PromptProfileDraft): void {
    this.writeLibrary(() => this.bridge.savePromptProfile(draft), (_id: string) => {
    });
  }

  private removeResource(work: () => Promise<boolean>): void {
    this.libraryBusy = true;
    this.clearError();
    work().then((removed: boolean) => {
      this.libraryBusy = false;
      if (!removed) {
        // Nothing was there. Saying so beats a spinner that stops with no
        // explanation, and it is the ordinary outcome when the same resource is
        // deleted from two screens.
        this.fail('already_removed');
      }
      this.loadLibrary();
      this.characters = [];
      this.searchCharacters(this.characterQuery);
    }).catch((error: Error) => {
      this.libraryBusy = false;
      this.fail('delete_failed', error.message);
    });
  }

  deleteCharacter(id: string): void {
    this.removeResource(() => this.bridge.deleteCharacter(id));
  }

  deletePersona(id: string): void {
    this.removeResource(() => this.bridge.deletePersona(id));
  }

  deleteWorldBook(id: string): void {
    this.removeResource(() => this.bridge.deleteWorldBook(id));
  }

  deletePromptProfile(id: string): void {
    this.removeResource(() => this.bridge.deletePromptProfile(id));
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

  /**
   * Switches to a model the endpoint listed, from anywhere — the top bar, not
   * only the settings form.
   *
   * A listed model is not a configured one, and that difference decides what
   * this has to do. `selectModel` only searches `this.models`, which holds what
   * has been saved, so calling it with a discovered id would quietly set the
   * current model to nothing. Worse, nothing would be sent: an endpoint carries
   * exactly one `selected_model` in Rust, so a model the user picked has to be
   * *saved* to become the model the next turn uses. That is why this is a save
   * and not an assignment, and why it reuses `saveProvider` rather than
   * reaching past it — the configured list is re-read afterwards, so the chat
   * and the settings form agree about what is current.
   *
   * The key comes from the credential store, because a model picker has no key
   * field. An empty key is not a refusal: `saveProvider` saves the description
   * and only writes a secret when one is given, so a user who never stored a key
   * still gets the model they chose and a `missing_credential` at the first
   * turn, which is a more honest place for that news than a picker that
   * silently does nothing.
   */
  async chooseModel(modelId: string): Promise<void> {
    const address: string = this.storedEndpoint === null ? '' : this.storedEndpoint.baseUrl.trim();
    if (address.length === 0) {
      // Nothing has been configured, so there is no endpoint to attach a model
      // to. Pointing at the settings form is more useful than a notice, because
      // the form is where an address gets typed.
      this.fail('provider_not_saved');
      return;
    }
    const previous: string = this.storedEndpoint?.selectedModel ?? '';
    if (previous === modelId) {
      this.selectModel(modelId);
      return;
    }
    // Caught rather than left to propagate. A pick comes from a sheet that has
    // already closed, so a rejected save with nobody listening would leave the
    // user with a top bar that did not change and no explanation anywhere. A
    // failed switch is the one place a notice is the whole answer.
    let saved: boolean = false;
    try {
      saved = await this.saveProvider({
        id: this.storedEndpoint?.id ?? 'endpoint-default',
        name: this.storedEndpoint?.label ?? address,
        baseUrl: address,
        selectedModel: modelId
      } as ProviderDraft, this.credentials.loadSecret(CREDENTIAL_NAME));
      if (saved) {
        // The runtime now names this model, so the list it reports contains it
        // and `selectModel` can find it. Re-reading the stored endpoint first
        // also picks up a save the bridge accepted but the local copy predates.
        this.storedEndpoint = await this.bridge.storedEndpoint();
      }
    } catch (reason) {
      this.fail('provider_not_saved', `${reason}`);
      return;
    }
    // Only now, and only if the runtime took it. Moving the highlight before
    // the save was the bug this guards: the top bar would name a model the
    // endpoint was still not using, so the next turn ran on something else and
    // the screen had already told the user otherwise. A failed switch leaves
    // the old model named and shows why.
    if (saved) {
      this.selectModel(modelId);
    }
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
      // The bindings are read from the snapshot rather than kept locally, so the
      // binding screen always shows what is stored. Optimistically applying a
      // checkbox would leave the two disagreeing whenever a write was refused.
      this.conversationParticipants = snapshot.participants;
      this.conversationPersonaId = snapshot.personaId ?? '';
      this.conversationWorldbookIds = snapshot.worldbookIds;
      this.conversationPromptProfileId = snapshot.promptProfileId ?? '';
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
        this.settleTurn();
        break;
      case TurnEventKind.Failed:
        this.assistantOpen = false;
        this.generationState = GenerationState.Failed;
        this.fail(event.message ?? '', event.text ?? '');
        // The runtime recorded which stage the turn stopped at. Reading the log
        // here is what makes that answer reachable: without it, "the turn
        // failed" is all a screen can say, and a failure during negotiation is
        // fixed in settings while a failure mid-request is not.
        this.loadDiagnostics();
        this.settleTurn();
        break;
      case TurnEventKind.Cancelled:
        this.assistantOpen = false;
        this.generationState = GenerationState.Cancelled;
        this.settleTurn();
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
