//! Stateful conversation runtime behind the C ABI.
//!
//! The runtime owns the provider-neutral data a frontend needs: sessions,
//! characters, models, context sources and conversation state, plus the turn
//! loop built on the shared agent runtime. Platform code receives only
//! normalized turn events from `events`, never provider wire formats or tool
//! internals.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sujiu_ai::{
    register_standard_context_tools, AgentConfig, AgentOutcome, AgentRuntime, AgentStop,
    CancelToken, ContextStore, ContextStoreError, InMemoryContextStore, OpenAiCompatConfig,
    OpenAiCompatProvider, OpenAiResponsesProvider, ProviderContinuation, ResponsesConfig,
    ToolRegistry,
};
use sujiu_core::{
    apply_reasoning_override, Character, ChatMessage, ChatRole, CompactionInput, ContextKind,
    ContextRecord, ContextSource, EndpointCapabilities, EndpointConfig, ModelListing,
    PromptCompiler, Protocol, ProviderIdentity, Session, Transcript, Turn,
    DEFAULT_APP_SYSTEM_PROMPT,
};

use sujiu_ai::diagnostics::{fields, DiagnosticEntry, DiagnosticKind, DiagnosticLog, LOG_KEY};
use sujiu_ai::CachedListing;

use crate::events::{TurnEventKind, TurnEventReporter, TurnEventSink};
use crate::storage::{AppStorage, FileStorage, MemoryStorage};

/// Upper bound on the conversation state a single call returns.
///
/// This bounds what crosses the FFI boundary, not what the session stores. A
/// transcript is never trimmed to satisfy it: dropping old turns here would
/// change the request prefix and invalidate a provider prompt cache, and the
/// full transcript is still sent to the model.
const MAX_TRANSCRIPT_MESSAGES: usize = 200;

/// Provider kinds this runtime can actually drive.
///
/// A frontend must only offer these. Accepting a kind it cannot serve would let
/// a user configure a provider and then fail every turn.

/// Sampling defaults for a turn, overridable per provider through `extra`.
const DEFAULT_MAX_TOKENS: u32 = 1024;
const DEFAULT_TEMPERATURE: f32 = 0.8;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub character_id: Option<String>,
    /// Resolved character name, so a list does not need a second lookup.
    pub character_name: String,
    /// First line of the last message, so a list can render without loading
    /// the whole conversation.
    pub preview: String,
    pub title: String,
    pub updated_at_ms: i64,
    pub message_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSummary {
    pub id: String,
    pub name: String,
    /// The character's own description, passed through as authored.
    ///
    /// The runtime does not shorten it into a teaser. Choosing a sentence, a
    /// length or an ellipsis is a presentation decision, and a truncation rule
    /// in the runtime would also be the wrong place to encode what counts as a
    /// sentence in a given language.
    pub description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub id: String,
    pub name: String,
    pub endpoint_id: String,
    /// A word to show a person, derived from the address. It says who answers
    /// and nothing else: no decision anywhere reads it, because a display label
    /// that changed how a conversation was spoken would be the same mistake
    /// wearing a different hat.
    pub endpoint_label: String,
    pub configured: bool,
}

/// What discovery found, and what the user may still do about it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDiscovery {
    /// One of `unknown`, `available`, `unavailable`, `permission_denied`,
    /// `rate_limited`, `unreachable`.
    pub listing: String,
    /// A sentence for the settings screen explaining this outcome.
    pub note: String,
    /// Always true. Discovery never takes the manual choice away.
    pub manual_entry_allowed: bool,
    pub models: Vec<ModelSummary>,
}

impl ModelDiscovery {
    fn without_endpoint(listing: ModelListing, note: &str) -> Self {
        Self {
            listing: listing_label(listing),
            note: note.to_string(),
            manual_entry_allowed: true,
            models: Vec::new(),
        }
    }
}

/// Everything an unsaved endpoint turned out to be.
///
/// The model list and the protocol are two independent questions, and they
/// are reported as two independent answers: a gateway can list nothing and
/// still speak Responses, and it can list fifty models and speak only the
/// oldest chat protocol. One of them failing says nothing about the other.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointExploration {
    /// The protocol negotiation settled on, or empty when it settled on
    /// nothing and `reason` says why.
    pub protocol: String,
    /// The protocols this endpoint was confirmed to answer for, best first.
    ///
    /// Negotiation stops at the first protocol that works, because probing the
    /// rest would spend the user's own requests to learn about transports this
    /// build is not going to use. So this is normally a single entry: it says
    /// what was established, not what might also be there.
    pub protocols: Vec<String>,
    /// How the protocol probe came out, as a code.
    ///
    /// The user is told two different things here and they must not be told one
    /// thing: a wrong key and an address that does not exist look identical if
    /// both are reduced to "probe failed", and the fix for one is the other
    /// field. `reason` is the sentence to read; this is the case to switch on.
    ///
    /// One of: `supported`, `no_usable_protocol`, `credentials_rejected`,
    /// `no_such_endpoint`, `rate_limited`, `server_unavailable`,
    /// `network_error`, `unreadable`, `undetermined`.
    pub status: String,
    /// Why negotiation concluded what it did, when it is not a clean answer.
    pub reason: Option<String>,
    pub models: ModelDiscovery,
}

/// What a model route returned, in the shape a screen reads.
///
/// The chosen model stays selectable even when the endpoint will not list
/// anything, otherwise discovery would hide the model the user already chose and
/// is using successfully — an endpoint that answers "I have no models" is
/// answering about its listing, not revoking a choice somebody already made.
/// An endpoint as the user typed it, before anything has been saved.
///
/// Deliberately carries no chosen model: this is the endpoint *before* the
/// configuration exists, and a model would change the capability key and with
/// it the cache the runtime is trying to find.
fn unsaved_endpoint(base_url: &str) -> EndpointConfig {
    EndpointConfig {
        id: "endpoint".to_string(),
        name: String::new(),
        base_url: base_url.to_string(),
        selected_model: None,
        credential_ref: None,
        overrides: serde_json::Map::new(),
    }
}

/// Turn what negotiation concluded into the shape a screen reads.
///
/// One function, used by both the live and the remembered answer, so a cached
/// exploration cannot drift from a fresh one. Two code paths building the same
/// summary is how a cached screen ends up disagreeing with a probed one about
/// which model is in use.
fn exploration_from(
    negotiation: &sujiu_ai::Negotiation,
    models: ModelDiscovery,
) -> EndpointExploration {
    EndpointExploration {
        protocol: negotiation.selected.map(protocol_label).unwrap_or_default(),
        protocols: negotiation
            .supported
            .iter()
            .map(|protocol| protocol_label(*protocol))
            .collect(),
        status: negotiation.status().to_string(),
        reason: negotiation.failure_explanation(),
        models,
    }
}

fn model_discovery(
    listing: sujiu_core::ModelListing,
    discovered: &[String],
    config: &EndpointConfig,
) -> ModelDiscovery {
    let mut models: Vec<ModelSummary> = discovered
        .iter()
        .map(|id| ModelSummary {
            id: id.clone(),
            name: id.clone(),
            endpoint_id: config.id.clone(),
            endpoint_label: config.display_label(),
            configured: config.selected_model.as_deref() == Some(id.as_str()),
        })
        .collect();

    if models.is_empty() {
        if let Some(chosen) = config
            .selected_model
            .as_ref()
            .filter(|model| !model.trim().is_empty())
        {
            models.push(ModelSummary {
                id: chosen.clone(),
                name: chosen.clone(),
                endpoint_id: config.id.clone(),
                endpoint_label: config.display_label(),
                configured: true,
            });
        }
    }

    ModelDiscovery {
        listing: listing_label(listing),
        note: listing_note(listing, config),
        manual_entry_allowed: true,
        models,
    }
}

fn listing_label(listing: ModelListing) -> String {
    match listing {
        ModelListing::Unknown => "unknown",
        ModelListing::Available => "available",
        ModelListing::Unavailable => "unavailable",
        ModelListing::PermissionDenied => "permission_denied",
        ModelListing::RateLimited => "rate_limited",
        ModelListing::Unreachable => "unreachable",
    }
    .to_string()
}

