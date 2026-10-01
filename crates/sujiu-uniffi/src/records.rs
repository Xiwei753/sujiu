//! The app-facing records this crate exports.
//!
//! These are the only shapes Android receives, and they are written once here
//! rather than copied into Kotlin. UniFFI turns each of them into a Kotlin
//! `data class`, so a field added in Rust appears in Kotlin on the next
//! generation instead of on the day somebody remembered to edit a second file.
//!
//! Two rules shaped this module:
//!
//! - **No provider wire formats.** Everything below is a Sujiu domain model or a
//!   Sujiu decision. An `EndpointRecord` names an address and a model; it never
//!   carries an OpenAI or Anthropic request body, and no field here decides
//!   anything a provider adapter owns.
//! - **A list row and an editor body are different records.** `CharacterCard`
//!   and `CharacterDraftRecord` describe the same entity at two different
//!   fidelities, deliberately. An editor prefilled from a summary row opens a
//!   blank form over a fully-written character, and saving then destroys every
//!   prompt the row never carried. The two shapes are kept apart so that
//!   mistake cannot be made by accident.

use std::collections::HashMap;

use sujiu_ai::diagnostics::DiagnosticEntry;
use sujiu_core::{EndpointConfig, WorldBookPosition};
use sujiu_runtime::events::TurnEventKind as RuntimeTurnEventKind;
use sujiu_runtime::library::{
    CharacterRequest, PersonaRequest, PromptProfileRequest, WorldBookEntryRequest, WorldBookRequest,
};
use sujiu_runtime::runtime::{
    CharacterSummary, ContextSourceSummary, ConversationSnapshot, ConversationSummary,
    EndpointExploration, MessageSummary, ModelDiscovery, ModelSummary, ParticipantRequest,
    ParticipantSummary, SendTurnRequest, ToolCallSummary,
};

/// What a participant is in the conversation.
///
/// The runtime stores a role per participant, and a platform that cannot tell a
/// character from a narrator will render both as the same thing.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum ParticipantRole {
    Character,
    Narrator,
}

impl From<ParticipantRole> for sujiu_core::ParticipantRole {
    fn from(role: ParticipantRole) -> Self {
        match role {
            ParticipantRole::Character => Self::Character,
            ParticipantRole::Narrator => Self::Narrator,
        }
    }
}

impl From<sujiu_core::ParticipantRole> for ParticipantRole {
    fn from(role: sujiu_core::ParticipantRole) -> Self {
        match role {
            sujiu_core::ParticipantRole::Character => Self::Character,
            sujiu_core::ParticipantRole::Narrator => Self::Narrator,
        }
    }
}

/// Who authored a transcript step.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum MessageRole {
    System,
    Developer,
    User,
    Assistant,
}

impl From<sujiu_core::ChatRole> for MessageRole {
    fn from(role: sujiu_core::ChatRole) -> Self {
        match role {
            sujiu_core::ChatRole::System => Self::System,
            sujiu_core::ChatRole::Developer => Self::Developer,
            sujiu_core::ChatRole::User => Self::User,
            sujiu_core::ChatRole::Assistant => Self::Assistant,
        }
    }
}

/// What kind of readable information a context source holds.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum ContextKind {
    WorldLore,
    StoryEvent,
    CharacterMemory,
    ChatHistory,
    Persona,
    Note,
    Other,
}

impl From<sujiu_core::ContextKind> for ContextKind {
    fn from(kind: sujiu_core::ContextKind) -> Self {
        match kind {
            sujiu_core::ContextKind::WorldLore => Self::WorldLore,
            sujiu_core::ContextKind::StoryEvent => Self::StoryEvent,
            sujiu_core::ContextKind::CharacterMemory => Self::CharacterMemory,
            sujiu_core::ContextKind::ChatHistory => Self::ChatHistory,
            sujiu_core::ContextKind::Persona => Self::Persona,
            sujiu_core::ContextKind::Note => Self::Note,
            sujiu_core::ContextKind::Other => Self::Other,
        }
    }
}

