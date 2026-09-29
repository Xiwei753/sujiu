//! Stateful conversation runtime behind the C ABI.
//!
//! The runtime owns the provider-neutral data a frontend needs: sessions,
//! characters, models, context sources and conversation state, plus the turn
//! loop built on the shared agent runtime. Platform code receives only
//! normalized turn events from `events`, never provider wire formats or tool
//! internals.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sujiu_ai::{
    register_standard_context_tools, AgentConfig, AgentError, AgentOutcome, AgentRuntime,
    CancelToken, ContextStore, ContextStoreError, InMemoryContextStore, OpenAiCompatConfig,
    OpenAiCompatProvider, ToolRegistry,
};
use sujiu_core::{
    Character, ChatMessage, ChatRole, ContextKind, ContextRecord, ContextSource, PromptCompiler,
    ProviderConfig, ProviderKind, Session, DEFAULT_APP_SYSTEM_PROMPT,
};

use crate::events::{TurnEventKind, TurnEventReporter, TurnEventSink};
use crate::storage::{AppStorage, FileStorage, MemoryStorage};

/// Upper bound on the transcript a single conversation state call returns.
const MAX_TRANSCRIPT_MESSAGES: usize = 200;

/// Provider kinds this runtime can actually drive.
///
/// A frontend must only offer these. Accepting a kind it cannot serve would let
/// a user configure a provider and then fail every turn.
const SUPPORTED_PROVIDER_KINDS: &[ProviderKind] = &[ProviderKind::OpenAiCompatible];

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
    pub provider_id: String,
    pub provider_name: String,
    pub kind: String,
    pub configured: bool,
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
    pub provider: Option<ProviderConfig>,
    /// Credential for this turn. Supplied by the platform secret store per
    /// call and never persisted by the runtime.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug)]
pub enum TurnError {
    SessionNotFound(String),
    NoProviderConfigured,
    UnsupportedProviderKind(String),
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
            Self::UnsupportedProviderKind(kind) => {
                write!(formatter, "unsupported_provider_kind: {kind}")
            }
            Self::MissingCredential => write!(formatter, "missing_credential"),
            Self::ContextStore(error) => write!(formatter, "{error}"),
        }
    }
}

struct Catalog {
    characters: Vec<Character>,
    sessions: Vec<Session>,
    /// Tool calls of the most recent turn, keyed by the assistant message id.
    tool_calls: Vec<(String, Vec<ToolCallSummary>)>,
}

struct Inner {
    catalog: Catalog,
    store: Arc<InMemoryContextStore>,
    tools: Arc<ToolRegistry>,
    provider_config: Option<ProviderConfig>,
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
    tool_calls: Vec<(String, Vec<ToolCallSummary>)>,
    provider_config: Option<ProviderConfig>,
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
const SNAPSHOT_FILE: &str = "sujiu-runtime.json";
const SNAPSHOT_VERSION: u32 = 1;

/// The stateful runtime exported across the FFI boundary.
pub struct SujiuRuntime {
    inner: Mutex<Inner>,
    cancel: Mutex<Option<CancelToken>>,
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
        let snapshot = if restore {
            storage
                .load(SNAPSHOT_FILE)
                .and_then(|document| serde_json::from_str::<Snapshot>(&document).ok())
        } else {
            None
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
                tool_calls: snapshot.tool_calls.clone(),
            },
            None => Catalog {
                characters: seed.characters,
                sessions: seed.sessions,
                tool_calls: Vec::new(),
            },
        };

