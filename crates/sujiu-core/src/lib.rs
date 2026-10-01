pub mod character;
pub mod chat;
pub mod context;
pub mod conversation;
pub mod library;
pub mod model;
pub mod persona;
pub mod prompt;
pub mod prompt_profile;
pub mod protocol;
pub mod provider;
pub mod transcript;
pub mod worldbook;

pub use character::{split_embedded_world_book, Character, CHARACTER_SCHEMA_VERSION};
pub use chat::{ChatMessage, ChatRole};
pub use context::{ContextKind, ContextRecord, ContextScope, ContextSource};
pub use conversation::{Conversation, Participant, ParticipantRole, CONVERSATION_SCHEMA_VERSION};
pub use library::{standalone_context, Library};
pub use model::{
    message_prefix_digest, AssistantTurn, ContinuationCoverage, ContinuationSupport,
    ContinuationUpdate, ModelMessage, ModelRole, ProviderContinuation, ProviderIdentity,
    ProviderRequest, ReasoningSidecar, TokenUsage, ToolAnnotations, ToolCall, ToolCallState,
    ToolContent, ToolDefinition, ToolDiscovery, ToolOutput, ToolResult, COVERAGE_KEY,
    SENT_MESSAGES_KEY,
};
pub use persona::{Persona, PERSONA_SCHEMA_VERSION};
pub use prompt::{
    CacheContinuity, DroppedWorldBookEntry, PromptCompiler, PromptContext, PromptPlan,
    PromptSegment, PromptSource, WorldBookBudget, WorldBookDropReason, DEFAULT_APP_SYSTEM_PROMPT,
    DEFAULT_MAX_WORLD_BOOK_CHARS, DEFAULT_MAX_WORLD_BOOK_ENTRIES,
};
pub use prompt_profile::{
    PromptPosition, PromptProfile, PromptProfileSegment, PROMPT_PROFILE_SCHEMA_VERSION,
};
pub use protocol::{
    classify, EndpointCapabilities, ModelListing, ProbeFailure, ProbeVerdict, Protocol,
};
pub use provider::{apply_reasoning_override, EndpointConfig, REPLAYS_ASSISTANT_REASONING_KEY};
pub use transcript::{
    model_messages_from_transcript, validate_pairing, AssistantStep, CompactedTurns,
    CompactionInput, PairingError, ToolCallRecord, ToolResultRecord, Transcript, Turn, TurnState,
    UiMessage, UiToolCall,
};
pub use worldbook::{WorldBook, WorldBookEntry, WorldBookPosition, WORLD_BOOK_SCHEMA_VERSION};

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
