//! napi surface for the shared Rust runtime.
//!
//! This crate is the only place where Rust types are shaped for a JavaScript
//! caller. It stays deliberately thin: the runtime owns conversation
//! semantics, and this layer only translates between the runtime's domain
//! types and the data a platform bridge needs.
//!
//! Turn events cross the boundary as JSON strings so the event vocabulary
//! stays provider neutral and can evolve without breaking the platform side.

use std::sync::Arc;

use napi::bindgen_prelude::{AsyncTask, Error, Result, Task};
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use sujiu_ai::DiagnosticEntry;
use sujiu_core::EndpointConfig;
use sujiu_core::{ParticipantRole, WorldBookPosition};
use sujiu_ffi::events::{TurnEvent, TurnEventReporter};
use sujiu_ffi::library::{
    CharacterRequest, PersonaRequest, PersonaSummary, PromptProfileRequest, PromptProfileSummary,
    WorldBookEntryRequest, WorldBookRequest, WorldBookSummary,
};
use sujiu_ffi::runtime::{
    CharacterSummary, ContextSourceSummary, ConversationSnapshot, CreateConversationRequest,
    EndpointExploration, ModelDiscovery, ModelSummary, ParticipantRequest, ParticipantSummary,
    SendTurnRequest, SessionSummary, SujiuRuntime, ToolCallSummary,
};

/// A live Rust runtime handed to the platform bridge.
///
/// The platform never sees provider wire formats or tool internals, only
/// domain summaries and normalized turn events.
#[napi]
pub struct SujiuRuntimeBridge {
    runtime: Arc<SujiuRuntime>,
}

/// One character taking part in a conversation.
///
/// A conversation is the root of a chat, and it may hold no participant, one
/// character, or a whole table of them. Nothing here implies a "main"
/// character; a table top is a game master plus several others.
#[napi(object)]
pub struct ParticipantDto {
    pub character_id: String,
    pub name: String,
    /// `character` or `narrator`, as a label a screen may show.
    pub role: String,
}

/// What a screen is asked to open a conversation with.
///
/// A bare character id still creates a one-participant conversation, so an
/// existing platform flow keeps working while the model underneath is already
/// group-shaped.
#[napi(object)]
pub struct ParticipantRequestDto {
    pub character_id: String,
    pub role: Option<String>,
    pub display_name: Option<String>,
}

#[napi(object)]
pub struct SessionSummaryDto {
    pub id: String,
    /// The first participant, for a screen that still shows one character.
    /// Read [`Self::participants`] for the conversation as it really is.
    pub character_id: String,
    /// The first participant's name, for the same reason.
    pub character_name: String,
    pub participants: Vec<ParticipantDto>,
    pub preview: String,
    pub title: String,
    pub updated_at_ms: f64,
    pub message_count: u32,
}