        let provider_config = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.provider_config.clone());

        let runtime = Self {
            inner: Mutex::new(Inner {
                catalog,
                store,
                tools: Arc::new(tools),
                provider_config,
            }),
            cancel: Mutex::new(None),
            storage: Mutex::new(storage),
            tokio,
        };

        if restore {
            // A first launch writes its seed, so the next launch is a restore
            // and not a reset.
            runtime.persist();
        }

        Ok(runtime)
    }

    /// Point the runtime at a directory a platform chose, restoring a document
    /// there if one exists. The directory is created if it is missing.
    pub fn use_directory(&self, path: &str) -> Result<(), String> {
        let storage = FileStorage::new(path).map_err(|error| error.to_string())?;
        if let Some(document) = storage.load(SNAPSHOT_FILE) {
            if let Ok(snapshot) = serde_json::from_str::<Snapshot>(&document) {
                let mut inner = self.inner.lock().unwrap();
                inner.catalog = Catalog {
                    characters: snapshot.characters,
                    sessions: snapshot.sessions,
                    tool_calls: snapshot.tool_calls,
                };
                inner.provider_config = snapshot.provider_config;
                inner.store.clear();
                for source in snapshot.sources {
                    inner.store.add_source(source);
                }
                for record in snapshot.records {
                    inner.store.add_record(record);
                }
            }
        }
        *self.storage.lock().unwrap() = Arc::new(storage);
        self.persist();
        Ok(())
    }

    /// The directory the runtime persists into, or `None` while it is not
    /// attached to one.
    pub fn directory(&self) -> Option<String> {
        self.storage.lock().unwrap().location()
    }

    fn persist(&self) {
        let snapshot = {
            let inner = self.inner.lock().unwrap();
            let (sources, records) = inner.store.snapshot();
            Snapshot {
                version: SNAPSHOT_VERSION,
                characters: inner.catalog.characters.clone(),
                sessions: inner.catalog.sessions.clone(),
                sources,
                records,
                tool_calls: inner.catalog.tool_calls.clone(),
                provider_config: inner.provider_config.clone(),
            }
        };

        if let Ok(document) = serde_json::to_string(&snapshot) {
            self.storage.lock().unwrap().save(SNAPSHOT_FILE, &document);
        }
    }

    pub fn provider_config(&self) -> Option<ProviderConfig> {
        self.inner.lock().unwrap().provider_config.clone()
    }

    /// Provider kinds a frontend may offer, in a stable order.
    pub fn supported_provider_kinds() -> Vec<String> {
        SUPPORTED_PROVIDER_KINDS
            .iter()
            .map(|kind| provider_kind_label(*kind))
            .collect()
    }

    /// Store the provider a frontend configured.
    ///
    /// An unsupported kind is rejected here rather than at the first turn, so
    /// the settings screen learns immediately that it cannot offer it.
    pub fn set_provider_config(&self, config: Option<ProviderConfig>) -> Result<(), TurnError> {
        if let Some(config) = config.as_ref() {
            if !SUPPORTED_PROVIDER_KINDS.contains(&config.kind) {
                return Err(TurnError::UnsupportedProviderKind(provider_kind_label(
                    config.kind,
                )));
            }
        }
        self.inner.lock().unwrap().provider_config = config;
        self.persist();
        Ok(())
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

                SessionSummary {
                    // A projection of the last message, not a display string. The
                    // bound exists so a session list does not carry whole
                    // messages across the boundary; how a platform renders or
                    // further clips it is that platform's business.
                    preview: session
                        .messages
                        .last()
                        .map(|message| projected_preview(&message.content))
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
                    message_count: session.messages.len(),
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
        let Some(config) = inner.provider_config.clone() else {
            return Vec::new();
        };

        vec![ModelSummary {
            id: config.model.clone(),
            name: config.model.clone(),
            provider_id: config.id.clone(),
            provider_name: config.name.clone(),
            kind: provider_kind_label(config.kind),
            configured: !config.base_url.trim().is_empty() && !config.model.trim().is_empty(),
        }]
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

        let messages = session
            .messages
            .iter()
            .rev()
            .take(MAX_TRANSCRIPT_MESSAGES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|message| MessageSummary {
                id: message.id.clone(),
                role: message.role,
                text: message.content.clone(),
                tool_calls: inner
                    .catalog
                    .tool_calls
                    .iter()
                    .find(|(id, _)| id == &message.id)
                    .map(|(_, calls)| calls)
                    .cloned()
                    .unwrap_or_default(),
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
                    messages: Vec::new(),
                    metadata: serde_json::Map::new(),
                },
            );

            id
        };

        self.persist();
        id
    }

    /// Run one agent turn, reporting normalized events to `reporter`.
    pub async fn send_turn(&self, request: SendTurnRequest, reporter: &mut dyn TurnEventReporter) {
        let cancel = CancelToken::new();
        *self.cancel.lock().unwrap() = Some(cancel.clone());

        let mut sink = TurnEventSink::new(reporter, cancel);
        sink.turn_started();

        match self.run_turn(&request, &mut sink).await {
            Ok(()) => {}
            Err(TurnFailure::Cancelled) => sink.turn_cancelled(),
            Err(TurnFailure::Error(error)) => sink.turn_failed(&error.to_string()),
        }

        *self.cancel.lock().unwrap() = None;
    }

    async fn run_turn(
        &self,
        request: &SendTurnRequest,
        sink: &mut TurnEventSink<'_>,
    ) -> Result<(), TurnFailure> {
        let (messages, provider) = self
            .prepare(request)
            .map_err(|error| TurnFailure::Error(error.to_string()))?;
        let tools = self.inner.lock().unwrap().tools.clone();

        let runtime = AgentRuntime::new(provider.as_ref(), tools.as_ref(), AgentConfig::default());
        let outcome = runtime
            .run_streaming(messages, &request.user_text, sink)
            .await
            .map_err(|error| match error {
                AgentError::Cancelled => TurnFailure::Cancelled,
                other => TurnFailure::Error(other.to_string()),
            })?;

        self.persist_turn(request, &outcome);
        sink.turn_completed(&outcome.final_text);

        Ok(())
    }

    /// Build the model messages and the provider for one turn.
    fn prepare(
        &self,
        request: &SendTurnRequest,
    ) -> Result<(Vec<sujiu_ai::ModelMessage>, TurnProvider), TurnError> {
        let inner = self.inner.lock().unwrap();

        let session = inner
            .catalog
            .sessions
            .iter()
            .find(|session| session.id == request.session_id)
            .cloned()
            .ok_or_else(|| TurnError::SessionNotFound(request.session_id.clone()))?;

        let config = request
            .provider
            .clone()
            .or_else(|| inner.provider_config.clone())
            .ok_or(TurnError::NoProviderConfigured)?;

        if config.base_url.trim().is_empty() || config.model.trim().is_empty() {
            return Err(TurnError::NoProviderConfigured);
        }

        if !SUPPORTED_PROVIDER_KINDS.contains(&config.kind) {
            return Err(TurnError::UnsupportedProviderKind(provider_kind_label(
                config.kind,
            )));
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
            &session.messages,
            &request.user_text,
        );

        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
            base_url: config.base_url.clone(),
            api_key,
            model: config.model.clone(),
            // Sampling defaults are provider semantics, so they live here and not
            // in a platform's settings form. A platform may still override them
            // through `extra`; it just should not have to know the defaults.
            max_tokens: Some(
                config
                    .extra
                    .get("maxTokens")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32)
                    .unwrap_or(DEFAULT_MAX_TOKENS),
            ),
            temperature: Some(
                config
                    .extra
                    .get("temperature")
                    .and_then(Value::as_f64)
                    .map(|value| value as f32)
                    .unwrap_or(DEFAULT_TEMPERATURE),
            ),
            supports_developer_role: false,
        });

        Ok((
            sujiu_ai::messages_from_prompt_plan(&plan),
            TurnProvider {
                provider: Box::new(provider),
            },
        ))
    }

    pub fn cancel(&self) {
        if let Some(token) = self.cancel.lock().unwrap().as_ref() {
            token.cancel();
        }
    }

    fn persist_turn(&self, request: &SendTurnRequest, outcome: &AgentOutcome) {
        let mut inner = self.inner.lock().unwrap();
        let Some(session) = inner
            .catalog
            .sessions
            .iter_mut()
            .find(|session| session.id == request.session_id)
        else {
            return;
        };

        let user_id = next_message_id(session);
        session.messages.push(ChatMessage {
            id: user_id,
            role: ChatRole::User,
            content: request.user_text.clone(),
            metadata: serde_json::Map::new(),
        });

        let assistant_id = next_message_id(session);
        session.messages.push(ChatMessage {
            id: assistant_id.clone(),
            role: ChatRole::Assistant,
            content: outcome.final_text.clone(),
            metadata: serde_json::Map::new(),
        });

        // The tool's own title is the display name the runtime already carries,
        // so pass it through. Deriving one from the tool id here would both
        // discard that title and put English casing in the runtime, which is not
        // where display text belongs.
        let titles: BTreeMap<String, String> = inner
            .tools
            .definitions_for(outcome.tool_results.iter().map(|r| &r.name))
            .into_iter()
            .filter_map(|definition| definition.title.map(|title| (definition.name, title)))
            .collect();

        let calls = outcome
            .tool_results
            .iter()
            .map(|result| ToolCallSummary {
                id: result.call_id.clone(),
                name: result.name.clone(),
                title: titles
                    .get(&result.name)
                    .cloned()
                    .unwrap_or_else(|| result.name.clone()),
                status: if result.output.is_error {
                    "failed".to_string()
                } else {
                    "completed".to_string()
                },
                is_error: result.output.is_error,
                result_text: result.output.model_text(),
            })
            .collect();

        inner
            .catalog
            .tool_calls
            .retain(|(id, _)| id != &assistant_id);
        inner.catalog.tool_calls.push((assistant_id, calls));
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

enum TurnFailure {
    Cancelled,
    Error(String),
}

/// The provider used for one turn, erased so callers never see an adapter.
struct TurnProvider {
    provider: Box<dyn sujiu_ai::AiProvider>,
}

impl TurnProvider {
    fn as_ref(&self) -> &dyn sujiu_ai::AiProvider {
        self.provider.as_ref()
    }
}

fn next_message_id(session: &Session) -> String {
    format!("msg-{}", session.messages.len() + 1)
}

fn provider_kind_label(kind: sujiu_core::ProviderKind) -> String {
    match kind {
        sujiu_core::ProviderKind::OpenAiCompatible => "openai_compatible".to_string(),
        sujiu_core::ProviderKind::Anthropic => "anthropic".to_string(),
        sujiu_core::ProviderKind::Gemini => "gemini".to_string(),
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