/// Which side of the prompt a world book entry is injected on.
#[derive(Clone, Copy, Debug, Default, uniffi::Enum)]
pub enum WorldBookPlacement {
    BeforeCharacter,
    #[default]
    AfterCharacter,
    NearHistory,
}

impl From<WorldBookPosition> for WorldBookPlacement {
    fn from(position: WorldBookPosition) -> Self {
        match position {
            WorldBookPosition::BeforeCharacter => Self::BeforeCharacter,
            WorldBookPosition::AfterCharacter => Self::AfterCharacter,
            WorldBookPosition::NearHistory => Self::NearHistory,
        }
    }
}

impl From<WorldBookPlacement> for WorldBookPosition {
    fn from(placement: WorldBookPlacement) -> Self {
        match placement {
            WorldBookPlacement::BeforeCharacter => WorldBookPosition::BeforeCharacter,
            WorldBookPlacement::AfterCharacter => WorldBookPosition::AfterCharacter,
            WorldBookPlacement::NearHistory => WorldBookPosition::NearHistory,
        }
    }
}

/// Why a diagnostics line is in the log.
///
/// The storage case is separate from discovery and chat on purpose: a turn that
/// worked and a store that failed look identical from inside the app, and only
/// this log says which happened.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum DiagnosticKind {
    Discovery,
    Chat,
    Storage,
}

impl From<sujiu_ai::DiagnosticKind> for DiagnosticKind {
    fn from(kind: sujiu_ai::DiagnosticKind) -> Self {
        match kind {
            sujiu_ai::DiagnosticKind::Discovery => Self::Discovery,
            sujiu_ai::DiagnosticKind::Chat => Self::Chat,
            sujiu_ai::DiagnosticKind::Storage => Self::Storage,
        }
    }
}

/// One normalized step of a turn, as the runtime reports it.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum TurnEventKind {
    TurnStarted,
    TextDelta,
    ThinkingDelta,
    ToolCallRequested,
    ToolCallStarted,
    ToolCallFinished,
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
}

impl From<RuntimeTurnEventKind> for TurnEventKind {
    fn from(kind: RuntimeTurnEventKind) -> Self {
        use RuntimeTurnEventKind as R;
        match kind {
            R::TurnStarted => Self::TurnStarted,
            R::TextDelta => Self::TextDelta,
            R::ThinkingDelta => Self::ThinkingDelta,
            R::ToolCallRequested => Self::ToolCallRequested,
            R::ToolCallStarted => Self::ToolCallStarted,
            R::ToolCallFinished => Self::ToolCallFinished,
            R::TurnCompleted => Self::TurnCompleted,
            R::TurnFailed => Self::TurnFailed,
            R::TurnCancelled => Self::TurnCancelled,
        }
    }
}

/// Everyone in a conversation, as a list row or snapshot sees them.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ParticipantRecord {
    pub character_id: String,
    /// The character's own name, or the title the conversation gave them.
    pub name: String,
    pub role: ParticipantRole,
}

impl From<ParticipantSummary> for ParticipantRecord {
    fn from(summary: ParticipantSummary) -> Self {
        // The runtime sends the role as a wire string so a platform can add a
        // role without a binary rebuild. Mapping it here keeps that string an
        // implementation detail: an unknown role is reported as a character,
        // which is what the runtime has always done.
        let role = if summary.role == "narrator" {
            ParticipantRole::Narrator
        } else {
            ParticipantRole::Character
        };
        Self {
            character_id: summary.character_id,
            name: summary.name,
            role,
        }
    }
}

/// A conversation, as a history row sees it.
///
/// `character_id` and `character_name` are the first participant, derived for
/// convenience. A conversation with three participants has no primary one, and a
/// screen that needs to know that has to read `participants`.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ConversationRecord {
    pub id: String,
    pub participants: Vec<ParticipantRecord>,
    pub character_id: Option<String>,
    pub character_name: String,
    pub preview: String,
    pub title: String,
    pub updated_at_ms: i64,
    pub message_count: u32,
}

impl From<ConversationSummary> for ConversationRecord {
    fn from(summary: ConversationSummary) -> Self {
        Self {
            id: summary.id,
            participants: summary.participants.into_iter().map(Into::into).collect(),
            character_id: summary.character_id,
            character_name: summary.character_name,
            preview: summary.preview,
            title: summary.title,
            updated_at_ms: summary.updated_at_ms,
            message_count: summary.message_count as u32,
        }
    }
}