#[napi(object)]
pub struct CharacterSummaryDto {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[napi(object)]
pub struct ModelSummaryDto {
    pub id: String,
    pub name: String,
    pub endpoint_id: String,
    /// A word to show a person, derived from the address. It names who answers
    /// and decides nothing: a gateway behind the same name can serve unrelated
    /// models, and the runtime never asks it.
    pub endpoint_label: String,
    pub configured: bool,
}

/// What model discovery found, and what the user may still do about it.
#[napi(object)]
pub struct ModelDiscoveryDto {
    pub listing: String,
    pub note: String,
    /// Always true, so a settings screen can keep the manual field enabled.
    pub manual_entry_allowed: bool,
    pub models: Vec<ModelSummaryDto>,
}

impl From<ModelDiscovery> for ModelDiscoveryDto {
    fn from(discovery: ModelDiscovery) -> Self {
        Self {
            listing: discovery.listing,
            note: discovery.note,
            manual_entry_allowed: discovery.manual_entry_allowed,
            models: discovery
                .models
                .into_iter()
                .map(ModelSummaryDto::from)
                .collect(),
        }
    }
}

/// What an unsaved endpoint turned out to be.
///
/// `protocol` and `models` are two independent answers, and reporting them as
/// one would be a claim Sujiu cannot make: an endpoint that lists nothing may
/// still speak the newest protocol, and one that lists fifty models may speak
/// only the oldest.
#[napi(object)]
pub struct EndpointExplorationDto {
    pub protocol: String,
    pub protocols: Vec<String>,
    /// How the protocol probe came out, as a code to branch on.
    ///
    /// A settings screen shows a different message per code. "Probe failed" as a
    /// single message is the state this replaces: a wrong key, a wrong path and
    /// a switched-off router all read the same, and each of them sends the user
    /// to edit a different field.
    pub status: String,
    /// Why no protocol was settled, when none was. A sentence to read; `status`
    /// is what to switch on.
    pub reason: Option<String>,
    pub models: ModelDiscoveryDto,
}

impl From<EndpointExploration> for EndpointExplorationDto {
    fn from(exploration: EndpointExploration) -> Self {
        Self {
            protocol: exploration.protocol,
            protocols: exploration.protocols,
            status: exploration.status,
            reason: exploration.reason,
            models: ModelDiscoveryDto::from(exploration.models),
        }
    }
}

/// One line of the diagnostic log, as a settings screen or a bug report needs it.
///
/// The text arrived already redacted. Nothing here is a raw body, and nothing
/// here is a credential: the runtime removes those before a line is ever
/// recorded, so a platform can render or share this without a second pass.
#[napi(object)]
pub struct DiagnosticEntryDto {
    /// `discovery` for capability and model probing, `chat` for a conversation.
    /// The two are separate investigations and a reader must be able to tell
    /// which one a line belongs to.
    pub kind: String,
    /// Where in that investigation this line sits: `probe_start`,
    /// `protocol_result`, `chat_request`, `turn_failed`, and so on.
    pub stage: String,
    pub message: String,
    pub at_ms: f64,
    /// The detail as a rendered `key=value` line, ready to show.
    pub detail: String,
}

impl From<&DiagnosticEntry> for DiagnosticEntryDto {
    fn from(entry: &DiagnosticEntry) -> Self {
        Self {
            kind: entry.kind.label().to_string(),
            stage: entry.stage.clone(),
            message: entry.message.clone(),
            at_ms: entry.at_ms as f64,
            detail: entry.render(),
        }
    }
}

/// An endpoint that is already stored, as far as a form may know one.
///
/// No credential field, and none is wanted: a form that could read the key back
/// would be one `dump the view state` away from writing it to a log. The key is
/// the one thing a settings screen asks to be told "yes you have one" and
/// nothing more.
#[napi(object)]
pub struct StoredEndpointDto {
    pub id: String,
    pub base_url: String,
    /// `None` until the user has chosen one, which is a normal state: an
    /// endpoint can be asked what it offers before anyone picks a model.
    pub selected_model: Option<String>,
    /// A word to show a person. Decides nothing.
    pub label: String,
}

#[napi(object)]
pub struct ContextSourceDto {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub record_count: u32,
}

#[napi(object)]
pub struct ToolCallDto {
    pub id: String,
    pub name: String,
    pub title: String,
    pub status: String,
    pub is_error: bool,
    pub result_text: String,
}

#[napi(object)]
pub struct MessageDto {
    pub id: String,
    pub role: String,
    /// Which participant spoke, when the transcript says.
    ///
    /// A conversation can hold several characters, so `role: "assistant"` alone
    /// does not say who is talking. Undefined means the step carries no
    /// attribution: choosing who speaks next is a speaking-order policy, and the
    /// runtime does not invent one.
    pub speaker_id: Option<String>,
    pub text: String,
    pub tool_calls: Vec<ToolCallDto>,
}

#[napi(object)]
pub struct ConversationSnapshotDto {
    pub session_id: String,
    /// The first participant, for a screen that still shows one character.
    pub character: Option<CharacterSummaryDto>,
    pub participants: Vec<ParticipantDto>,
    pub persona_id: Option<String>,
    pub worldbook_ids: Vec<String>,
    pub prompt_profile_id: Option<String>,
    pub messages: Vec<MessageDto>,
}

/// One stored persona, as a list row.
///
/// A persona is an entity a user owns, not a field of a character card, which is
/// why it has its own list here. Nothing in this shape implies that anything is
/// bound to anything: which persona a chat uses is that conversation's answer,
/// and it is asked separately.
#[napi(object)]
pub struct PersonaDto {
    pub id: String,
    pub name: String,
    pub description: String,
    /// How many world books this persona brings of its own.
    pub worldbook_count: u32,
}

/// A persona to store.
///
/// `id` is absent or blank to create one. A present id that names nothing is
/// refused rather than treated as a create: a screen that lost the persona it was
/// editing would otherwise be told it saved, and would have saved a duplicate
/// under the same name.
#[napi(object)]
pub struct PersonaRequestDto {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub user_prompt: String,
    pub worldbook_ids: Vec<String>,
}

/// One world-book entry, as an editor sees it.
#[napi(object)]
pub struct WorldBookEntryDto {
    pub id: String,
    pub name: String,
    /// The entry's text, unshortened.
    ///
    /// This is lore the model reads, so a preview here would be a truncation
    /// rule baked into the bridge, and choosing what counts as the interesting
    /// sentence of a piece of lore is not this layer's decision to make.
    pub content: String,
    pub keys: Vec<String>,
    pub enabled: bool,
    pub constant: bool,
    /// `before_character`, `after_character` or `near_history`.
    pub position: String,
}

/// One world book, with its entries.
#[napi(object)]
pub struct WorldBookDto {
    pub id: String,
    pub name: String,
    pub entries: Vec<WorldBookEntryDto>,
}

/// A world-book entry to store.
#[napi(object)]
pub struct WorldBookEntryRequestDto {
    pub id: Option<String>,
    pub name: String,
    pub content: String,
    pub keys: Vec<String>,
    /// Absent means the runtime's default, which is enabled: an entry the user
    /// just wrote and cannot see is worse than one that works.
    pub enabled: Option<bool>,
    pub constant: Option<bool>,
    pub position: Option<String>,
}

/// A world book to store. Same id rule as [`PersonaRequestDto`].
#[napi(object)]
pub struct WorldBookRequestDto {
    pub id: Option<String>,
    pub name: String,
    pub entries: Vec<WorldBookEntryRequestDto>,
}

/// One prompt profile, as a list row.
///
/// Only the name. The four prompt bodies are what the profile *says*, and a row
/// carrying all of them would be a wall of text rather than a row.
#[napi(object)]
pub struct PromptProfileDto {
    pub id: String,
    pub name: String,
}

/// A prompt profile to store. Same id rule as [`PersonaRequestDto`].
///
/// The named fields are the whole of what a basic editor can change, and the
/// runtime keeps the profile's fixed segments when this is saved.
#[napi(object)]
pub struct PromptProfileRequestDto {
    pub id: Option<String>,
    pub name: String,
    pub system_prompt: String,
    pub user_prompt: String,
    pub post_history_instructions: String,
    pub format_rules: String,
}

/// A character to store. Same id rule as [`PersonaRequestDto`].
#[napi(object)]
pub struct CharacterRequestDto {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub first_message: String,
    pub alternate_greetings: Vec<String>,
    pub example_dialogue: String,
    pub system_prompt: String,
    pub post_history_instructions: String,
    pub worldbook_ids: Vec<String>,
}

#[napi(object)]
pub struct ProviderConfigDto {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// The model the user picked, if they have picked one yet.
    ///
    /// Optional because exploration runs first: an endpoint is worth asking
    /// about before anyone has chosen what to ask it. A form that insists on
    /// a model before it will save is the reason discovery used to have to
    /// come last.
    pub selected_model: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    /// Whether this endpoint needs the assistant reasoning that produced a tool
    /// call sent back with it.
    ///
    /// Optional because most endpoints do not, and a known thinking-mode
    /// service is recognised without it. A platform only describes the provider
    /// it was given; the protocol requirement is the runtime's answer, so this
    /// is a stated capability rather than something a form has to know about.
    pub replays_assistant_reasoning: Option<bool>,
}

#[napi(object)]
pub struct TurnRequestDto {
    pub session_id: String,
    pub user_text: String,
    pub provider: Option<ProviderConfigDto>,
    pub api_key: Option<String>,
}

/// Renders a domain enum as the snake_case label the bridge contract uses.
fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Reads a participant role a screen sent, defaulting to a speaking character.
///
/// A screen is not required to know the vocabulary: an unrecognised or missing
/// label means the ordinary case rather than a rejected conversation.
fn participant_role(label: &str) -> ParticipantRole {
    match label {
        "narrator" | "gm" | "gameMaster" | "game_master" => ParticipantRole::Narrator,
        _ => ParticipantRole::Character,
    }
}

/// Reads a world-book position a screen sent.
///
/// An unrecognised or absent label means `after_character`, the domain default.
/// Guessing one of the other two would be worse: a position that is wrong
/// silently reorders part of the prompt block the provider is meant to cache, and
/// a screen that cannot express a position can still express "where it normally
/// goes".
fn world_book_position(label: &str) -> WorldBookPosition {
    match label {
        "before_character" | "beforeCharacter" => WorldBookPosition::BeforeCharacter,
        "near_history" | "nearHistory" => WorldBookPosition::NearHistory,
        _ => WorldBookPosition::AfterCharacter,
    }
}

impl From<ParticipantSummary> for ParticipantDto {
    fn from(value: ParticipantSummary) -> Self {
        Self {
            character_id: value.character_id,
            name: value.name,
            role: value.role,
        }
    }
}

impl From<SessionSummary> for SessionSummaryDto {
    fn from(value: SessionSummary) -> Self {
        Self {
            id: value.id,
            character_id: value.character_id.unwrap_or_default(),
            character_name: value.character_name,
            participants: value
                .participants
                .into_iter()
                .map(ParticipantDto::from)
                .collect(),
            preview: value.preview,
            title: value.title,
            updated_at_ms: value.updated_at_ms as f64,
            message_count: value.message_count as u32,
        }
    }
}

impl From<CharacterSummary> for CharacterSummaryDto {
    fn from(value: CharacterSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            description: value.description,
        }
    }
}

impl From<ModelSummary> for ModelSummaryDto {
    fn from(value: ModelSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            endpoint_id: value.endpoint_id,
            endpoint_label: value.endpoint_label,
            configured: value.configured,
        }
    }
}

impl From<ContextSourceSummary> for ContextSourceDto {
    fn from(value: ContextSourceSummary) -> Self {
        Self {
            id: value.id,
            kind: enum_label(&value.kind),
            name: value.name,
            description: value.description,
            record_count: value.record_count as u32,
        }
    }
}

impl From<ToolCallSummary> for ToolCallDto {
    fn from(value: ToolCallSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            title: value.title,
            status: value.status,
            is_error: value.is_error,
            result_text: value.result_text,
        }
    }
}