/// Say what a listing outcome does and does not mean.
///
/// The distinction matters because these look alarming and are not: an endpoint
/// without a model list is usually a gateway, and a key without listing
/// permission is usually scoped to chat. Both can still hold a conversation.
fn listing_note(listing: ModelListing, config: &EndpointConfig) -> String {
    let host = config.base_url.trim();
    match listing {
        ModelListing::Unknown => format!(
            "{host} did not answer a model listing. You can still type a model name."
        ),
        ModelListing::Available => format!("{host} listed its models."),
        ModelListing::Unavailable => format!(
            "{host} has no model listing endpoint. That is common for a gateway and does not mean it cannot chat, so you can still type a model name."
        ),
        ModelListing::PermissionDenied => format!(
            "The key for {host} is not allowed to list models. It may still be allowed to chat, so you can still type a model name."
        ),
        ModelListing::RateLimited => format!(
            "{host} is rate limiting right now. You can still type a model name."
        ),
        ModelListing::Unreachable => format!(
            "{host} could not be reached to list models. You can still type a model name."
        ),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceSummary {
    pub id: String,
    pub kind: ContextKind,
    pub name: String,
    pub description: String,
    pub record_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallSummary {
    pub id: String,
    pub name: String,
    pub title: String,
    /// `completed`, `failed`, `interrupted` or `cancelled`.
    ///
    /// A call that never produced a result still appears here, because dropping
    /// it would hide a step the model was actually shown.
    pub status: String,
    pub is_error: bool,
    pub result_text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: String,
    pub role: ChatRole,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSnapshot {
    pub session_id: String,
    pub character: Option<CharacterSummary>,
    pub messages: Vec<MessageSummary>,
}

/// Input for one turn. The api key is supplied per call by the platform secret
/// store and is never persisted by the runtime.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendTurnRequest {
    pub session_id: String,
    pub user_text: String,
    /// Provider to use for this turn, overriding the stored configuration.
    #[serde(default)]
    pub provider: Option<EndpointConfig>,
    /// Credential for this turn. Supplied by the platform secret store per
    /// call and never persisted by the runtime.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug)]
pub enum TurnError {
    SessionNotFound(String),
    NoProviderConfigured,
    NoUsableProtocol(String),
    MissingCredential,
    ContextStore(ContextStoreError),
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionNotFound(id) => write!(formatter, "session_not_found: {id}"),
            Self::NoProviderConfigured => {
                write!(formatter, "no_provider_configured")
            }
            Self::NoUsableProtocol(reason) => {
                write!(formatter, "no_usable_protocol: {reason}")
            }
            Self::MissingCredential => write!(formatter, "missing_credential"),
            Self::ContextStore(error) => write!(formatter, "{error}"),
        }
    }
}

struct Catalog {
    characters: Vec<Character>,
    sessions: Vec<Session>,
}

struct Inner {
    catalog: Catalog,
    store: Arc<InMemoryContextStore>,
    tools: Arc<ToolRegistry>,
    endpoint: Option<EndpointConfig>,
    storage_state: StorageState,
}

/// Whether the attached document may still be written to.
///
/// Skipping one write is not the same as protecting a file. The first version
/// of this only refused to write back on the call that noticed the problem,
/// which left the runtime happily overwriting the document on the next
/// configure, turn or compaction — so the refusal has to be a state the
/// runtime stays in, not a decision it makes once.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StorageState {
    Writable,
    /// A document is there that this build cannot read. Writing would destroy
    /// the only copy, so the runtime keeps working in memory and says so.
    Protected {
        reason: String,
    },
}

impl StorageState {
    fn reason(&self) -> Option<&str> {
        match self {
            StorageState::Writable => None,
            StorageState::Protected { reason } => Some(reason),
        }
    }
}

/// Seed data for a fresh runtime.
///
/// Used when no document has been persisted yet, so a first launch has a small
/// deterministic catalog of real domain values instead of inventing data per
/// call. Once a document exists the seed is ignored.
#[derive(Default)]
pub struct Seed {
    pub characters: Vec<Character>,
    pub sessions: Vec<Session>,
    pub sources: Vec<ContextSource>,
    pub records: Vec<ContextRecord>,
}

/// Everything a launch needs, as one document.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    /// Bumped when the document shape changes incompatibly.
    version: u32,
    characters: Vec<Character>,
    sessions: Vec<Session>,
    sources: Vec<ContextSource>,
    records: Vec<ContextRecord>,
    endpoint: Option<EndpointConfig>,
}

/// A session as version 1 stored it: a flat list of text messages.
///
/// Reading one of these is a migration, not a normal load. The old shape had
/// no place to record a tool call, so a migration can restore the turn/step
/// structure but never the steps that were already lost.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySession {
    id: String,
    #[serde(default)]
    character_id: Option<String>,
    #[serde(default)]
    messages: Vec<ChatMessage>,
    #[serde(default)]
    metadata: serde_json::Map<String, Value>,
}

impl LegacySession {
    fn into_session(self) -> Session {
        let mut transcript = Transcript::default();

        for message in self.messages {
            let at_ms = message.metadata.get("atMs").and_then(Value::as_i64);

            match message.role {
                // A user message opens a turn. Reusing the message id as the
                // turn id keeps the ids a platform already stored recognizable.
                ChatRole::User => transcript.push(Turn {
                    id: message.id,
                    user: message.content,
                    steps: Vec::new(),
                    state: sujiu_core::TurnState::Completed,
                    created_at_ms: at_ms,
                }),
                ChatRole::Assistant => {
                    let step = sujiu_core::AssistantStep::text_only(message.content);
                    match transcript.turns.last_mut() {
                        Some(turn) => {
                            turn.steps.push(step);
                            turn.created_at_ms = turn.created_at_ms.or(at_ms);
                        }
                        // An assistant message with no turn before it is kept,
                        // so a migration never silently drops history.
                        None => {
                            let mut turn = Turn::new(message.id, String::new());
                            turn.created_at_ms = at_ms;
                            turn.steps.push(step);
                            transcript.push(turn);
                        }
                    }
                }
                ChatRole::System | ChatRole::Developer => {}
            }
        }

        Session {
            id: self.id,
            character_id: self.character_id,
            transcript,
            metadata: self.metadata,
        }
    }
}

/// The endpoint as version 2 of the document wrote it.
///
/// It had a vendor `kind` and a `model`. Both are read here and dropped on the
/// way in: the kind never decided anything worth keeping, and the model becomes
/// the selected one. Reading it through `EndpointConfig` instead would fail,
/// because a v2 file is exactly the kind of document this build has to keep
/// opening without losing the user's sessions.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyEndpointConfig {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    credential_ref: Option<String>,
    #[serde(default)]
    extra: serde_json::Map<String, Value>,
}

impl LegacyEndpointConfig {
    fn into_endpoint(self) -> EndpointConfig {
        EndpointConfig {
            id: self.id,
            name: self.name,
            base_url: self.base_url,
            selected_model: (!self.model.trim().is_empty()).then_some(self.model),
            credential_ref: self.credential_ref,
            overrides: self.extra,
        }
    }
}

/// Version 1 of the document, read only to migrate it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySnapshot {
    #[serde(default)]
    characters: Vec<Character>,
    #[serde(default)]
    sessions: Vec<LegacySession>,
    #[serde(default)]
    sources: Vec<ContextSource>,
    #[serde(default)]
    records: Vec<ContextRecord>,
    #[serde(default)]
    provider_config: Option<LegacyEndpointConfig>,
}

/// Version 2 of the document.
///
/// Only the endpoint moved: a version 2 session was already a real transcript,
/// with the tool calls and results the older flat shape had nowhere to put. So
/// its sessions are read as they are. Running them through the version 1
/// migration instead would silently drop every turn, and the unreadable
/// continuation event inside one of those turns would be dropped with it,
/// which is how a store can lose a conversation without ever saying so.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version2Snapshot {
    #[serde(default)]
    characters: Vec<Character>,
    #[serde(default)]
    sessions: Vec<Session>,
    #[serde(default)]
    sources: Vec<ContextSource>,
    #[serde(default)]
    records: Vec<ContextRecord>,
    #[serde(default)]
    provider_config: Option<LegacyEndpointConfig>,
}

impl Version2Snapshot {
    fn into_snapshot(self) -> Snapshot {
        Snapshot {
            version: SNAPSHOT_VERSION,
            characters: self.characters,
            sessions: self.sessions,
            sources: self.sources,
            records: self.records,
            endpoint: self
                .provider_config
                .map(LegacyEndpointConfig::into_endpoint),
        }
    }
}

/// Read a persisted document, migrating an older shape when necessary.
fn parse_snapshot(document: &str) -> Option<Snapshot> {
    let value: Value = serde_json::from_str(document).ok()?;
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(1);

    // A version 2 document wrote its endpoint under `providerConfig`, and a
    // version 3 one writes `endpoint`. Reading a v2 file as v3 would find no
    // endpoint at all and quietly look like a fresh install.
    if version < 3 {
        if version < 2 {
            let legacy: LegacySnapshot = serde_json::from_value(value).ok()?;
            return Some(Snapshot {
                version: SNAPSHOT_VERSION,
                characters: legacy.characters,
                sessions: legacy
                    .sessions
                    .into_iter()
                    .map(LegacySession::into_session)
                    .collect(),
                sources: legacy.sources,
                records: legacy.records,
                endpoint: legacy
                    .provider_config
                    .map(LegacyEndpointConfig::into_endpoint),
            });
        }

        let version2: Version2Snapshot = serde_json::from_value(value).ok()?;
        return Some(version2.into_snapshot());
    }

    serde_json::from_value(value).ok()
}