/// One character, at list-row fidelity.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CharacterCard {
    pub id: String,
    pub name: String,
    pub description: String,
}

impl From<CharacterSummary> for CharacterCard {
    fn from(summary: CharacterSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
            description: summary.description,
        }
    }
}

/// One character in full, for an editor to prefill from.
///
/// This is a different shape from [`CharacterCard`] on purpose; see the module
/// comment. A blank `id` means "create", and a non-blank one that the runtime
/// does not know is refused rather than treated as a create, because writing it
/// would silently produce a second copy of a character the user meant to edit.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct CharacterDraftRecord {
    pub id: String,
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

impl From<&sujiu_core::Character> for CharacterDraftRecord {
    fn from(character: &sujiu_core::Character) -> Self {
        Self {
            id: character.id.clone(),
            name: character.name.clone(),
            description: character.description.clone(),
            personality: character.personality.clone(),
            scenario: character.scenario.clone(),
            first_message: character.first_message.clone(),
            alternate_greetings: character.alternate_greetings.clone(),
            example_dialogue: character.example_dialogue.clone(),
            system_prompt: character.system_prompt.clone(),
            post_history_instructions: character.post_history_instructions.clone(),
            worldbook_ids: character.worldbook_ids.clone(),
        }
    }
}

impl From<CharacterDraftRecord> for CharacterRequest {
    fn from(record: CharacterDraftRecord) -> Self {
        Self {
            id: Some(record.id).filter(|id| !id.is_empty()),
            name: record.name,
            description: record.description,
            personality: record.personality,
            scenario: record.scenario,
            first_message: record.first_message,
            alternate_greetings: record.alternate_greetings,
            example_dialogue: record.example_dialogue,
            system_prompt: record.system_prompt,
            post_history_instructions: record.post_history_instructions,
            worldbook_ids: record.worldbook_ids,
        }
    }
}

/// One persona, at list-row fidelity.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PersonaCard {
    pub id: String,
    pub name: String,
    pub description: String,
    pub worldbook_count: u32,
}

impl From<sujiu_runtime::library::PersonaSummary> for PersonaCard {
    fn from(summary: sujiu_runtime::library::PersonaSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
            description: summary.description,
            worldbook_count: summary.worldbook_count as u32,
        }
    }
}

/// One persona in full, for an editor to prefill from.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct PersonaDraftRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub user_prompt: String,
    pub worldbook_ids: Vec<String>,
}

impl From<&sujiu_core::Persona> for PersonaDraftRecord {
    fn from(persona: &sujiu_core::Persona) -> Self {
        Self {
            id: persona.id.clone(),
            name: persona.name.clone(),
            description: persona.description.clone(),
            user_prompt: persona.user_prompt.clone(),
            worldbook_ids: persona.worldbook_ids.clone(),
        }
    }
}

impl From<PersonaDraftRecord> for PersonaRequest {
    fn from(record: PersonaDraftRecord) -> Self {
        Self {
            id: Some(record.id).filter(|id| !id.is_empty()),
            name: record.name,
            description: record.description,
            user_prompt: record.user_prompt,
            worldbook_ids: record.worldbook_ids,
        }
    }
}

/// One entry inside a world book.
#[derive(Clone, Debug, uniffi::Record)]
pub struct WorldBookEntryRecord {
    pub id: String,
    pub name: String,
    pub content: String,
    pub keys: Vec<String>,
    pub enabled: bool,
    pub constant: bool,
    pub placement: WorldBookPlacement,
}

impl From<&sujiu_core::WorldBookEntry> for WorldBookEntryRecord {
    fn from(entry: &sujiu_core::WorldBookEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.name.clone(),
            content: entry.content.clone(),
            keys: entry.keys.clone(),
            enabled: entry.enabled,
            constant: entry.constant,
            placement: entry.position.into(),
        }
    }
}