impl From<ConversationSnapshot> for ConversationSnapshotDto {
    fn from(value: ConversationSnapshot) -> Self {
        Self {
            session_id: value.session_id,
            character: value.character.map(CharacterSummaryDto::from),
            participants: value
                .participants
                .into_iter()
                .map(ParticipantDto::from)
                .collect(),
            persona_id: value.persona_id,
            worldbook_ids: value.worldbook_ids,
            prompt_profile_id: value.prompt_profile_id,
            messages: value
                .messages
                .into_iter()
                .map(|message| MessageDto {
                    id: message.id,
                    role: serde_json::to_value(message.role)
                        .ok()
                        .and_then(|role| role.as_str().map(str::to_string))
                        .unwrap_or_else(|| "assistant".to_string()),
                    speaker_id: message.speaker_id,
                    text: message.text,
                    tool_calls: message
                        .tool_calls
                        .into_iter()
                        .map(ToolCallDto::from)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl ProviderConfigDto {
    fn to_domain(&self) -> EndpointConfig {
        let mut extra = serde_json::Map::new();
        if let Some(max_tokens) = self.max_tokens {
            extra.insert("maxTokens".to_string(), max_tokens.into());
        }
        if let Some(temperature) = self.temperature {
            extra.insert("temperature".to_string(), temperature.into());
        }
        if let Some(replays) = self.replays_assistant_reasoning {
            extra.insert(
                sujiu_core::REPLAYS_ASSISTANT_REASONING_KEY.to_string(),
                replays.into(),
            );
        }

        EndpointConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            // A blank model means "not chosen yet", not "the empty model".
            selected_model: self
                .selected_model
                .as_deref()
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string),
            credential_ref: None,
            overrides: extra,
        }
    }
}

impl TurnRequestDto {
    fn to_domain(self) -> SendTurnRequest {
        SendTurnRequest {
            session_id: self.session_id,
            user_text: self.user_text,
            provider: self.provider.map(|provider| provider.to_domain()),
            api_key: self.api_key,
        }
    }
}

impl From<PersonaSummary> for PersonaDto {
    fn from(value: PersonaSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            description: value.description,
            worldbook_count: value.worldbook_count as u32,
        }
    }
}

impl From<WorldBookSummary> for WorldBookDto {
    fn from(value: WorldBookSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            entries: value
                .entries
                .into_iter()
                .map(|entry| WorldBookEntryDto {
                    id: entry.id,
                    name: entry.name,
                    content: entry.content,
                    keys: entry.keys,
                    enabled: entry.enabled,
                    constant: entry.constant,
                    position: enum_label(&entry.position),
                })
                .collect(),
        }
    }
}

impl From<PromptProfileSummary> for PromptProfileDto {
    fn from(value: PromptProfileSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
        }
    }
}

impl From<ParticipantRequestDto> for ParticipantRequest {
    fn from(value: ParticipantRequestDto) -> Self {
        Self {
            character_id: value.character_id,
            role: value.role.as_deref().map(participant_role),
            display_name: value.display_name,
        }
    }
}

impl PersonaRequestDto {
    fn to_domain(self) -> PersonaRequest {
        PersonaRequest {
            id: self.id,
            name: self.name,
            description: self.description,
            user_prompt: self.user_prompt,
            worldbook_ids: self.worldbook_ids,
        }
    }
}

impl WorldBookEntryRequestDto {
    fn to_domain(self) -> WorldBookEntryRequest {
        WorldBookEntryRequest {
            id: self.id,
            name: self.name,
            content: self.content,
            keys: self.keys,
            enabled: self.enabled.unwrap_or(true),
            constant: self.constant.unwrap_or(false),
            position: self
                .position
                .as_deref()
                .map(world_book_position)
                .unwrap_or_default(),
        }
    }
}

impl WorldBookRequestDto {
    fn to_domain(self) -> WorldBookRequest {
        WorldBookRequest {
            id: self.id,
            name: self.name,
            entries: self
                .entries
                .into_iter()
                .map(WorldBookEntryRequestDto::to_domain)
                .collect(),
        }
    }
}

impl PromptProfileRequestDto {
    fn to_domain(self) -> PromptProfileRequest {
        PromptProfileRequest {
            id: self.id,
            name: self.name,
            system_prompt: self.system_prompt,
            user_prompt: self.user_prompt,
            post_history_instructions: self.post_history_instructions,
            format_rules: self.format_rules,
        }
    }
}

impl CharacterRequestDto {
    fn to_domain(self) -> CharacterRequest {
        CharacterRequest {
            id: self.id,
            name: self.name,
            description: self.description,
            personality: self.personality,
            scenario: self.scenario,
            first_message: self.first_message,
            alternate_greetings: self.alternate_greetings,
            example_dialogue: self.example_dialogue,
            system_prompt: self.system_prompt,
            post_history_instructions: self.post_history_instructions,
            worldbook_ids: self.worldbook_ids,
        }
    }
}

/// The bindings a screen set on one conversation.
///
/// Same shape as the request that creates one, because they are the same four
/// questions: who is in this chat, which persona is the user bringing, which
/// world books apply, and which fixed prompt it is spoken with.
#[derive(Clone, Debug, Default)]
pub struct BindingsRequest {
    pub participants: Vec<ParticipantRequest>,
    pub persona_id: Option<String>,
    pub worldbook_ids: Vec<String>,
    pub prompt_profile_id: Option<String>,
}

impl BindingsRequest {
    fn to_domain(self) -> CreateConversationRequest {
        CreateConversationRequest {
            participants: self.participants,
            persona_id: self.persona_id,
            worldbook_ids: self.worldbook_ids,
            prompt_profile_id: self.prompt_profile_id,
        }
    }
}

impl From<Vec<ParticipantRequestDto>> for BindingsRequest {
    fn from(value: Vec<ParticipantRequestDto>) -> Self {
        Self {
            participants: value.into_iter().map(ParticipantRequest::from).collect(),
            ..BindingsRequest::default()
        }
    }
}

impl BindingsRequest {
    fn with_persona(mut self, persona_id: Option<String>) -> Self {
        self.persona_id = persona_id;
        self
    }

    fn with_world_books(mut self, worldbook_ids: Option<Vec<String>>) -> Self {
        self.worldbook_ids = worldbook_ids.unwrap_or_default();
        self
    }

    fn with_prompt_profile(mut self, prompt_profile_id: Option<String>) -> Self {
        self.prompt_profile_id = prompt_profile_id;
        self
    }
}

/// Reports each normalized turn event to the platform bridge as JSON.
struct EventReporter {
    sink: ThreadsafeFunction<String>,
}

impl TurnEventReporter for EventReporter {
    fn report(&mut self, event: TurnEvent) {
        let payload = serde_json::to_string(&event).unwrap_or_else(|_| {
            serde_json::json!({ "kind": "turn_failed", "isError": true }).to_string()
        });
        // A closed platform callback must not take the runtime down.
        let _ = self
            .sink
            .call(Ok(payload), ThreadsafeFunctionCallMode::Blocking);
    }
}

/// Runs one turn off the UI thread and streams events back through a callback.
pub struct SendTurnTask {
    runtime: Arc<SujiuRuntime>,
    request: Option<SendTurnRequest>,
    sink: Option<ThreadsafeFunction<String>>,
}

impl Task for SendTurnTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        let request = self
            .request
            .take()
            .ok_or_else(|| napi::Error::from_reason("turn already started"))?;
        let mut reporter = EventReporter {
            sink: self
                .sink
                .take()
                .ok_or_else(|| napi::Error::from_reason("turn already started"))?,
        };

        self.runtime
            .tokio
            .block_on(self.runtime.send_turn(request, &mut reporter));

        // The turn result is a summary for the caller only; the events already
        // carried the full stream to the platform.
        Ok(serde_json::json!({ "ok": true }).to_string())
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