/// A new session id that cannot collide with one already in the catalog.
fn next_session_id(sessions: &[Session]) -> String {
    let mut number = sessions.len() + 1;
    loop {
        let id = format!("session-{number}");
        if !sessions.iter().any(|session| session.id == id) {
            return id;
        }
        number += 1;
    }
}

/// Document name the runtime keeps its state in.
/// What the endpoint cache is filed under.
///
/// Separate from the conversation document, and for the same reason the log
/// is: a cache this build cannot read is a cache that starts empty, and that
/// must never be a reason to protect or rewrite a conversation.
const CAPABILITIES_KEY: &str = "sujiu-capabilities.json";
const SNAPSHOT_FILE: &str = "sujiu-runtime.json";
const SNAPSHOT_VERSION: u32 = 3;

/// The stateful runtime exported across the FFI boundary.
pub struct SujiuRuntime {
    inner: Mutex<Inner>,
    cancel: Mutex<Option<CancelToken>>,
    /// What each configured endpoint was found to speak.
    ///
    /// A probe is a network round trip and some endpoints rate limit, so the
    /// answer is remembered per endpoint. It is only ever a memory: a wrong
    /// answer costs one extra probe after a restart, and re-probing is always
    /// possible.
    capabilities: sujiu_ai::CapabilityCache,
    /// What was asked of the endpoint and what came back.
    ///
    /// Shared rather than owned by any one call because the two activities that
    /// write to it — discovery and a conversation — are the two a user needs to
    /// tell apart, and they happen in different places. Persisted alongside the
    /// snapshot, because the question this answers ("why did that not work?") is
    /// usually asked after the process that failed is gone.
    diagnostics: Arc<DiagnosticLog>,
    /// Where the snapshot lives. A platform attaches a directory; a runtime
    /// without one forgets everything when it exits.
    storage: Mutex<Arc<dyn AppStorage>>,
    pub tokio: tokio::runtime::Runtime,
}

impl SujiuRuntime {
    /// A runtime that lives only as long as the process. Used by tests and by
    /// any host with no platform storage.
    pub fn new(seed: Seed) -> std::io::Result<Self> {
        Self::assemble(seed, Arc::new(MemoryStorage::new()), false)
    }

    /// A runtime that keeps its state in `storage`, restoring a previous launch
    /// when one is there and writing the seed when it is not.
    pub fn new_persistent(seed: Seed, storage: Arc<dyn AppStorage>) -> std::io::Result<Self> {
        Self::assemble(seed, storage, true)
    }

    fn assemble(seed: Seed, storage: Arc<dyn AppStorage>, restore: bool) -> std::io::Result<Self> {
        let document = restore.then(|| storage.load(SNAPSHOT_FILE)).flatten();
        let snapshot = document.as_deref().and_then(parse_snapshot);

        // A document that is there but unreadable is not a fresh install. The
        // seed is what a first launch looks like, and writing it over a file we
        // failed to understand would trade a recoverable file for an empty one.
        let storage_state = match document.as_deref() {
            Some(unreadable) if snapshot.is_none() => StorageState::Protected {
                reason: format!(
                    "the stored document could not be read by this version ({unreadable})"
                ),
            },
            _ => StorageState::Writable,
        };

        let sources = snapshot
            .as_ref()
            .map(|snapshot| snapshot.sources.clone())
            .unwrap_or(seed.sources);
        let records = snapshot
            .as_ref()
            .map(|snapshot| snapshot.records.clone())
            .unwrap_or(seed.records);

        let store = Arc::new(InMemoryContextStore::new());
        for source in &sources {
            store.add_source(source.clone());
        }
        for record in &records {
            store.add_record(record.clone());
        }

        let mut tools = ToolRegistry::new();
        register_standard_context_tools(&mut tools, store.clone() as Arc<dyn ContextStore>);

        let tokio = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        let catalog = match &snapshot {
            Some(snapshot) => Catalog {
                characters: snapshot.characters.clone(),
                sessions: snapshot.sessions.clone(),
            },
            None => Catalog {
                characters: seed.characters,
                sessions: seed.sessions,
            },
        };

        let endpoint = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.endpoint.clone());

        let runtime = Self {
            inner: Mutex::new(Inner {
                catalog,
                store,
                tools: Arc::new(tools),
                endpoint,
                storage_state,
            }),
            cancel: Mutex::new(None),
            capabilities: sujiu_ai::CapabilityCache::new(),
            diagnostics: Arc::new(DiagnosticLog::default()),
            storage: Mutex::new(storage),
            tokio,
        };

        // A log is restored separately from the conversation document, and
        // deliberately so: a diagnostic log this build cannot read is still
        // worth keeping and must never be a reason to protect the store.
        if let Some(saved) = runtime.storage.lock().unwrap().load(LOG_KEY) {
            if let Ok(entries) = serde_json::from_str::<Vec<DiagnosticEntry>>(&saved) {
                runtime.diagnostics.restore(entries);
            }
        }

        // The same reasoning applies to the endpoint cache, and here it is not
        // merely tidy: without it every launch re-asks every question, which is
        // the cost that made the answers worth remembering in the first place.
        match runtime.storage.lock().unwrap().load(CAPABILITIES_KEY) {
            Some(saved) => match serde_json::from_str::<sujiu_ai::CapabilityCache>(&saved) {
                Ok(previous) => {
                    let restored = previous.len();
                    let listings = previous.listing_count();
                    runtime.capabilities.restore(previous);
                    runtime.diagnostics.record(
                        DiagnosticKind::Discovery,
                        "discovery_cache_restored",
                        "the endpoint answers from the last launch were read back",
                        fields([
                            ("protocols", restored.to_string()),
                            ("listings", listings.to_string()),
                        ]),
                    );
                }
                Err(error) => {
                    runtime.diagnostics.record(
                        DiagnosticKind::Discovery,
                        "discovery_cache_unreadable",
                        "the remembered endpoint answers could not be read back",
                        fields([("why", error.to_string())]),
                    );
                }
            },
            None => runtime.diagnostics.note(
                DiagnosticKind::Discovery,
                "discovery_cache_absent",
                "no endpoint answers were remembered from a previous launch",
            ),
        }
        // A launch-time observation that only exists in memory is lost the first
        // time the process ends, and this is exactly the observation somebody
        // reads after "my settings did not come back".
        runtime.persist_diagnostics();

        if restore {
            // A first launch writes its seed, so the next launch is a restore
            // and not a reset. A protected document is left alone, and
            // `persist` is the thing that knows how to leave it alone.
            runtime.persist();
        }