impl From<WorldBookEntryRecord> for WorldBookEntryRequest {
    fn from(record: WorldBookEntryRecord) -> Self {
        Self {
            id: Some(record.id).filter(|id| !id.is_empty()),
            name: record.name,
            content: record.content,
            keys: record.keys,
            enabled: record.enabled,
            constant: record.constant,
            position: record.placement.into(),
        }
    }
}

/// One world book, at list-row fidelity, with its entries inlined.
#[derive(Clone, Debug, uniffi::Record)]
pub struct WorldBookCard {
    pub id: String,
    pub name: String,
    pub entries: Vec<WorldBookEntryRecord>,
}

impl From<&sujiu_core::WorldBook> for WorldBookCard {
    fn from(book: &sujiu_core::WorldBook) -> Self {
        Self {
            id: book.id.clone(),
            name: book.name.clone(),
            entries: book.entries.iter().map(Into::into).collect(),
        }
    }
}

impl From<sujiu_runtime::library::WorldBookEntrySummary> for WorldBookEntryRecord {
    fn from(entry: sujiu_runtime::library::WorldBookEntrySummary) -> Self {
        Self {
            id: entry.id,
            name: entry.name,
            content: entry.content,
            keys: entry.keys,
            enabled: entry.enabled,
            constant: entry.constant,
            placement: entry.position.into(),
        }
    }
}

/// The list row for a world book, built from the summary the runtime returns.
///
/// The row and the full card have the same fields today, which is not a reason
/// to hand out the full card instead: the summary is deliberately narrower, and
/// a card that starts being wider must not silently become a second way to read
/// a resource the expensive way.
impl From<sujiu_runtime::library::WorldBookSummary> for WorldBookCard {
    fn from(summary: sujiu_runtime::library::WorldBookSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
            entries: summary.entries.into_iter().map(Into::into).collect(),
        }
    }
}

/// One world book to be saved. Blank `id` means create.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct WorldBookDraftRecord {
    pub id: String,
    pub name: String,
    pub entries: Vec<WorldBookEntryRecord>,
}

impl From<WorldBookDraftRecord> for WorldBookRequest {
    fn from(record: WorldBookDraftRecord) -> Self {
        Self {
            id: Some(record.id).filter(|id| !id.is_empty()),
            name: record.name,
            entries: record.entries.into_iter().map(Into::into).collect(),
        }
    }
}

/// One prompt profile, at list-row fidelity.
///
/// The user-facing name for this is "prompts", not "prompt profile". The Core
/// type name is not vocabulary a user should have to learn, but it stays here in
/// Rust where it costs nothing.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PromptProfileCard {
    pub id: String,
    pub name: String,
}

impl From<sujiu_runtime::library::PromptProfileSummary> for PromptProfileCard {
    fn from(summary: sujiu_runtime::library::PromptProfileSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
        }
    }
}

/// One prompt profile in full, for an editor to prefill from.
///
/// The four prompt bodies are here and the fixed segments are not, on purpose:
/// the runtime edits a profile field by field, so saving from an editor that
/// only renders these strings cannot drop segments the editor never saw.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct PromptProfileDraftRecord {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub user_prompt: String,
    pub post_history_instructions: String,
    pub format_rules: String,
}

impl From<&sujiu_core::PromptProfile> for PromptProfileDraftRecord {
    fn from(profile: &sujiu_core::PromptProfile) -> Self {
        Self {
            id: profile.id.clone(),
            name: profile.name.clone(),
            system_prompt: profile.system_prompt.clone(),
            user_prompt: profile.user_prompt.clone(),
            post_history_instructions: profile.post_history_instructions.clone(),
            format_rules: profile.format_rules.clone(),
        }
    }
}

impl From<PromptProfileDraftRecord> for PromptProfileRequest {
    fn from(record: PromptProfileDraftRecord) -> Self {
        Self {
            id: Some(record.id).filter(|id| !id.is_empty()),
            name: record.name,
            system_prompt: record.system_prompt,
            user_prompt: record.user_prompt,
            post_history_instructions: record.post_history_instructions,
            format_rules: record.format_rules,
        }
    }
}