/// What a settings form gets back about an endpoint that is already configured.
///
/// A free function so the conversion can be tested on its own. The generated
/// binding around it needs a live NAPI runtime and so cannot be reached from a
/// test binary at all, which makes this the part that has to be right by
/// construction rather than by luck.
///
/// Note what is absent: a credential. Every field here becomes a property the
/// form can read, and a form that could read the key back would be one careless
/// log line away from printing it. The address and the chosen model are enough
/// to redraw the screen, and nothing more is offered.
fn stored_endpoint_of(config: Option<EndpointConfig>) -> Option<StoredEndpointDto> {
    config.map(|config| StoredEndpointDto {
        id: config.id.clone(),
        base_url: config.base_url.clone(),
        selected_model: config.selected_model.clone(),
        label: config.display_label(),
    })
}

/// Builds a runtime from the seed catalog that ships with the app.
///
/// This is a free function rather than an associated one on purpose: a factory
/// declared inside `impl SujiuRuntimeBridge` is registered as a static method
/// on the exported class, so ArkTS would have to call
/// `SujiuRuntimeBridge.create()` instead of `create()`.
#[napi]
pub fn create() -> Result<SujiuRuntimeBridge> {
    // The seeded catalog, not Seed::default(): a fresh launch would otherwise
    // have no characters, no history and no context sources to show.
    let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed())
        .map_err(|error| napi::Error::from_reason(format!("runtime init failed: {error}")))?;

    Ok(SujiuRuntimeBridge {
        runtime: Arc::new(runtime),
    })
}

#[napi]
impl SujiuRuntimeBridge {
    /// Attaches a directory the runtime may persist to.
    ///
    /// The platform chooses the location; the runtime decides what goes in it.
    /// Attaching after startup restores a document that is already there, so a
    /// frontend can hand the directory over whenever it learns it.
    #[napi]
    pub fn use_data_directory(&self, directory: String) -> Result<()> {
        self.runtime
            .use_directory(&directory)
            .map_err(napi::Error::from_reason)
    }

    /// The directory currently attached, or null when the runtime is in memory.
    #[napi]
    pub fn data_directory(&self) -> Option<String> {
        self.runtime.directory()
    }

    /// The shared runtime version, so a platform can report what it is bound to.
    #[napi]
    pub fn core_version(&self) -> String {
        sujiu_ffi::CORE_VERSION.to_string()
    }

    #[napi]
    pub fn list_sessions(&self) -> Vec<SessionSummaryDto> {
        self.runtime
            .sessions()
            .into_iter()
            .map(SessionSummaryDto::from)
            .collect()
    }

    #[napi]
    pub fn list_characters(&self, query: Option<String>) -> Vec<CharacterSummaryDto> {
        self.runtime
            .characters(query.as_deref().unwrap_or_default())
            .into_iter()
            .map(CharacterSummaryDto::from)
            .collect()
    }

    #[napi]
    pub fn list_models(&self) -> Vec<ModelSummaryDto> {
        self.runtime
            .models()
            .into_iter()
            .map(ModelSummaryDto::from)
            .collect()
    }

    /// Ask the endpoint which models it serves.
    ///
    /// A convenience for the settings screen. It never decides how a turn
    /// runs, and `manualEntryAllowed` is always true: a gateway with no model
    /// list still chats, so a listing failure must not take the manual choice
    /// away.
    #[napi]
    pub fn discover_models(&self, api_key: Option<String>) -> Result<ModelDiscoveryDto> {
        let discovery = self
            .runtime
            .tokio
            .block_on(self.runtime.discover_models(api_key.as_deref()));
        Ok(ModelDiscoveryDto::from(discovery))
    }

    #[napi]
    pub fn list_context_sources(&self) -> Result<Vec<ContextSourceDto>> {
        let sources = self
            .runtime
            .tokio
            .block_on(async { self.runtime.context_sources().await })
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;

        Ok(sources.into_iter().map(ContextSourceDto::from).collect())
    }

    #[napi]
    pub fn conversation_state(&self, session_id: String) -> Result<ConversationSnapshotDto> {
        self.runtime
            .conversation_state(&session_id)
            .map(ConversationSnapshotDto::from)
            .ok_or_else(|| napi::Error::from_reason(format!("unknown session: {session_id}")))
    }

    #[napi]
    pub fn create_session(&self, character_id: String) -> String {
        self.runtime.create_session(Some(character_id.as_str()))
    }

    /// Open a conversation with any number of characters.
    ///
    /// This is the form that needs no fiction about a main character: an empty
    /// list is a conversation with nobody in it yet, one is the ordinary chat,
    /// and several is a table. The single-character path above is a shortcut
    /// into this one, not a different storage model.
    #[napi]
    pub fn create_conversation(
        &self,
        participants: Vec<ParticipantRequestDto>,
        persona_id: Option<String>,
        worldbook_ids: Option<Vec<String>>,
        prompt_profile_id: Option<String>,
    ) -> String {
        self.runtime
            .create_conversation(&CreateConversationRequest {
                participants: participants
                    .into_iter()
                    .map(|participant| ParticipantRequest {
                        character_id: participant.character_id,
                        role: participant.role.as_deref().map(participant_role),
                        display_name: participant.display_name,
                    })
                    .collect(),
                persona_id,
                worldbook_ids: worldbook_ids.unwrap_or_default(),
                prompt_profile_id,
            })
    }

    /// Every stored persona, as list rows.
    ///
    /// Its own call rather than a column on `list_characters`: a persona is a
    /// separate entity, and keeping it inside a character's shape is exactly what
    /// made it invisible.
    #[napi]
    pub fn list_personas(&self) -> Vec<PersonaDto> {
        self.runtime
            .personas()
            .into_iter()
            .map(PersonaDto::from)
            .collect()
    }

    /// One persona in full, for an editor.
    ///
    /// A different shape from the list row on purpose: an editor needs the
    /// bodies, and a summary would leave it nothing to prefill and nothing to
    /// tell "empty" from "unchanged".
    #[napi]
    pub fn persona(&self, id: String) -> Option<PersonaRequestDto> {
        self.runtime.persona(&id).map(|persona| PersonaRequestDto {
            id: Some(persona.id),
            name: persona.name,
            description: persona.description,
            user_prompt: persona.user_prompt,
            worldbook_ids: persona.worldbook_ids,
        })
    }