        Ok(runtime)
    }

    /// Point the runtime at a directory a platform chose, restoring a document
    /// there if one exists. The directory is created if it is missing.
    ///
    /// A document that cannot be read is left exactly as it is. Overwriting it
    /// with an empty catalog would destroy the only copy of a session because
    /// one value in it was not understood, and a store that cannot be read yet
    /// is a store whose contents are still worth keeping.
    pub fn use_directory(&self, path: &str) -> Result<(), String> {
        let storage = FileStorage::new(path).map_err(|error| error.to_string())?;
        if let Some(document) = storage.load(SNAPSHOT_FILE) {
            match parse_snapshot(&document) {
                Some(snapshot) => {
                    let mut inner = self.inner.lock().unwrap();
                    inner.catalog = Catalog {
                        characters: snapshot.characters,
                        sessions: snapshot.sessions,
                    };
                    inner.endpoint = snapshot.endpoint;
                    inner.store.clear();
                    for source in snapshot.sources {
                        inner.store.add_source(source);
                    }
                    for record in snapshot.records {
                        inner.store.add_record(record);
                    }
                }
                // Keep what is there, now and later. A directory that has
                // content this build cannot read is not an empty directory,
                // and every later write would replace the only copy of it.
                None => {
                    self.inner.lock().unwrap().storage_state = StorageState::Protected {
                        reason: format!(
                            "the stored document in {path} could not be read by this version ({document})"
                        ),
                    };
                }
            }
        } else {
            self.inner.lock().unwrap().storage_state = StorageState::Writable;
        }
        // Everything the new storage holds has to be read back, not just the
        // conversation. The cache and the log were restored in `assemble`, from
        // whatever storage the runtime happened to be born with, and on a
        // platform the directory is only named after construction — so a
        // restore that does not happen here has silently read the wrong place.
        // That is not hypothetical: it is why a HarmonyOS build remembered the
        // endpoint's models on the first launch after a probe and then forgot
        // them on the next one, while the conversation came back perfectly.
        let diagnostics = match storage.load(LOG_KEY) {
            Some(saved) => serde_json::from_str::<Vec<DiagnosticEntry>>(&saved).ok(),
            None => None,
        };
        let capabilities = match storage.load(CAPABILITIES_KEY) {
            Some(saved) => serde_json::from_str::<sujiu_ai::CapabilityCache>(&saved).ok(),
            None => None,
        };
        if let Some(entries) = diagnostics {
            self.diagnostics.restore(entries);
        }
        match capabilities {
            Some(previous) => {
                let listings = previous.listing_count();
                self.capabilities.restore(previous);
                self.diagnostics.record(
                    DiagnosticKind::Discovery,
                    "discovery_cache_restored",
                    "the endpoint answers from this directory were read back",
                    fields([("listings", listings.to_string())]),
                );
            }
            None => self.diagnostics.note(
                DiagnosticKind::Discovery,
                "discovery_cache_absent",
                "this directory holds no remembered endpoint answers",
            ),
        }

        *self.storage.lock().unwrap() = Arc::new(storage);
        self.persist();
        self.persist_diagnostics();
        Ok(())
    }

    /// Why the runtime will not write to its store, when it will not.
    ///
    /// A platform should surface this. A runtime that silently keeps its
    /// changes in memory looks exactly like one that saved them, and the user
    /// only finds out at the next launch.
    pub fn storage_protection(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .storage_state
            .reason()
            .map(str::to_owned)
    }

    /// Replace an unreadable document with the seed, on purpose.
    ///
    /// This is the user's call, not the runtime's: it is the only way out of
    /// the protected state, and it throws away whatever was in the file. It
    /// exists so a user who has decided the old document is not worth keeping
    /// is not locked out of the app forever.
    pub fn discard_protected_document(&self, seed: Seed) -> Result<(), TurnError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.storage_state.reason().is_none() {
            return Ok(());
        }

        inner.catalog = Catalog {
            characters: seed.characters,
            sessions: seed.sessions,
        };
        inner.storage_state = StorageState::Writable;
        drop(inner);

        self.persist();
        Ok(())
    }

    /// The directory the runtime persists into, or `None` while it is not
    /// attached to one.
    pub fn directory(&self) -> Option<String> {
        self.storage.lock().unwrap().location()
    }

    fn persist(&self) {
        if self.storage_protection().is_some() {
            return;
        }

        let snapshot = {
            let inner = self.inner.lock().unwrap();
            let (sources, records) = inner.store.snapshot();
            Snapshot {
                version: SNAPSHOT_VERSION,
                characters: inner.catalog.characters.clone(),
                sessions: inner.catalog.sessions.clone(),
                sources,
                records,
                endpoint: inner.endpoint.clone(),
            }
        };

        if let Ok(document) = serde_json::to_string(&snapshot) {
            self.storage.lock().unwrap().save(SNAPSHOT_FILE, &document);
        }
    }

    pub fn endpoint(&self) -> Option<EndpointConfig> {
        self.inner.lock().unwrap().endpoint.clone()
    }

    /// The log a settings screen shows and a user pastes into a bug report.
    ///
    /// `limit` is the number of most recent entries; zero means everything.
    pub fn diagnostics(&self, limit: usize) -> Vec<DiagnosticEntry> {
        self.diagnostics.recent(limit)
    }

    /// Only the discovery half, which is what someone configuring an endpoint
    /// wants to read. A turn's own log is noise while deciding a key.
    pub fn discovery_diagnostics(&self, limit: usize) -> Vec<DiagnosticEntry> {
        self.diagnostics
            .recent(limit)
            .into_iter()
            .filter(|entry| entry.kind == DiagnosticKind::Discovery)
            .collect()
    }

    /// The log as text, one entry per line.
    pub fn diagnostics_text(&self) -> String {
        self.diagnostics.render()
    }

    /// Empty the log, so a user can start a fresh investigation.
    pub fn clear_diagnostics(&self) {
        self.diagnostics.clear();
        self.persist_diagnostics();
    }

    /// Write the log out.
    ///
    /// Separate from `persist` on purpose. A diagnostic line is not
    /// conversation state: it must be saveable while the store is protected, and
    /// it must never be the reason a document is rewritten. A user who lost
    /// their sessions still needs the log that explains what the endpoint did.
    fn persist_diagnostics(&self) {
        let Ok(document) = serde_json::to_string(&self.diagnostics.entries()) else {
            return;
        };
        self.storage.lock().unwrap().save(LOG_KEY, &document);
    }

    /// File the endpoint cache, so the next launch starts with the answers this
    /// one paid for.
    ///
    /// Written separately from the conversation document for the reason the log
    /// is: the two have nothing to do with each other, and coupling them would
    /// let a cache that cannot be read put a user's conversations at risk.
    fn persist_capabilities(&self) {
        let Ok(document) = serde_json::to_string(&self.capabilities) else {
            return;
        };
        self.storage
            .lock()
            .unwrap()
            .save(CAPABILITIES_KEY, &document);
    }

    /// The wire formats this build speaks, in the order it prefers them.
    ///
    /// This replaces the old "provider kinds" list. A frontend used to be
    /// handed a list of vendors to put in a dropdown, which asked the user to
    /// decide something negotiation decides by asking the endpoint. Nothing
    /// needs choosing here; the list is only useful for showing what the
    /// runtime may end up using.
    pub fn supported_protocols() -> Vec<String> {
        Protocol::PRIORITY
            .iter()
            .map(|p| protocol_label(*p))
            .collect()
    }

    /// Store the endpoint a frontend configured.
    ///
    /// There is no kind to validate: an endpoint is whatever the user says it
    /// is, and the first turn asks it what it speaks. What is validated here
    /// is only that there is somewhere to send a request.
    pub fn set_endpoint(&self, config: Option<EndpointConfig>) -> Result<(), TurnError> {
        if let Some(config) = config.as_ref() {
            if config.base_url.trim().is_empty() {
                return Err(TurnError::NoProviderConfigured);
            }
        }

        // What was configured, and which model, before the address is dropped
        // from view. A log that only recorded requests would show what the
        // endpoint did and not what the user had set up to ask it.
        if let Some(config) = config.as_ref() {
            self.diagnostics.record(
                DiagnosticKind::Discovery,
                "endpoint_configured",
                "the endpoint was saved",
                fields([
                    ("endpoint", config.normalized_base_url()),
                    (
                        "model",
                        config
                            .selected_model
                            .clone()
                            .filter(|model| !model.trim().is_empty())
                            .unwrap_or_else(|| "(not chosen yet)".to_string()),
                    ),
                ]),
            );
            self.persist_diagnostics();
        }

        self.inner.lock().unwrap().endpoint = config;
        // A changed endpoint is a different question, so the old answer does
        // not carry over to it.
        self.forget_capabilities_about();
        self.persist();
        Ok(())
    }

    /// Drop everything negotiation concluded.
    ///
    /// Called when the provider config changes, because a cached answer about
    /// a gateway the user has just re-pointed is worse than no answer. The
    /// whole cache goes rather than one entry: the config id may have been
    /// re-pointed at a different endpoint under the same name, and the cache
    /// is cheap to refill.
    pub fn forget_capabilities_about(&self) {
        self.capabilities.clear();
        // Forgetfulness that outlives the process is still forgetfulness; without
        // this the next launch would restore the answer about the endpoint the
        // user has just re-pointed.
        self.persist_capabilities();
    }

    pub fn sessions(&self) -> Vec<SessionSummary> {
        let inner = self.inner.lock().unwrap();

        inner
            .catalog
            .sessions
            .iter()
            .map(|session| {
                let character = session.character_id.as_ref().and_then(|id| {
                    inner
                        .catalog
                        .characters
                        .iter()
                        .find(|character| &character.id == id)
                });

                // The folded conversation, because this is a list. The
                // transcript behind it keeps every step.
                let messages = session.ui_messages();

                SessionSummary {
                    // A projection of the last message, not a display string. The
                    // bound exists so a session list does not carry whole
                    // messages across the boundary; how a platform renders or
                    // further clips it is that platform's business.
                    preview: messages
                        .iter()
                        .rev()
                        .find(|message| !message.text.trim().is_empty())
                        .map(|message| projected_preview(&message.text))
                        .unwrap_or_default(),
                    character_name: character
                        .map(|character| character.name.clone())
                        .unwrap_or_default(),
                    id: session.id.clone(),
                    character_id: session.character_id.clone(),
                    title: session
                        .metadata
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        // An untitled session stays empty: what to call it is
                        // presentation copy, and the UI localizes the fallback.
                        .unwrap_or_else(|| match character {
                            Some(character) => character.name.clone(),
                            None => String::new(),
                        }),
                    updated_at_ms: session
                        .metadata
                        .get("updatedAtMs")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                    message_count: messages.len(),
                }
            })
            .collect()
    }

    pub fn characters(&self, query: &str) -> Vec<CharacterSummary> {
        let needle = query.trim().to_lowercase();
        let inner = self.inner.lock().unwrap();

        inner
            .catalog
            .characters
            .iter()
            .filter(|character| {
                needle.is_empty()
                    || character.name.to_lowercase().contains(&needle)
                    || character.description.to_lowercase().contains(&needle)
            })
            .map(|character| CharacterSummary {
                id: character.id.clone(),
                name: character.name.clone(),
                description: character.description.clone(),
            })
            .collect()
    }

    pub fn character(&self, id: &str) -> Option<Character> {
        self.inner
            .lock()
            .unwrap()
            .catalog
            .characters
            .iter()
            .find(|character| character.id == id)
            .cloned()
    }

    pub fn models(&self) -> Vec<ModelSummary> {
        let inner = self.inner.lock().unwrap();
        let Some(config) = inner.endpoint.clone() else {
            return Vec::new();
        };

        match config
            .selected_model
            .as_ref()
            .filter(|m| !m.trim().is_empty())
        {
            Some(chosen) => vec![ModelSummary {
                id: chosen.clone(),
                name: chosen.clone(),
                endpoint_id: config.id.clone(),
                // The label is the one place a hostname may say something, and
                // it says it only to a human.
                endpoint_label: config.display_label(),

                configured: true,
            }],
            // A known endpoint that has not chosen a model yet is still an
            // endpoint, and reporting nothing here would hide it.
            None => Vec::new(),
        }
    }

    /// Ask the endpoint which models it serves, without deciding anything.
    ///
    /// Discovery is a convenience for the settings screen, never a step in the
    /// conversation: the transcript protocol comes from negotiation, so a
    /// model list that comes back empty changes nothing about how a turn runs.
    ///
    /// Every outcome keeps `manual_entry_allowed` true. A gateway with no
    /// listing endpoint is still perfectly able to chat, and a key without
    /// listing permission is still a key that can chat; refusing to let the
    /// user type a model would turn a cosmetic limitation into a dead end.
    pub async fn discover_models(&self, api_key: Option<&str>) -> ModelDiscovery {
        let inner = self.inner.lock().unwrap();
        let Some(config) = inner.endpoint.clone() else {
            return ModelDiscovery::without_endpoint(
                ModelListing::Unknown,
                "no endpoint configured yet",
            );
        };

        drop(inner);
        self.explore(&config, api_key).await
    }

    /// Explore an endpoint from nothing but a base URL and a key.
    ///
    /// This is the whole configuration flow the review asked for: the user
    /// supplies an address and a credential, and Sujiu works out what is
    /// there. No endpoint is stored, no model needs to be known, and no vendor
    /// is named. The only thing the user is afterwards asked to decide is the
    /// model, because that is the one choice a machine genuinely cannot make
    /// for them.
    /// Explore an address and a key the user just typed.
    ///
    /// `refresh` asks the endpoint again instead of answering from what it said
    /// before; see `explore_endpoint` for why that is a per-call choice rather
    /// than a policy the runtime decides on its own.
    pub async fn discover_endpoint(
        &self,
        base_url: &str,
        api_key: &str,
        refresh: bool,
    ) -> EndpointExploration {
        let config = unsaved_endpoint(base_url);

        self.diagnostics.record(
            DiagnosticKind::Discovery,
            "discovery_request",
            "the user asked what this endpoint can do",
            fields([
                ("endpoint", config.normalized_base_url()),
                (
                    "key",
                    // Masked, and only so a user can tell whether the field they
                    // typed is the one being used. A blank one is the single
                    // most common reason a probe answers nothing.
                    if api_key.trim().is_empty() {
                        "(not given)".to_string()
                    } else {
                        sujiu_ai::mask_secret(api_key)
                    },
                ),
            ]),
        );

        let exploration = self.explore_endpoint(&config, Some(api_key), refresh).await;
        self.persist_diagnostics();
        exploration
    }

    /// What this endpoint told us before, without asking it again.
    ///
    /// Nothing here touches the network, which is the entire point. A caller
    /// that wants the current truth calls `discover_endpoint`; this is for the
    /// screen that just wants to show what is already known, and it is what a
    /// launch reads so the model list is there before anybody asks. Returning
    /// nothing is an ordinary answer — it means nobody has asked this endpoint
    /// yet — and not a failure.
    pub fn remembered_endpoint(&self, base_url: &str) -> Option<EndpointExploration> {
        let config = unsaved_endpoint(base_url);
        let key = config.capability_key();
        let listing = self.capabilities.listing(&key)?;
        let negotiation = self
            .capabilities
            .get(&key)
            .unwrap_or_else(sujiu_ai::Negotiation::not_established);
        Some(exploration_from(
            &negotiation,
            model_discovery(listing.listing, &listing.models, &config),
        ))
    }

    /// Explore an endpoint that has **not** been saved yet.
    ///
    /// This is the order the user actually works in: type a base URL and a
    /// key, and find out what is there, before committing to anything. The
    /// earlier flow made the endpoint un-saveable until a model was already
    /// known and then asked the very same questions through the saved
    /// endpoint, so discovery ran last and could never inform the decision it
    /// was supposed to inform.
    ///
    /// Nothing here is written to disk. A settings screen explores, the user
    /// picks or types a model, and only then is an endpoint stored.
    /// What an endpoint can do: which protocols it answers, and which models it
    /// lists.
    ///
    /// `refresh` is the escape hatch, and it exists because a cache that cannot
    /// be refilled is a bug waiting to happen — a user who has just corrected a
    /// key needs a way to say "ask again" rather than being shown yesterday's
    /// answer to yesterday's question. Everything else is answered from what
    /// this endpoint already said, because every open of this screen used to
    /// cost several round trips, and against a free tier it cost the user a
    /// wait and the endpoint a rate limit.
    pub async fn explore_endpoint(
        &self,
        config: &EndpointConfig,
        api_key: Option<&str>,
        refresh: bool,
    ) -> EndpointExploration {
        let key = config.capability_key();

        if !refresh {
            if let Some(listing) = self.capabilities.listing(&key) {
                // The listing is what makes this worth answering from memory. A
                // protocol answer is deliberately not cached when the walk could
                // not conclude one, so "only the listing is remembered" is the
                // normal case for an endpoint that routes by model. Pairing that
                // listing with an invented protocol answer would be reporting
                // the absence of a probe as the result of one.
                let negotiation = self
                    .capabilities
                    .get(&key)
                    .unwrap_or_else(sujiu_ai::Negotiation::not_established);

                self.diagnostics.record(
                    DiagnosticKind::Discovery,
                    "discovery_cached",
                    "answered from what this endpoint said before",
                    fields([
                        ("endpoint", config.normalized_base_url()),
                        (
                            "protocol",
                            negotiation
                                .selected
                                .map(protocol_label)
                                .unwrap_or_else(|| "none".to_string()),
                        ),
                        ("listing", listing_label(listing.listing)),
                        ("count", listing.models.len().to_string()),
                        (
                            "asked_ms_ago",
                            (sujiu_ai::diagnostics::now_ms() - listing.at_ms).to_string(),
                        ),
                    ]),
                );
                return exploration_from(
                    &negotiation,
                    model_discovery(listing.listing, &listing.models, config),
                );
            }
        }

        let models = self.explore(config, api_key).await;

        // The listing runs first so the probe has a real model to retry with. A
        // gateway that routes by model answers the placeholder with the same 404
        // it uses for a missing path, which proves nothing about the protocol;
        // asking again with a model this endpoint actually lists is the only way
        // to make the route answer at all.
        let listed: Vec<String> = models.models.iter().map(|model| model.id.clone()).collect();

        let negotiation = sujiu_ai::negotiate_asking(
            None,
            config,
            api_key.unwrap_or_default(),
            Some(&self.capabilities),
            Some(self.diagnostics.as_ref()),
            &listed,
        )
        .await;

        self.persist_capabilities();

        exploration_from(&negotiation, models)
    }

    async fn explore(&self, config: &EndpointConfig, api_key: Option<&str>) -> ModelDiscovery {
        let (listing, discovered) = sujiu_ai::list_models(
            None,
            config,
            api_key.unwrap_or_default(),
            Some(self.diagnostics.as_ref()),
        )
        .await;

        // Not every answer is worth keeping, and this is the same rule the
        // probe already follows: a bad key, a rate limit and an unreachable host
        // are statements about this moment, not about the endpoint. Remembering
        // one would make a transient failure permanent — a user who fixes their
        // key would keep being told the endpoint has no models, with nothing on
        // screen suggesting why. "There is no model route here" and "the models
        // are these" are the only two answers that are facts about the service.
        if matches!(
            listing,
            sujiu_core::ModelListing::Available | sujiu_core::ModelListing::Unavailable
        ) {
            self.capabilities.remember_listing(
                config.capability_key(),
                CachedListing {
                    listing,
                    models: discovered.clone(),
                    at_ms: sujiu_ai::diagnostics::now_ms(),
                },
            );
            self.persist_capabilities();
        }

        model_discovery(listing, &discovered, config)
    }

    pub async fn context_sources(&self) -> Result<Vec<ContextSourceSummary>, ContextStoreError> {
        let store = self.inner.lock().unwrap().store.clone();
        let sources = store.list_sources(&[]).await?;

        Ok(sources
            .iter()
            .map(|source| ContextSourceSummary {
                id: source.id.clone(),
                kind: source.kind,
                name: source.name.clone(),
                description: source.description.clone(),
                record_count: source.record_count,
            })
            .collect())
    }

    pub fn conversation_state(&self, session_id: &str) -> Option<ConversationSnapshot> {
        let inner = self.inner.lock().unwrap();
        let session = inner
            .catalog
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;

        let character = session
            .character_id
            .as_ref()
            .and_then(|id| {
                inner
                    .catalog
                    .characters
                    .iter()
                    .find(|character| &character.id == id)
            })
            .map(|character| CharacterSummary {
                id: character.id.clone(),
                name: character.name.clone(),
                description: character.description.clone(),
            });

        // The UI projection: one user message and one assistant message per
        // turn, with the turn's tool calls folded into the assistant message.
        // This is a view of the transcript, not a replacement for it.
        let messages = session
            .ui_messages()
            .into_iter()
            .rev()
            .take(MAX_TRANSCRIPT_MESSAGES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|message| MessageSummary {
                id: message.id,
                role: message.role,
                text: message.text,
                tool_calls: message
                    .tool_calls
                    .into_iter()
                    .map(|call| ToolCallSummary {
                        id: call.id,
                        name: call.name,
                        title: call.title,
                        status: tool_call_status(call.state).to_string(),
                        is_error: call.is_error,
                        result_text: call.result_text,
                    })
                    .collect(),
            })
            .collect();

        Some(ConversationSnapshot {
            session_id: session.id.clone(),
            character,
            messages,
        })
    }

    /// Create a session and return its id.
    pub fn create_session(&self, character_id: Option<&str>) -> String {
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = next_session_id(&inner.catalog.sessions);

            inner.catalog.sessions.insert(
                0,
                Session {
                    id: id.clone(),
                    character_id: character_id.map(str::to_owned),
                    transcript: Transcript::default(),
                    metadata: serde_json::Map::new(),
                },
            );

            id
        };

        self.persist();
        id
    }

    /// Replace the oldest turns of a session with a summary, keeping the last
    /// `keep_recent` turns verbatim.
    ///
    /// The summary text is supplied by the caller because producing it needs a
    /// model turn of its own. The runtime's job is to guarantee that compaction
    /// cannot break the tool protocol: it cuts on a turn boundary, keeps the
    /// compacted turns retrievable, and reports whether anything changed.
    ///
    /// `summary` must be **cumulative**: it has to stand in for every archived
    /// turn, not just the ones this call moves. Get the material to write it from
    /// [`SujiuRuntime::compaction_input`] — that is what carries the previous
    /// summary and the already-archived turns, so a caller can actually build one
    /// instead of being asked to remember what it cannot see.
    pub fn compact_session(
        &self,
        session_id: &str,
        keep_recent: usize,
        summary: &str,
    ) -> Result<bool, TurnError> {
        let compacted = {
            let mut inner = self.inner.lock().unwrap();
            let session = inner
                .catalog
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .ok_or_else(|| TurnError::SessionNotFound("unknown session".to_string()))?;

            session
                .transcript
                .compact(keep_recent, |_, _| summary.to_owned())
        };

        if compacted {
            self.persist();
        }

        Ok(compacted)
    }

    /// What a caller has to summarize to compact this session, or `None` when
    /// nothing is old enough.
    ///
    /// This exists so compaction cannot quietly lose the older half of a
    /// conversation. A layer above that only ever saw the turns being archived
    /// now has no way to write a summary covering the rest, and would replace
    /// the existing summary with one that no longer mentions it.
    pub fn compaction_input(
        &self,
        session_id: &str,
        keep_recent: usize,
    ) -> Result<Option<CompactionInput>, TurnError> {
        let inner = self.inner.lock().unwrap();
        let session = inner
            .catalog
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| TurnError::SessionNotFound("unknown session".to_string()))?;

        Ok(session.transcript.compaction_input(keep_recent))
    }

    /// Run one agent turn, reporting normalized events to `reporter`.
    pub async fn send_turn(&self, request: SendTurnRequest, reporter: &mut dyn TurnEventReporter) {
        let cancel = CancelToken::new();
        *self.cancel.lock().unwrap() = Some(cancel.clone());

        let mut sink = TurnEventSink::new(reporter, cancel);
        sink.turn_started();

        match self.run_turn(&request, &mut sink).await {
            Ok(()) => {}
            Err(TurnFailure(error)) => sink.turn_failed(&error),
        }

        *self.cancel.lock().unwrap() = None;
    }

    async fn run_turn(
        &self,
        request: &SendTurnRequest,
        sink: &mut TurnEventSink<'_>,
    ) -> Result<(), TurnFailure> {
        let prepared = self
            .prepare(request)
            .map_err(|error| TurnFailure(error.to_string()))?;
        let messages = prepared.messages.clone();
        let continuation = prepared.continuation.clone();
        let provider = self.build_provider(prepared).await?;
        let tools = self.inner.lock().unwrap().tools.clone();

        let runtime = AgentRuntime::new(provider.as_ref(), tools.as_ref(), AgentConfig::default());
        // A turn that stops early is still a turn. Everything the model already
        // produced is committed, so the next request continues from the same
        // transcript instead of replaying a conversation the model has half
        // forgotten.
        let outcome = runtime
            .run_streaming(messages, continuation, &request.user_text, sink)
            .await;

        self.persist_turn(request, &outcome);

        match outcome.stop {
            AgentStop::Completed => {
                self.diagnostics.record(
                    DiagnosticKind::Chat,
                    "turn_completed",
                    "the turn answered",
                    fields([
                        ("steps", outcome.turn.steps.len().to_string()),
                        ("tool_calls", count_tool_calls(&outcome).to_string()),
                    ]),
                );
                self.persist_diagnostics();
                sink.turn_completed(&outcome.final_text);
                Ok(())
            }
            // The turn is committed, but it did not answer. A cancelled turn
            // is not an error the user needs to dismiss, and a turn that ran
            // out of rounds never will answer on its own.
            AgentStop::Cancelled => {
                self.diagnostics.note(
                    DiagnosticKind::Chat,
                    "turn_cancelled",
                    "the user stopped the turn",
                );
                self.persist_diagnostics();
                sink.turn_cancelled();
                Ok(())
            }
            AgentStop::MaxRounds(rounds) => {
                self.diagnostics.record(
                    DiagnosticKind::Chat,
                    "turn_failed",
                    "the model kept asking for tools until the round limit",
                    fields([
                        ("stage", "tool_rounds".to_string()),
                        ("rounds", rounds.to_string()),
                    ]),
                );
                self.persist_diagnostics();
                sink.turn_failed(&format!("max_tool_rounds_exceeded: {rounds}"));
                Ok(())
            }
            // The rounds that did finish stay in the transcript, so the next
            // turn continues from them. The reason is reported separately so
            // the UI can show what went wrong.
            AgentStop::Failed(error) => {
                // The stage is what makes this line worth keeping: "the request
                // failed" and "the fifth tool call failed" need completely
                // different fixes, and the message alone rarely says which.
                self.diagnostics.record(
                    DiagnosticKind::Chat,
                    "turn_failed",
                    "the request itself failed",
                    fields([("stage", "request".to_string()), ("detail", error.clone())]),
                );
                self.persist_diagnostics();
                sink.turn_failed(&error);
                Ok(())
            }
        }
    }

    /// Build the model messages, the reusable provider state, and the provider
    /// for one turn.
    fn prepare(&self, request: &SendTurnRequest) -> Result<PreparedTurn, TurnError> {
        let inner = self.inner.lock().unwrap();

        let session = inner
            .catalog
            .sessions
            .iter()
            .find(|session| session.id == request.session_id)
            .cloned()
            .ok_or_else(|| TurnError::SessionNotFound(request.session_id.clone()))?;

        // A per-turn provider is an override, not a replacement.
        //
        // Treating it as a replacement meant that a platform which re-sent a
        // partial config on every turn silently dropped everything the stored
        // one had — capabilities included. The override is therefore merged
        // onto the stored config, and only a field the override actually
        // carries wins.
        let config = match (inner.endpoint.clone(), request.provider.clone()) {
            (_, Some(override_)) if inner.endpoint.is_some() => {
                merge_endpoint(inner.endpoint.as_ref().unwrap(), &override_)
            }
            (_, Some(only)) => only,
            (Some(stored), None) => stored,
            (None, None) => return Err(TurnError::NoProviderConfigured),
        };

        // The base URL and a credential are what a request needs. A model is a
        // choice the user makes after discovery, and refusing a turn before
        // that would mean the endpoint could never be asked anything at all.
        if config.base_url.trim().is_empty() {
            return Err(TurnError::NoProviderConfigured);
        }

        let api_key = request
            .api_key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .ok_or(TurnError::MissingCredential)?;

        let character = session
            .character_id
            .as_ref()
            .and_then(|id| {
                inner
                    .catalog
                    .characters
                    .iter()
                    .find(|character| &character.id == id)
                    .cloned()
            })
            .unwrap_or_default();

        let plan = PromptCompiler::compile(
            Some(DEFAULT_APP_SYSTEM_PROMPT),
            &character,
            &session.transcript,
            &request.user_text,
        );

        // Only replay provider state that belongs to this exact endpoint and
        // model. After a switch this is `None`, and the adapter rebuilds the
        // request from the normalized transcript instead.
        // The protocol is not in the config any more: it is whatever
        // negotiation settles on, and until then an unknown protocol means this
        // identity matches nothing, so no stale provider state is replayed.
        let identity = ProviderIdentity {
            protocol: Protocol::default(),
            endpoint_id: config.id.clone(),
            base_url: config.base_url.clone(),
            model: config.selected_model.clone().unwrap_or_default(),
        };
        let continuation = session.transcript.continuation_for(&identity).cloned();

        Ok(PreparedTurn {
            messages: plan.model_messages(),
            continuation,
            config,
            identity,
            api_key,
        })
    }

    /// Ask the endpoint what it speaks, then build the adapter it can use.
    ///
    /// Negotiation is a network call, so it is cached per endpoint. The first
    /// turn of a session pays for it and later turns do not, and a probe that
    /// could not conclude is never cached, because a rate limit is not a fact
    /// about the endpoint.
    async fn build_provider(&self, prepared: PreparedTurn) -> Result<TurnProvider, TurnFailure> {
        let negotiation = sujiu_ai::negotiate_protocol(
            None,
            &prepared.config,
            &prepared.api_key,
            Some(&self.capabilities),
            Some(self.diagnostics.as_ref()),
            // A turn already has a chosen model, so the probe names it and
            // cannot be blinded by a gateway that routes on model. The retry
            // exists for the settings screen, where nothing has been picked yet.
            None,
        )
        .await;

        if negotiation.selected.is_none() {
            let reason = negotiation
                .reason
                .unwrap_or_else(|| "no usable protocol was found at that endpoint".to_string());
            self.diagnostics.record(
                DiagnosticKind::Chat,
                "turn_failed",
                "the turn stopped before any request was sent",
                fields([
                    ("stage", "negotiate".to_string()),
                    ("endpoint", prepared.config.normalized_base_url()),
                    ("reason", reason.clone()),
                ]),
            );
            self.persist_diagnostics();
            return Err(TurnFailure(reason));
        }

        // The selected protocol is always one of the supported ones, so this
        // cannot be None. It is not defaulted into existence: a fabricated
        // capability set is exactly the guess this negotiation replaced.
        let negotiated =
            EndpointCapabilities::negotiate(&negotiation.supported).ok_or_else(|| {
                TurnFailure("negotiation selected a protocol it did not report".to_string())
            })?;
        let capabilities =
            apply_reasoning_override(negotiated, prepared.config.forced_reasoning_replay());

        let message_count = prepared.messages.len();
        let config = prepared.config;
        let api_key = prepared.api_key;
        let identity = prepared.identity;

        // The protocol the request will actually use, named before the request
        // exists.
        //
        // This line is what makes the log worth keeping: "why did it answer the
        // way it did" is very often "which wire format was it even speaking",
        // and a log that only recorded the answer leaves that unanswerable.
        self.diagnostics.record(
            DiagnosticKind::Chat,
            "chat_request",
            format!("one turn over {}", protocol_label(capabilities.protocol)),
            fields([
                ("endpoint", config.normalized_base_url()),
                ("protocol", protocol_label(capabilities.protocol)),
                (
                    "supported",
                    negotiation
                        .supported
                        .iter()
                        .map(|protocol| protocol_label(*protocol))
                        .collect::<Vec<_>>()
                        .join(","),
                ),
                ("from_cache", negotiation.from_cache.to_string()),
                (
                    "model",
                    config
                        .selected_model
                        .clone()
                        .unwrap_or_else(|| "(not chosen yet)".to_string()),
                ),
                ("messages", message_count.to_string()),
                (
                    "replays_reasoning",
                    capabilities.replays_assistant_reasoning.to_string(),
                ),
            ]),
        );
        self.persist_diagnostics();

        // The adapter is chosen by what the endpoint said it speaks, in the
        // order the protocol priority fixes. The user is not asked, and neither
        // is a name: an endpoint that speaks Responses gets Responses, because
        // that transport carries continuation, reasoning and tool state in its
        // own shape instead of in fields bolted onto a chat message.
        if capabilities.protocol == Protocol::OpenAiResponses {
            return Ok(TurnProvider {
                provider: Box::new(OpenAiResponsesProvider::new(ResponsesConfig {
                    base_url: config.base_url.clone(),
                    identity,
                    api_key,
                    model: config
                        .selected_model
                        .clone()
                        .unwrap_or_else(|| "probe".to_string()),
                    max_output_tokens: Some(
                        config
                            .overrides
                            .get("maxTokens")
                            .and_then(Value::as_u64)
                            .map(|value| value as u32)
                            .unwrap_or(DEFAULT_MAX_TOKENS),
                    ),
                    temperature: Some(
                        config
                            .overrides
                            .get("temperature")
                            .and_then(Value::as_f64)
                            .map(|value| value as f32)
                            .unwrap_or(DEFAULT_TEMPERATURE),
                    ),
                    capabilities,
                })),
            });
        }

        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
            base_url: config.base_url.clone(),
            identity,
            api_key,
            model: config
                .selected_model
                .clone()
                .unwrap_or_else(|| "probe".to_string()),
            // Sampling defaults are provider semantics, so they live here and not
            // in a platform's settings form. A platform may still override them
            // through `overrides`; it just should not have to know the defaults.
            max_tokens: Some(
                config
                    .overrides
                    .get("maxTokens")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32)
                    .unwrap_or(DEFAULT_MAX_TOKENS),
            ),
            temperature: Some(
                config
                    .overrides
                    .get("temperature")
                    .and_then(Value::as_f64)
                    .map(|value| value as f32)
                    .unwrap_or(DEFAULT_TEMPERATURE),
            ),
            supports_developer_role: false,
            // What the endpoint said it can do, not what a form guessed. The
            // protocol was chosen by negotiation, and a stated override is only
            // still allowed to speak about reasoning on top of that.
            capabilities,
        });

        Ok(TurnProvider {
            provider: Box::new(provider),
        })
    }

    pub fn cancel(&self) {
        if let Some(token) = self.cancel.lock().unwrap().as_ref() {
            token.cancel();
        }
    }

    /// Commit a turn to its session.
    ///
    /// The whole turn is stored: the user message, every assistant step, every
    /// tool call with its result, and the provider continuation state. Only
    /// `final_text` is what the user reads; the rest is what the model was
    /// given, and dropping it here is what used to make a tool-using turn
    /// collapse into two messages on the next request.
    fn persist_turn(&self, request: &SendTurnRequest, outcome: &AgentOutcome) {
        let now_ms = now_ms();
        let mut inner = self.inner.lock().unwrap();
        let Some(session) = inner
            .catalog
            .sessions
            .iter_mut()
            .find(|session| session.id == request.session_id)
        else {
            return;
        };

        let mut turn = outcome.turn.clone();
        turn.id = next_turn_id(&session.transcript);
        turn.created_at_ms = Some(now_ms);
        session.transcript.push(turn);
        session
            .metadata
            .insert("updatedAtMs".to_string(), Value::from(now_ms));

        drop(inner);

        self.persist();
    }
}