/// One model an endpoint answers for.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ModelRecord {
    pub id: String,
    pub name: String,
    pub endpoint_id: String,
    /// A word to show a person. It decides nothing.
    pub endpoint_label: String,
    pub configured: bool,
}

impl From<ModelSummary> for ModelRecord {
    fn from(summary: ModelSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
            endpoint_id: summary.endpoint_id,
            endpoint_label: summary.endpoint_label,
            configured: summary.configured,
        }
    }
}

/// The outcome of asking an endpoint what models it has.
///
/// `listing` and `models` answer different questions, and every outcome of the
/// first still leaves manual entry available.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ModelDiscoveryRecord {
    pub listing: String,
    pub note: String,
    pub manual_entry_allowed: bool,
    pub models: Vec<ModelRecord>,
}

impl From<ModelDiscovery> for ModelDiscoveryRecord {
    fn from(discovery: ModelDiscovery) -> Self {
        Self {
            listing: discovery.listing,
            note: discovery.note,
            manual_entry_allowed: discovery.manual_entry_allowed,
            models: discovery.models.into_iter().map(Into::into).collect(),
        }
    }
}

/// One readable collection the model can search.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ContextSourceRecord {
    pub id: String,
    pub kind: ContextKind,
    pub name: String,
    pub description: String,
    pub record_count: u32,
}

impl From<ContextSourceSummary> for ContextSourceRecord {
    fn from(summary: ContextSourceSummary) -> Self {
        Self {
            id: summary.id,
            kind: summary.kind.into(),
            name: summary.name,
            description: summary.description,
            record_count: summary.record_count as u32,
        }
    }
}

/// One tool call the model made inside a turn.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub title: String,
    /// `completed`, `failed`, `interrupted` or `cancelled`.
    pub status: String,
    pub is_error: bool,
    pub result_text: String,
}

impl From<ToolCallSummary> for ToolCallRecord {
    fn from(summary: ToolCallSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
            title: summary.title,
            status: summary.status,
            is_error: summary.is_error,
            result_text: summary.result_text,
        }
    }
}

/// One transcript step, including the tool calls it made.
///
/// The fold a UI applies may hide these, but they are part of the step the model
/// saw, so they cross the boundary whole.
#[derive(Clone, Debug, uniffi::Record)]
pub struct MessageRecord {
    pub id: String,
    pub role: MessageRole,
    /// Which participant spoke, when the transcript says.
    pub speaker_id: Option<String>,
    pub text: String,
    pub tool_calls: Vec<ToolCallRecord>,
}

impl From<MessageSummary> for MessageRecord {
    fn from(summary: MessageSummary) -> Self {
        Self {
            id: summary.id,
            role: summary.role.into(),
            speaker_id: summary.speaker_id,
            text: summary.text,
            tool_calls: summary.tool_calls.into_iter().map(Into::into).collect(),
        }
    }
}

/// A whole conversation: its bindings and its transcript.
///
/// Named `conversation_id` rather than the runtime's internal `session_id`,
/// because a conversation is not two things with two ids. The value is the same
/// one the history list shows.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ConversationSnapshotRecord {
    pub conversation_id: String,
    pub participants: Vec<ParticipantRecord>,
    /// The first participant's card, for a screen that only knows about one.
    pub character: Option<CharacterCard>,
    pub persona_id: Option<String>,
    pub worldbook_ids: Vec<String>,
    pub prompt_profile_id: Option<String>,
    pub messages: Vec<MessageRecord>,
}

impl From<ConversationSnapshot> for ConversationSnapshotRecord {
    fn from(snapshot: ConversationSnapshot) -> Self {
        Self {
            conversation_id: snapshot.session_id,
            participants: snapshot.participants.into_iter().map(Into::into).collect(),
            character: snapshot.character.map(Into::into),
            persona_id: snapshot.persona_id,
            worldbook_ids: snapshot.worldbook_ids,
            prompt_profile_id: snapshot.prompt_profile_id,
            messages: snapshot.messages.into_iter().map(Into::into).collect(),
        }
    }
}