    #[napi]
    pub fn save_persona(&self, persona: PersonaRequestDto) -> Result<String> {
        self.runtime
            .save_persona(&persona.to_domain())
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    #[napi]
    pub fn delete_persona(&self, id: String) -> Result<bool> {
        self.runtime
            .delete_persona(&id)
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    /// Every stored world book, each with its entries.
    ///
    /// Entries travel with the book because an entry has no meaning outside the
    /// book holding it: an editor has to have the whole book to save one entry.
    #[napi]
    pub fn list_world_books(&self) -> Vec<WorldBookDto> {
        self.runtime
            .world_books()
            .into_iter()
            .map(WorldBookDto::from)
            .collect()
    }

    #[napi]
    pub fn save_world_book(&self, book: WorldBookRequestDto) -> Result<String> {
        self.runtime
            .save_world_book(&book.to_domain())
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    #[napi]
    pub fn delete_world_book(&self, id: String) -> Result<bool> {
        self.runtime
            .delete_world_book(&id)
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    /// Every stored prompt profile, as list rows.
    ///
    /// The domain calls it a prompt profile because that is what it is; a screen
    /// may show it as "Prompts" and say nothing about the type name.
    #[napi]
    pub fn list_prompt_profiles(&self) -> Vec<PromptProfileDto> {
        self.runtime
            .prompt_profiles()
            .into_iter()
            .map(PromptProfileDto::from)
            .collect()
    }

    /// One prompt profile in full, for an editor.
    #[napi]
    pub fn prompt_profile(&self, id: String) -> Option<PromptProfileRequestDto> {
        self.runtime
            .prompt_profile(&id)
            .map(|profile| PromptProfileRequestDto {
                id: Some(profile.id),
                name: profile.name,
                system_prompt: profile.system_prompt,
                user_prompt: profile.user_prompt,
                post_history_instructions: profile.post_history_instructions,
                format_rules: profile.format_rules,
            })
    }

    #[napi]
    pub fn save_prompt_profile(&self, profile: PromptProfileRequestDto) -> Result<String> {
        self.runtime
            .save_prompt_profile(&profile.to_domain())
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    #[napi]
    pub fn delete_prompt_profile(&self, id: String) -> Result<bool> {
        self.runtime
            .delete_prompt_profile(&id)
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    /// Create or edit a character.
    ///
    /// Exposed because a library has to be editable, not only readable: a user
    /// who cannot correct a character's own words has to delete and recreate it,
    /// which loses every conversation that bound it.
    #[napi]
    pub fn save_character(&self, character: CharacterRequestDto) -> Result<String> {
        self.runtime
            .save_character(&character.to_domain())
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    /// One character in full, for an editor to prefill from.
    ///
    /// `list_characters` answers with summaries, and a summary has no scenario,
    /// no first message and no prompt bodies. An editor prefilled from one starts
    /// blank over a character that is fully written, and saving it would replace
    /// everything with what survived in three fields.
    ///
    /// Resolves to nothing for a blank or unknown id, which is the answer a
    /// "new character" form wants.
    #[napi]
    pub fn character(&self, id: String) -> Option<CharacterRequestDto> {
        let character = self.runtime.character(&id)?;
        Some(CharacterRequestDto {
            id: Some(character.id),
            name: character.name,
            description: character.description,
            personality: character.personality,
            scenario: character.scenario,
            first_message: character.first_message,
            alternate_greetings: character.alternate_greetings,
            example_dialogue: character.example_dialogue,
            system_prompt: character.system_prompt,
            post_history_instructions: character.post_history_instructions,
            worldbook_ids: character.worldbook_ids,
        })
    }

    #[napi]
    pub fn delete_character(&self, id: String) -> Result<bool> {
        self.runtime
            .delete_character(&id)
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    /// Replace what a conversation binds.
    ///
    /// This is the conversation half of the split: it changes which stored
    /// resources one chat *uses* and never edits those resources, so a persona
    /// bound in two conversations stays one persona. The transcript is left
    /// alone — changing who is in a room is not rewriting what has already been
    /// said in it.
    #[napi]
    pub fn set_conversation_bindings(
        &self,
        session_id: String,
        participants: Vec<ParticipantRequestDto>,
        persona_id: Option<String>,
        worldbook_ids: Option<Vec<String>>,
        prompt_profile_id: Option<String>,
    ) -> Result<()> {
        let bindings = BindingsRequest::from(participants)
            .with_persona(persona_id)
            .with_world_books(worldbook_ids)
            .with_prompt_profile(prompt_profile_id);

        self.runtime
            .set_conversation_bindings(&session_id, &bindings.to_domain())
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    /// The wire formats the runtime speaks, in the order it tries them.
    ///
    /// This replaces the list of provider kinds a settings screen used to be
    /// given. A vendor list asked the user to decide something negotiation
    /// settles by asking the endpoint, and a name in that list could be wrong
    /// for the very address the user typed.
    #[napi]
    pub fn supported_protocols(&self) -> Vec<String> {
        SujiuRuntime::supported_protocols()
    }

    /// Store the endpoint a settings screen configured.
    ///
    /// There is no kind to reject: an address the runtime has never heard of
    /// is exactly the self-hosted gateway this is meant to support. Whether it
    /// can be reached, and what it speaks, is what the first turn's
    /// negotiation answers.
    #[napi]
    pub fn configure_provider(&self, provider: ProviderConfigDto) -> Result<()> {
        self.runtime
            .set_endpoint(Some(provider.to_domain()))
            .map_err(|error| napi::Error::from_reason(format!("{error}")))
    }

    /// Read back what is already configured, so a settings form can open on
    /// the truth rather than on three empty fields.
    ///
    /// No credential comes back. The key lives in platform storage and is
    /// never something a form can read into a field and then leak into a log
    /// line or a state dump.
    #[napi]
    pub fn stored_endpoint(&self) -> Option<StoredEndpointDto> {
        stored_endpoint_of(self.runtime.endpoint())
    }

    /// What an endpoint told us before, without asking it again.
    ///
    /// This is what a platform reads at launch so the model list is already
    /// there. It never reaches the network, which is the difference from
    /// `discover_endpoint`: a launch that quietly asked would cost every user a
    /// round trip every time they opened the app, and on a free tier a rate
    /// limit as well.
    #[napi]
    pub fn remembered_endpoint(&self, base_url: String) -> Option<EndpointExplorationDto> {
        self.runtime
            .remembered_endpoint(&base_url)
            .map(EndpointExplorationDto::from)
    }

    /// Find out what an endpoint is, from an address and a key and nothing else.
    ///
    /// This is the order the user actually works in, and it is why the settings
    /// screen can be shown a model list before anything has been saved: there is
    /// no configuration to complete first, and no model to know in advance.
    /// The model list and the protocol are reported as two independent answers,
    /// because a gateway can list nothing and still speak Responses.
    ///
    /// `refresh` is what a platform's "ask again" button passes. Leaving it out
    /// asks the cache, which is what an ordinary open of a settings screen
    /// wants: the endpoint already answered this, and re-asking costs the user a
    /// wait and the endpoint a rate limit.
    #[napi]
    pub fn discover_endpoint(
        &self,
        base_url: String,
        api_key: String,
        refresh: Option<bool>,
    ) -> Result<EndpointExplorationDto> {
        let exploration = self.runtime.tokio.block_on(self.runtime.discover_endpoint(
            &base_url,
            &api_key,
            refresh.unwrap_or(false),
        ));
        Ok(EndpointExplorationDto::from(exploration))
    }

    /// Sends one turn. The callback receives normalized turn events as JSON.
    #[napi]
    pub fn send_turn(
        &self,
        request: TurnRequestDto,
        on_event: ThreadsafeFunction<String>,
    ) -> AsyncTask<SendTurnTask> {
        AsyncTask::new(SendTurnTask {
            runtime: Arc::clone(&self.runtime),
            request: Some(request.to_domain()),
            sink: Some(on_event),
        })
    }

    #[napi]
    pub fn cancel_turn(&self) {
        self.runtime.cancel();
    }

    /// The diagnostic log, oldest entry first.
    ///
    /// This is what makes a failure debuggable instead of guessable: it names the
    /// endpoint, every protocol that was tried, what each one answered, whether
    /// the model list worked, which protocol and model the turn actually used,
    /// and which stage a failure happened in. `limit` is a number of most
    /// recent entries; zero means everything the log still holds.
    #[napi]
    pub fn diagnostics(&self, limit: Option<u32>) -> Vec<DiagnosticEntryDto> {
        self.runtime
            .diagnostics(limit.unwrap_or(0) as usize)
            .iter()
            .map(DiagnosticEntryDto::from)
            .collect()
    }

    /// Only the capability and model probing half of the log.
    ///
    /// Someone deciding which key to type wants the endpoint's behaviour, not
    /// the last conversation's traffic.
    #[napi]
    pub fn discovery_diagnostics(&self, limit: Option<u32>) -> Vec<DiagnosticEntryDto> {
        self.runtime
            .discovery_diagnostics(limit.unwrap_or(0) as usize)
            .iter()
            .map(DiagnosticEntryDto::from)
            .collect()
    }

    /// The whole log as text, for the "copy this into a bug report" case.
    #[napi]
    pub fn diagnostics_text(&self) -> String {
        self.runtime.diagnostics_text()
    }

    /// Start a fresh investigation. The stored endpoint and key are untouched:
    /// the log is evidence, not configuration.
    #[napi]
    pub fn clear_diagnostics(&self) {
        self.runtime.clear_diagnostics();
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderConfigDto;
    use super::{participant_role, CreateConversationRequest, ParticipantRequest};
    use sujiu_core::{apply_reasoning_override, EndpointCapabilities, Protocol};
    use sujiu_ffi::events::TurnEventReporter;

    fn empty_with(prototype: &CreateConversationRequest) -> CreateConversationRequest {
        CreateConversationRequest {
            participants: Vec::new(),
            persona_id: prototype.persona_id.clone(),
            worldbook_ids: Vec::new(),
            prompt_profile_id: prototype.prompt_profile_id.clone(),
        }
    }

    fn form_fields(replays_assistant_reasoning: Option<bool>) -> ProviderConfigDto {
        ProviderConfigDto {
            id: "provider-1".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            selected_model: Some("deepseek-reasoner".into()),
            max_tokens: Some(2048),
            temperature: Some(0.7),
            replays_assistant_reasoning,
        }
    }

    /// A platform only describes the endpoint: an id, a URL, a model and a
    /// key. It does not declare which wire protocol the endpoint speaks, and it
    /// must not have to, because that answer goes stale and a form cannot know
    /// it. So the settings form's fields convert to a config that asks the
    /// endpoint rather than asserting anything about it.
    #[test]
    fn a_settings_form_does_not_have_to_declare_any_protocol_capability() {
        let config = form_fields(None).to_domain();

        assert_eq!(config.base_url, "https://api.deepseek.com/v1");
        assert_eq!(config.selected_model.as_deref(), Some("deepseek-reasoner"));
        assert_eq!(
            config.forced_reasoning_replay(),
            None,
            "the form stated nothing, so nothing may be inferred from a vendor or model name"
        );
        assert_eq!(
            config.protocols(),
            Protocol::PRIORITY.to_vec(),
            "there is nothing for a form to choose and nothing to derive from a name"
        );
    }

    /// The model may be left unchosen, because exploration happens before
    /// anyone has picked one. A form that cannot express "not yet" would force
    /// the user to know the answer before the question could be asked.
    #[test]
    fn an_endpoint_crosses_the_bridge_before_a_model_is_chosen() {
        let config = ProviderConfigDto {
            selected_model: None,
            ..form_fields(None)
        }
        .to_domain();

        assert!(
            config.selected_model.is_none(),
            "no model chosen yet is a real state, not a missing field"
        );
        assert!(config.protocols().contains(&Protocol::OpenAiResponses));
    }

    /// A blank model means the same thing, rather than becoming a request for
    /// a model whose name is nothing.
    #[test]
    fn a_blank_model_is_treated_as_unchosen_rather_than_as_a_name() {
        let config = ProviderConfigDto {
            selected_model: Some("   ".into()),
            ..form_fields(None)
        }
        .to_domain();

        assert!(config.selected_model.is_none());
    }

    /// The same fields, whatever model name they carry, lead to the same
    /// question. This is the behaviour the old vendor-name lookup had and the
    /// negotiation replaced: a gateway serving any number of models is still
    /// one endpoint, and a model name is a request parameter.
    #[test]
    fn no_model_name_decides_what_the_endpoint_speaks() {
        for model in ["deepseek-reasoner", "r1", "thinking-v2", "gpt-4o-mini"] {
            let dto = ProviderConfigDto {
                selected_model: Some(model.into()),
                ..form_fields(None)
            };
            let config = dto.to_domain();

            assert_eq!(config.forced_reasoning_replay(), None, "{model}");
            assert_eq!(
                config.capability_key(),
                "provider-1|https://api.deepseek.com/v1",
                "{model} is a parameter, not an endpoint"
            );
        }
    }

    /// The display name a user typed is a label and stays one.
    #[test]
    fn a_display_name_is_never_part_of_how_the_endpoint_is_asked() {
        let named = ProviderConfigDto {
            name: "My own gateway".into(),
            ..form_fields(None)
        }
        .to_domain();
        let unnamed = ProviderConfigDto {
            name: "  ".into(),
            ..form_fields(None)
        }
        .to_domain();

        assert_eq!(named.display_label(), "My own gateway");
        assert_ne!(
            named.display_label(),
            unnamed.display_label(),
            "an unnamed endpoint still shows something a person can recognise"
        );
        assert_eq!(
            named.capability_key(),
            unnamed.capability_key(),
            "what the two are asked is decided by where they point, not what they are called"
        );
    }

    /// A platform that does know something the negotiation cannot see may still
    /// say so, and saying it has to survive the conversion in both directions.
    #[test]
    fn a_stated_capability_survives_the_bridge_in_both_directions() {
        let on = form_fields(Some(true)).to_domain();
        assert_eq!(on.forced_reasoning_replay(), Some(true));

        let off = form_fields(Some(false)).to_domain();
        assert_eq!(off.forced_reasoning_replay(), Some(false));
    }

    /// A stated capability is applied on top of what the endpoint was found to
    /// do, and it only ever changes the reasoning answer.
    #[test]
    fn a_stated_capability_is_applied_on_top_of_a_negotiated_one() {
        let negotiated = EndpointCapabilities::negotiate(&[Protocol::OpenAiChatCompletions])
            .expect("chat completions is implemented");

        let forced = apply_reasoning_override(negotiated.clone(), Some(true));

        assert!(forced.replays_assistant_reasoning);
        assert_eq!(forced.protocol, negotiated.protocol);
        assert_eq!(forced.supported, negotiated.supported);
    }

    /// A settings form has to be able to redraw itself on what is configured.
    ///
    /// This drives the round trip a platform actually performs: a DTO goes in,
    /// the runtime stores it, and a DTO comes back out for the form. The
    /// generated NAPI binding around those two calls needs a live NAPI runtime
    /// and cannot be linked into a test binary at all, so the two mappings on
    /// either side of the runtime are driven directly. Those are the only parts
    /// that can quietly go wrong -- a dropped field, a model arriving as an
    /// empty string instead of absent, a credential appearing by accident.
    #[test]
    fn a_settings_form_can_open_on_what_is_already_configured() {
        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");

        assert!(
            super::stored_endpoint_of(runtime.endpoint()).is_none(),
            "nothing is configured until something is"
        );

        // An address on its own is still an endpoint: a model is chosen after
        // discovery, not before it.
        runtime
            .set_endpoint(Some(
                ProviderConfigDto {
                    id: "endpoint-default".into(),
                    name: "Gateway".into(),
                    base_url: "https://gateway.example/v1".into(),
                    selected_model: None,
                    max_tokens: None,
                    temperature: None,
                    replays_assistant_reasoning: None,
                }
                .to_domain(),
            ))
            .expect("an endpoint without a model");

        let chosen = super::stored_endpoint_of(runtime.endpoint()).expect("it was just stored");
        assert_eq!(chosen.base_url, "https://gateway.example/v1");
        assert_eq!(
            chosen.selected_model, None,
            "a model that was never chosen is absent, not an empty string"
        );

        runtime
            .set_endpoint(Some(
                ProviderConfigDto {
                    id: "endpoint-default".into(),
                    name: "Gateway".into(),
                    base_url: "https://gateway.example/v1".into(),
                    selected_model: Some("picked-model".into()),
                    max_tokens: None,
                    temperature: None,
                    replays_assistant_reasoning: None,
                }
                .to_domain(),
            ))
            .expect("a chosen model");

        let chosen = super::stored_endpoint_of(runtime.endpoint()).expect("it is still stored");
        assert_eq!(chosen.base_url, "https://gateway.example/v1");
        assert_eq!(chosen.selected_model.as_deref(), Some("picked-model"));
        assert_eq!(chosen.label, "Gateway");
    }

    /// The log is only useful if a platform can render it, and it is only safe if
    /// a platform can never be the thing that leaks the key. The redaction has
    /// already happened by the time a DTO exists, so what crosses is a finished
    /// line and there is no second pass to forget.
    ///
    /// The NAPI methods themselves need a live runtime binding and cannot run in
    /// a test binary, so this drives the mapping and the log the mapping reads.
    #[test]
    fn a_diagnostic_line_crosses_the_bridge_ready_to_show_and_without_the_key() {
        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        let key = "sk-sujiu-bridge-1234567890abcdef";

        runtime.tokio.block_on(runtime.discover_endpoint(
            "https://sujiu-does-not-resolve.invalid/v1".into(),
            key.into(),
            true,
        ));

        let entries: Vec<super::DiagnosticEntryDto> = runtime
            .diagnostics(0)
            .iter()
            .map(super::DiagnosticEntryDto::from)
            .collect();

        assert!(!entries.is_empty(), "the probe has to have said something");

        for entry in &entries {
            assert!(
                !entry.message.contains(key) && !entry.detail.contains(key),
                "a credential reached the platform: {}",
                entry.detail
            );
            assert!(
                !entry.stage.is_empty(),
                "a line with no stage cannot be read"
            );
            assert!(entry.at_ms > 0.0, "a line with no time cannot be ordered");
        }

        let requested = entries
            .iter()
            .find(|entry| entry.stage == "discovery_request")
            .expect("the request that started it all");
        assert_eq!(requested.kind, "discovery");
        assert!(
            requested.detail.contains("sujiu-does-not-resolve.invalid"),
            "the line has to name the endpoint, or it is not diagnosable: {}",
            requested.detail
        );
        assert!(
            !requested.message.contains("sk-sujiu-bridge"),
            "the masked form must not be the raw one"
        );
    }

    /// Asking for one half of the log has to return one half. A settings screen
    /// showing the endpoint's behaviour should not have the last conversation
    /// interleaved into it, or the relevant line is impossible to pick out.
    #[test]
    fn a_platform_reading_only_the_discovery_half_gets_only_the_discovery_half() {
        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        runtime
            .set_endpoint(Some(
                ProviderConfigDto {
                    id: "endpoint-default".into(),
                    name: "Gateway".into(),
                    base_url: "https://gateway.example/v1".into(),
                    selected_model: Some("picked-model".into()),
                    max_tokens: None,
                    temperature: None,
                    replays_assistant_reasoning: None,
                }
                .to_domain(),
            ))
            .expect("provider");

        let mut reporter = sujiu_ffi::runtime::CollectingReporter::default();
        runtime.tokio.block_on(runtime.send_turn(
            sujiu_ffi::runtime::SendTurnRequest {
                session_id: runtime.sessions()[0].id.clone(),
                user_text: "hello".into(),
                provider: None,
                api_key: Some("sk-a-key-1234567890abcdef".into()),
            },
            &mut reporter,
        ));

        let everything: Vec<super::DiagnosticEntryDto> = runtime
            .diagnostics(0)
            .iter()
            .map(super::DiagnosticEntryDto::from)
            .collect();
        let discovery: Vec<super::DiagnosticEntryDto> = runtime
            .discovery_diagnostics(0)
            .iter()
            .map(super::DiagnosticEntryDto::from)
            .collect();

        assert!(
            everything.len() > discovery.len(),
            "the turn produced lines the discovery filter should have removed"
        );
        assert!(
            discovery.iter().all(|entry| entry.kind == "discovery"),
            "the filtered read leaked the conversation half"
        );
    }

    /// A screen reads a conversation as a list of participants, and a session
    /// summary still answers the one-character question a simple list has.
    ///
    /// The wire keeps `characterId` because removing it would break every
    /// platform list row at once, and a convenience field is cheap. The list is
    /// the truth, and a conversation with three of them has to be expressible
    /// through the same DTO.
    #[test]
    fn a_conversation_reports_every_participant_and_still_answers_the_simple_question() {
        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");

        let table: Vec<super::SessionSummaryDto> = runtime
            .sessions()
            .into_iter()
            .map(super::SessionSummaryDto::from)
            .collect();
        let roundtable = table
            .iter()
            .find(|summary| summary.participants.len() == 2)
            .expect("the seed includes a conversation with two characters in it");
        assert_eq!(
            roundtable.participants[0].role, "narrator",
            "a narrator seat is reported as what it is"
        );
        assert_eq!(roundtable.participants[1].role, "character");
        assert_eq!(
            roundtable.character_id, roundtable.participants[0].character_id,
            "the convenience field is the first participant, not a main character"
        );
        assert_eq!(roundtable.character_name, "Shen");

        let snapshot = super::ConversationSnapshotDto::from(
            runtime
                .conversation_state(&roundtable.id)
                .expect("the conversation is readable"),
        );
        assert_eq!(snapshot.participants.len(), 2);
        assert_eq!(snapshot.session_id, roundtable.id);
    }

    /// A screen may hand over any number of characters, and the DTO it hands
    /// them over in loses nothing on the way to the runtime.
    ///
    /// The bridge's own `create_conversation` cannot run in a test binary, so
    /// what is proved here is the translation: the DTOs a screen fills in become
    /// the request the runtime receives, and an unstated role stays an ordinary
    /// speaking character instead of becoming an error.
    #[test]
    fn a_screen_can_open_a_conversation_with_nobody_one_or_many() {
        let empty = CreateConversationRequest {
            participants: Vec::new(),
            persona_id: None,
            worldbook_ids: Vec::new(),
            prompt_profile_id: None,
        };
        let one = CreateConversationRequest {
            participants: vec![ParticipantRequest {
                character_id: "character-lin".into(),
                role: None,
                display_name: None,
            }],
            ..empty_with(&empty)
        };
        let table = CreateConversationRequest {
            participants: vec![
                ParticipantRequest {
                    character_id: "character-shen".into(),
                    role: Some(participant_role("narrator")),
                    display_name: None,
                },
                ParticipantRequest {
                    character_id: "character-wen".into(),
                    // A label this build does not know must not fail the call.
                    role: Some(participant_role("gameMaster")),
                    display_name: None,
                },
            ],
            persona_id: Some("persona-insomniac".into()),
            worldbook_ids: vec!["world-book-coast".into()],
            prompt_profile_id: None,
        };

        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        let empty_id = runtime.create_conversation(&empty);
        let one_id = runtime.create_conversation(&one);
        let table_id = runtime.create_conversation(&table);

        let state = |id: &str| {
            super::ConversationSnapshotDto::from(runtime.conversation_state(id).expect("readable"))
        };
        assert!(state(&empty_id).participants.is_empty());
        assert_eq!(state(&one_id).participants[0].role, "character");

        let table = state(&table_id);
        assert_eq!(table.participants.len(), 2);
        assert_eq!(table.participants[0].role, "narrator");
        assert_eq!(table.participants[1].name, "Wen");
        assert_eq!(table.persona_id.as_deref(), Some("persona-insomniac"));
        assert_eq!(table.worldbook_ids, vec!["world-book-coast"]);
    }

    /// The four resources have to be reachable as four things rather than as one
    /// catalog with a kind column. A bridge that could only hand back
    /// "characters" is what pushed personas into a character card to begin with,
    /// so the list shapes themselves are the thing under test.
    ///
    /// These exercise the translation rather than the runtime calls: this crate's
    /// test binary cannot link a `napi::Error`, which only exists inside Node.
    /// The runtime behaviour behind them is covered in `sujiu-ffi`.
    #[test]
    fn each_resource_kind_reaches_a_screen_as_its_own_shape() {
        use super::{
            PersonaDto, PersonaSummary, PromptProfileDto, PromptProfileSummary, WorldBookDto,
            WorldBookSummary,
        };

        let persona = PersonaDto::from(PersonaSummary {
            id: "persona-1".into(),
            name: "The insomniac".into(),
            description: "A night-shift listener.".into(),
            worldbook_count: 2,
        });
        assert_eq!(persona.id, "persona-1");
        assert_eq!(
            persona.worldbook_count, 2,
            "a persona's own world books are counted, not listed as if a chat had bound them"
        );

        let book = WorldBookDto::from(WorldBookSummary {
            id: "worldbook-1".into(),
            name: "The northern coast".into(),
            entries: vec![sujiu_ffi::library::WorldBookEntrySummary {
                id: "entry-1".into(),
                name: "Tone".into(),
                content: "The coast is cold.".into(),
                keys: vec!["coast".into()],
                enabled: true,
                constant: true,
                position: sujiu_core::WorldBookPosition::NearHistory,
            }],
        });
        assert_eq!(book.entries[0].position, "near_history");
        assert_eq!(book.entries[0].keys, vec!["coast".to_string()]);

        let profile = PromptProfileDto::from(PromptProfileSummary {
            id: "promptprofile-1".into(),
            name: "Roleplay".into(),
        });
        assert_eq!(profile.name, "Roleplay");
    }

    /// The row and the editor are deliberately two different shapes.
    ///
    /// A list of profiles is a list of names. A body carried on the row would make
    /// the list a wall of text and would leave a screen with no reason to ask for
    /// the profile itself — and a screen that read the row instead would never see
    /// the difference between a cleared prompt and one that was never set.
    #[test]
    fn the_prompt_profile_row_and_the_editor_are_different_shapes() {
        use super::{PromptProfileDto, PromptProfileRequestDto, PromptProfileSummary};

        // Destructured exhaustively: a field added to the row fails to compile
        // here, which is the point of keeping the row this small.
        let PromptProfileDto { id, name } = PromptProfileDto::from(PromptProfileSummary {
            id: "promptprofile-1".into(),
            name: "Roleplay".into(),
        });
        assert_eq!(id, "promptprofile-1");
        assert_eq!(name, "Roleplay");

        // The editor carries the four bodies and the id, and nothing else.
        let editor = PromptProfileRequestDto {
            id: Some(id.clone()),
            name: name.clone(),
            system_prompt: String::new(),
            user_prompt: String::new(),
            post_history_instructions: String::new(),
            format_rules: "One paragraph.".into(),
        };
        assert_eq!(editor.format_rules, "One paragraph.");
        assert!(editor.system_prompt.is_empty());
    }

    /// An entry's position has to survive a round trip through the wire.
    ///
    /// A position that is wrong reorders part of the prompt block the provider is
    /// meant to cache, and neither side can see the other's mistake.
    #[test]
    fn a_world_book_position_round_trips_through_its_label() {
        use super::{world_book_position, WorldBookPosition};
        for position in [
            WorldBookPosition::BeforeCharacter,
            WorldBookPosition::AfterCharacter,
            WorldBookPosition::NearHistory,
        ] {
            let label = super::enum_label(&position);
            assert_eq!(world_book_position(&label), position, "{label}");
        }
    }

    /// A screen that cannot express a position still has to be able to say "where
    /// it normally goes", and an unknown label must not become a different
    /// position.
    #[test]
    fn an_unrecognised_world_book_position_falls_back_to_the_domain_default() {
        use super::{world_book_position, WorldBookPosition};
        assert_eq!(
            world_book_position("somewhere else"),
            WorldBookPosition::AfterCharacter
        );
        assert_eq!(
            world_book_position("beforeCharacter"),
            WorldBookPosition::BeforeCharacter
        );
    }

    /// A screen creates with an absent id and edits with a real one, and the
    /// runtime is what tells those two apart.
    #[test]
    fn an_absent_id_says_create_and_a_present_id_says_edit() {
        use super::{PersonaRequestDto, WorldBookEntryRequestDto};

        assert_eq!(
            PersonaRequestDto {
                id: None,
                name: "New".into(),
                description: String::new(),
                user_prompt: String::new(),
                worldbook_ids: Vec::new(),
            }
            .to_domain()
            .id,
            None
        );

        assert_eq!(
            PersonaRequestDto {
                id: Some("persona-1".into()),
                name: "Existing".into(),
                description: String::new(),
                user_prompt: String::new(),
                worldbook_ids: Vec::new(),
            }
            .to_domain()
            .id
            .as_deref(),
            Some("persona-1")
        );

        // An entry the screen never named is created enabled, and one it says is
        // disabled stays disabled: absent means "not stated", not "off".
        assert!(
            WorldBookEntryRequestDto {
                id: None,
                name: String::new(),
                content: "Lore.".into(),
                keys: Vec::new(),
                enabled: None,
                constant: None,
                position: None,
            }
            .to_domain()
            .enabled
        );
        assert!(
            !WorldBookEntryRequestDto {
                id: Some("entry-1".into()),
                name: String::new(),
                content: "Lore.".into(),
                keys: Vec::new(),
                enabled: Some(false),
                constant: None,
                position: None,
            }
            .to_domain()
            .enabled
        );
    }

    /// Bindings are four separate questions, and none of them may be lost or
    /// invented on the way in.
    #[test]
    fn bindings_arrive_as_the_four_answers_they_are() {
        use super::{BindingsRequest, ParticipantRequestDto};

        let request = BindingsRequest::from(vec![ParticipantRequestDto {
            character_id: "character-wen".into(),
            role: Some("narrator".into()),
            display_name: None,
        }])
        .with_persona(Some("persona-1".into()))
        .with_world_books(Some(vec!["worldbook-1".into()]))
        .with_prompt_profile(Some("promptprofile-1".into()))
        .to_domain();

        assert_eq!(request.participants.len(), 1);
        assert_eq!(request.participants[0].character_id, "character-wen");
        assert_eq!(
            request.participants[0].role,
            Some(sujiu_core::ParticipantRole::Narrator)
        );
        assert_eq!(request.persona_id.as_deref(), Some("persona-1"));
        assert_eq!(request.worldbook_ids, vec!["worldbook-1"]);
        assert_eq!(
            request.prompt_profile_id.as_deref(),
            Some("promptprofile-1")
        );

        // Absent lists mean "none", not "leave alone". Clearing a binding has to be
        // expressible, or a chat could never be un-bound from anything.
        let cleared = BindingsRequest::from(Vec::new())
            .with_world_books(None)
            .to_domain();
        assert!(cleared.participants.is_empty());
        assert!(cleared.worldbook_ids.is_empty());
    }
}