impl Drop for SujiuRuntime {
    fn drop(&mut self) {
        // A turn may still be running on another thread; stop it before the
        // tokio runtime shuts down.
        self.cancel();
    }
}

/// Why a turn could not be started.
///
/// A turn that started and then stopped is not a failure here: it is committed
/// to the transcript and reported through the event sink.
struct TurnFailure(String);

/// The provider used for one turn, erased so callers never see an adapter.
///
/// The protocol it speaks is not repeated here: it is already inside the
/// `EndpointCapabilities` the adapter was built from, and a second copy could
/// disagree with the one the adapter actually uses.
struct TurnProvider {
    provider: Box<dyn sujiu_ai::AiProvider>,
}

/// Everything one turn needs before a provider exists.
struct PreparedTurn {
    messages: Vec<sujiu_ai::ModelMessage>,
    continuation: Option<ProviderContinuation>,
    config: EndpointConfig,
    identity: ProviderIdentity,
    api_key: String,
}

/// Overlay a per-turn provider onto the stored one.
///
/// A platform that only means to change the model should not have to restate
/// the endpoint and its capabilities, and a platform that restates them
/// incompletely should not silently drop them. Fields the override genuinely
/// sets win; the rest are inherited.
fn merge_endpoint(stored: &EndpointConfig, override_: &EndpointConfig) -> EndpointConfig {
    let blank = |value: &str| value.trim().is_empty();

    let overrides = if override_.overrides.is_empty() {
        stored.overrides.clone()
    } else {
        let mut overrides = stored.overrides.clone();
        overrides.extend(override_.overrides.clone());
        overrides
    };

    EndpointConfig {
        id: if blank(&override_.id) {
            stored.id.clone()
        } else {
            override_.id.clone()
        },
        name: if blank(&override_.name) {
            stored.name.clone()
        } else {
            override_.name.clone()
        },
        base_url: if blank(&override_.base_url) {
            stored.base_url.clone()
        } else {
            override_.base_url.clone()
        },
        // A model is a choice, not an inherited property, so `None` here means
        // "I have not chosen one", not "keep the stored one". A caller that
        // wants the stored model inherits it by not sending an override at all.
        selected_model: override_
            .selected_model
            .clone()
            .filter(|m| !m.trim().is_empty()),
        credential_ref: override_
            .credential_ref
            .clone()
            .or_else(|| stored.credential_ref.clone()),
        overrides,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> EndpointConfig {
        EndpointConfig {
            id: "stored".into(),
            name: "Stored".into(),
            base_url: "https://stored.example/v1".into(),
            selected_model: Some("stored-model".into()).into(),
            credential_ref: Some("stored-key".into()),
            overrides: serde_json::Map::new(),
        }
    }

    /// A platform that resends a partial provider for one turn used to throw
    /// the whole saved config away, including a capability it had no field for.
    #[test]
    fn a_partial_per_turn_provider_keeps_what_it_does_not_mention() {
        let mut stored = config();
        stored.overrides.insert(
            sujiu_core::REPLAYS_ASSISTANT_REASONING_KEY.to_string(),
            serde_json::Value::Bool(true),
        );

        // The shape a settings form actually sends back: a name and a model.
        let override_ = EndpointConfig {
            id: String::new(),
            name: "Same provider".into(),
            base_url: String::new(),
            selected_model: Some("another-model".into()),
            credential_ref: None,
            overrides: serde_json::Map::new(),
        };

        let merged = merge_endpoint(&stored, &override_);

        assert_eq!(merged.base_url, "https://stored.example/v1");
        assert_eq!(merged.id, "stored");
        assert_eq!(
            merged.credential_ref.as_deref(),
            Some("stored-key"),
            "a turn must not lose the credential the user saved"
        );
        assert_eq!(
            merged.selected_model.as_deref(),
            Some("another-model"),
            "the model was overridden"
        );
        assert_eq!(
            merged.forced_reasoning_replay(),
            Some(true),
            "a capability the turn never mentioned has to survive the turn"
        );
    }

    #[test]
    fn a_per_turn_provider_may_still_state_a_capability_of_its_own() {
        let mut stored = config();
        stored.overrides.insert(
            sujiu_core::REPLAYS_ASSISTANT_REASONING_KEY.to_string(),
            serde_json::Value::Bool(true),
        );

        let mut override_ = config();
        override_.id = "turn".into();
        override_.base_url = "https://other.example/v1".into();
        override_.overrides.insert(
            sujiu_core::REPLAYS_ASSISTANT_REASONING_KEY.to_string(),
            serde_json::Value::Bool(false),
        );

        let merged = merge_endpoint(&stored, &override_);

        assert_eq!(merged.base_url, "https://other.example/v1");
        assert_eq!(
            merged.forced_reasoning_replay(),
            Some(false),
            "an override that really does state a capability still wins"
        );
    }

    #[test]
    fn a_capability_the_turn_does_not_repeat_is_not_an_agreement_to_drop_it() {
        // The per-key case: a turn that sets one extra key must not clear the
        // others, which is what a whole-object replacement would have done.
        let mut stored = config();
        stored.overrides.insert(
            sujiu_core::REPLAYS_ASSISTANT_REASONING_KEY.to_string(),
            serde_json::Value::Bool(true),
        );
        stored
            .overrides
            .insert("temperature".to_string(), serde_json::Value::from(0.3));

        let mut override_ = config();
        override_
            .overrides
            .insert("maxTokens".to_string(), serde_json::Value::from(512));

        let merged = merge_endpoint(&stored, &override_);

        assert_eq!(
            merged.overrides.get("maxTokens"),
            Some(&serde_json::json!(512))
        );
        assert_eq!(
            merged.overrides.get("temperature"),
            Some(&serde_json::json!(0.3)),
            "an unrelated saved setting is not collateral damage"
        );
        assert_eq!(merged.forced_reasoning_replay(), Some(true));
    }
}

impl TurnProvider {
    fn as_ref(&self) -> &dyn sujiu_ai::AiProvider {
        self.provider.as_ref()
    }
}

/// A turn id that cannot collide with one already in the transcript.
///
/// Archived turns count. After a compaction the live list is shorter than it
/// was, so numbering from `len()` alone would hand out an id that a compacted
/// turn already owns, and turn ids are what the UI uses to key its bubbles.
fn next_turn_id(transcript: &Transcript) -> String {
    let mut number = transcript.len() + 1;
    loop {
        let id = format!("turn-{number}");
        if !transcript.turn_ids().any(|existing| existing == id) {
            return id;
        }
        number += 1;
    }
}

/// How many tool calls a finished turn made, across every step.
///
/// Counted rather than read from one place because a tool-using turn spreads
/// them over several steps, and a log that reported only the last step's count
/// would under-report exactly the turns worth investigating.
fn count_tool_calls(outcome: &AgentOutcome) -> usize {
    outcome
        .turn
        .steps
        .iter()
        .map(|step| step.tool_calls.len())
        .sum()
}

/// The wire label of a tool call state.
fn tool_call_status(state: sujiu_core::ToolCallState) -> &'static str {
    match state {
        sujiu_core::ToolCallState::Completed => "completed",
        sujiu_core::ToolCallState::Failed => "failed",
        sujiu_core::ToolCallState::Interrupted => "interrupted",
        sujiu_core::ToolCallState::Cancelled => "cancelled",
    }
}