/// Where an endpoint is and what it is allowed to be asked for.
///
/// This is the whole provider configuration a user can set: an address, a name,
/// an optional model and a reference to a secret the platform holds. It is not
/// an OpenAI or Anthropic request, and adding provider wire fields here would be
/// the exact leak this crate exists to prevent.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct EndpointRecord {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub selected_model: Option<String>,
    /// A reference to a secret in platform secure storage, never the secret.
    pub credential_ref: Option<String>,
    /// Advanced compatibility overrides as a raw JSON object, passed through
    /// untouched.
    ///
    /// Kept opaque on purpose. The runtime reads exactly one override today,
    /// and typing the map here would invent a contract the runtime does not have
    /// while inviting a frontend to start making decisions from it.
    pub overrides_json: String,
}

impl From<&EndpointConfig> for EndpointRecord {
    fn from(config: &EndpointConfig) -> Self {
        Self {
            id: config.id.clone(),
            name: config.name.clone(),
            base_url: config.base_url.clone(),
            selected_model: config.selected_model.clone(),
            credential_ref: config.credential_ref.clone(),
            overrides_json: serde_json::to_string(&config.overrides)
                .unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

impl From<EndpointRecord> for EndpointConfig {
    fn from(record: EndpointRecord) -> Self {
        // A malformed override blob must not take the endpoint down with it.
        // The overrides are an advanced escape hatch, so an unreadable one is
        // dropped rather than refused: the address and model, which is what the
        // user is actually configuring, still work.
        let overrides =
            serde_json::from_str(&record.overrides_json).unwrap_or_else(|_| serde_json::Map::new());
        Self {
            id: record.id,
            name: record.name,
            base_url: record.base_url,
            selected_model: record.selected_model,
            credential_ref: record.credential_ref,
            overrides,
        }
    }
}

/// What probing an endpoint concluded.
#[derive(Clone, Debug, uniffi::Record)]
pub struct EndpointExplorationRecord {
    /// The protocol negotiation settled on, or empty when it settled on nothing.
    pub protocol: String,
    pub protocols: Vec<String>,
    /// A code to switch on. See the runtime docs for the full set.
    pub status: String,
    /// Why negotiation concluded what it did, when it is not a clean answer.
    pub reason: Option<String>,
    pub models: ModelDiscoveryRecord,
}

impl From<EndpointExploration> for EndpointExplorationRecord {
    fn from(exploration: EndpointExploration) -> Self {
        Self {
            protocol: exploration.protocol,
            protocols: exploration.protocols,
            status: exploration.status,
            reason: exploration.reason,
            models: exploration.models.into(),
        }
    }
}

/// One line of the diagnostics log, already redacted.
#[derive(Clone, Debug, uniffi::Record)]
pub struct DiagnosticRecord {
    pub at_ms: i64,
    pub kind: DiagnosticKind,
    /// A stable word such as `probe_start`, not a sentence, so a screen can
    /// localize it and a reader can filter on it.
    pub stage: String,
    pub message: String,
    pub fields: HashMap<String, String>,
}

impl From<DiagnosticEntry> for DiagnosticRecord {
    fn from(entry: DiagnosticEntry) -> Self {
        Self {
            at_ms: entry.at_ms,
            kind: entry.kind.into(),
            stage: entry.stage,
            message: entry.message,
            fields: entry.fields.into_iter().collect(),
        }
    }
}

/// One participant a conversation should have.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ParticipantBindingRecord {
    pub character_id: String,
    pub role: ParticipantRole,
    /// An override for the character's own name in this conversation.
    pub display_name: Option<String>,
}

impl From<ParticipantBindingRecord> for ParticipantRequest {
    fn from(record: ParticipantBindingRecord) -> Self {
        Self {
            character_id: record.character_id,
            role: Some(record.role.into()),
            display_name: record.display_name,
        }
    }
}

/// The four things one conversation binds, and nothing else.
///
/// This is the whole of a conversation's relationship to the library: four lists
/// of ids. The conversation does not own a copy of a persona or a world book, so
/// editing a resource edits it for every conversation that names it.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct ConversationBindingRecord {
    pub participants: Vec<ParticipantBindingRecord>,
    pub persona_id: Option<String>,
    pub worldbook_ids: Vec<String>,
    pub prompt_profile_id: Option<String>,
}

impl From<ConversationBindingRecord> for sujiu_runtime::runtime::CreateConversationRequest {
    fn from(record: ConversationBindingRecord) -> Self {
        Self {
            participants: record.participants.into_iter().map(Into::into).collect(),
            persona_id: record.persona_id,
            worldbook_ids: record.worldbook_ids,
            prompt_profile_id: record.prompt_profile_id,
        }
    }
}

/// One turn to run.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct SendTurnRequestRecord {
    pub conversation_id: String,
    pub user_text: String,
    /// An endpoint for this turn only. Absent means the saved one.
    pub endpoint: Option<EndpointRecord>,
    /// The secret to use for this turn. Never persisted by the runtime.
    pub api_key: Option<String>,
}