/// Wall-clock milliseconds, for the metadata a session list sorts on.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn protocol_label(protocol: Protocol) -> String {
    match protocol {
        Protocol::OpenAiResponses => "openai_responses".to_string(),
        Protocol::OpenAiChatCompletions => "openai_chat_completions".to_string(),
        Protocol::AnthropicMessages => "anthropic_messages".to_string(),
    }
}

/// A bounded projection of stored text, for list rows.
///
/// Unlike a teaser this invents nothing: it collapses existing whitespace and
/// stops at a length, so a platform can show it as is or clip it further. It
/// stays here because the length bound is about what crosses the FFI boundary,
/// not about how the row looks.
fn projected_preview(content: &str) -> String {
    const PREVIEW_CHARS: usize = 90;

    let single_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= PREVIEW_CHARS {
        return single_line;
    }

    let truncated: String = single_line.chars().take(PREVIEW_CHARS).collect();
    format!("{truncated}…")
}

/// Reporter that collects events into memory, used by the synchronous
/// entrypoints and by tests.
#[derive(Default)]
pub struct CollectingReporter {
    pub events: Vec<crate::events::TurnEvent>,
}

impl TurnEventReporter for CollectingReporter {
    fn report(&mut self, event: crate::events::TurnEvent) {
        self.events.push(event);
    }
}

impl CollectingReporter {
    pub fn kinds(&self) -> Vec<TurnEventKind> {
        self.events.iter().map(|event| event.kind).collect()
    }

    pub fn joined_text(&self) -> String {
        self.events
            .iter()
            .filter(|event| event.kind == TurnEventKind::TextDelta)
            .filter_map(|event| event.text.clone())
            .collect()
    }
}