impl From<SendTurnRequestRecord> for SendTurnRequest {
    fn from(record: SendTurnRequestRecord) -> Self {
        Self {
            session_id: record.conversation_id,
            user_text: record.user_text,
            provider: record.endpoint.map(Into::into),
            api_key: record.api_key,
        }
    }
}
/// What a caller must summarize before a conversation can be compacted.
///
/// The runtime's own `CompactionInput` holds the turns as transcript objects,
/// which a foreign function boundary cannot carry, and which a summarizer prompt
/// would only render as text anyway. So the binding hands over the two things
/// the caller actually needs: the summary this one extends, and the turns to
/// fold in, already rendered. Exposing what has to be summarized is the whole
/// point of asking — a caller cannot summarize turns it cannot see.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CompactionInputRecord {
    /// The summary already standing in for archived turns, empty before the
    /// first compaction.
    pub previous_summary: String,
    /// The previous summary followed by every turn this summary must cover.
    pub prompt_text: String,
}

impl From<&sujiu_core::CompactionInput> for CompactionInputRecord {
    fn from(input: &sujiu_core::CompactionInput) -> Self {
        Self {
            previous_summary: input.previous_summary.clone(),
            prompt_text: input.to_prompt_text(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TurnEventRecord;

    /// The rule that costs the most when broken: an editor prefill is built from
    /// the draft, not the card. A conversion that forgets one field here does
    /// not fail a build, it empties a written prompt on the next save.
    #[test]
    fn a_character_draft_carries_every_field_a_card_does_not() {
        let mut character = sujiu_core::Character::new("char-1", "Lin");
        character.description = "Night-shift radio host".to_string();
        character.personality = "Deflects with questions".to_string();
        character.scenario = "A city that never sees morning".to_string();
        character.first_message = "*The console is still blinking.*".to_string();
        character.alternate_greetings = vec!["*Testing.*".to_string()];
        character.example_dialogue = "Lin: You again.".to_string();
        character.system_prompt = "Speak as Lin, never as the narrator.".to_string();
        character.post_history_instructions = "Keep her voice.".to_string();
        character.worldbook_ids = vec!["worldbook-1".to_string()];

        let draft = CharacterDraftRecord::from(&character);
        assert_eq!(draft.id, "char-1");
        assert_eq!(draft.system_prompt, character.system_prompt);
        assert_eq!(
            draft.post_history_instructions,
            character.post_history_instructions
        );
        assert_eq!(draft.worldbook_ids, character.worldbook_ids);
        assert_eq!(draft.alternate_greetings, character.alternate_greetings);

        // And the card is deliberately the short shape, so a list row cannot
        // be mistaken for something an editor may be prefilled from.
        let card = CharacterCard::from(CharacterSummary {
            id: character.id.clone(),
            name: character.name.clone(),
            description: character.description.clone(),
        });
        assert!(card.description.contains("Night-shift"));
    }

    /// A blank id is a create. Getting this wrong writes a second character
    /// instead of editing the one the user opened.
    #[test]
    fn a_blank_id_asks_for_a_create_and_a_written_id_asks_for_an_edit() {
        let create = CharacterRequest::from(CharacterDraftRecord {
            name: "New".to_string(),
            ..Default::default()
        });
        assert_eq!(create.id, None);

        let edit = CharacterRequest::from(CharacterDraftRecord {
            id: "char-1".to_string(),
            name: "Existing".to_string(),
            ..Default::default()
        });
        assert_eq!(edit.id.as_deref(), Some("char-1"));
    }

    /// An override blob is an escape hatch, not the thing being configured. If
    /// it cannot be read, the address and model still have to work.
    #[test]
    fn an_unreadable_override_blob_costs_the_escape_hatch_and_not_the_endpoint() {
        let endpoint = EndpointRecord {
            id: "endpoint-1".to_string(),
            name: "Home".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            selected_model: Some("a-model".to_string()),
            credential_ref: None,
            overrides_json: "{not json".to_string(),
        };
        let config: EndpointConfig = endpoint.into();
        assert_eq!(config.id, "endpoint-1");
        assert_eq!(config.selected_model.as_deref(), Some("a-model"));
        assert!(config.overrides.is_empty());
    }

    /// The four things one conversation binds arrive as the four answers they
    /// are. An absent binding is "none", not "leave whatever was there".
    #[test]
    fn a_conversation_binding_carries_all_four_answers() {
        let request: sujiu_runtime::runtime::CreateConversationRequest =
            ConversationBindingRecord {
                participants: vec![ParticipantBindingRecord {
                    character_id: "char-1".to_string(),
                    role: ParticipantRole::Character,
                    display_name: Some("The Host".to_string()),
                }],
                persona_id: Some("persona-1".to_string()),
                worldbook_ids: vec!["worldbook-1".to_string(), "worldbook-2".to_string()],
                prompt_profile_id: Some("promptprofile-1".to_string()),
            }
            .into();

        assert_eq!(request.participants.len(), 1);
        assert_eq!(request.participants[0].character_id, "char-1");
        assert_eq!(
            request.participants[0].display_name.as_deref(),
            Some("The Host")
        );
        assert_eq!(request.persona_id.as_deref(), Some("persona-1"));
        assert_eq!(request.worldbook_ids.len(), 2);
        assert_eq!(
            request.prompt_profile_id.as_deref(),
            Some("promptprofile-1")
        );

        let empty: sujiu_runtime::runtime::CreateConversationRequest =
            ConversationBindingRecord::default().into();
        assert!(empty.participants.is_empty());
        assert_eq!(empty.persona_id, None);
        assert!(empty.worldbook_ids.is_empty());
        assert_eq!(empty.prompt_profile_id, None);
    }

    /// A caller cannot summarize turns it cannot see, and it cannot extend a
    /// summary it was never handed. So the summary has to lead the text and be
    /// readable on its own, not appear somewhere among the turns.
    #[test]
    fn a_compaction_input_leads_with_the_summary_it_extends() {
        let input = sujiu_core::CompactionInput {
            previous_summary: "They met at the north gate.".to_string(),
            turns: Vec::new(),
        };
        let record = CompactionInputRecord::from(&input);
        assert_eq!(record.previous_summary, "They met at the north gate.");
        assert!(
            record
                .prompt_text
                .find("They met at the north gate.")
                .expect("the previous summary appears in the prompt text")
                < record
                    .prompt_text
                    .find("Turns to fold into that summary:")
                    .expect("the turns section appears in the prompt text")
        );
    }

    /// A turn event crosses with only the fields its kind carries, so a screen
    /// can switch on the kind without every case having to test for absent.
    #[test]
    fn a_turn_event_keeps_its_optional_fields_absent() {
        let started = TurnEventRecord::from(sujiu_runtime::events::TurnEvent::simple(
            RuntimeTurnEventKind::TurnStarted,
        ));
        assert!(matches!(started.kind, TurnEventKind::TurnStarted));
        assert_eq!(started.text, None);
        assert_eq!(started.tool_name, None);

        let finished = TurnEventRecord::from(sujiu_runtime::events::TurnEvent {
            kind: RuntimeTurnEventKind::ToolCallFinished,
            text: None,
            tool_name: Some("search_context".to_string()),
            tool_call_id: Some("call-1".to_string()),
            is_error: Some(true),
        });
        assert_eq!(finished.tool_name.as_deref(), Some("search_context"));
        assert_eq!(finished.tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(finished.is_error, Some(true));
    }
}
