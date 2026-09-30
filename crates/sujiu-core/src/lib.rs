pub mod character;
pub mod context;
pub mod model;
pub mod prompt;
pub mod protocol;
pub mod provider;
pub mod session;
pub mod transcript;
pub mod worldbook;

pub use character::Character;
pub use context::{ContextKind, ContextRecord, ContextScope, ContextSource};
pub use model::{
    AssistantTurn, ContinuationSupport, ContinuationUpdate, ModelMessage, ModelRole,
    ProviderContinuation, ProviderIdentity, ProviderRequest, ReasoningSidecar, TokenUsage,
    ToolAnnotations, ToolCall, ToolCallState, ToolContent, ToolDefinition, ToolDiscovery,
    ToolOutput, ToolResult,
};
pub use prompt::{
    CacheContinuity, PromptCompiler, PromptPlan, PromptSegment, PromptSource,
    DEFAULT_APP_SYSTEM_PROMPT,
};
pub use protocol::{
    classify, EndpointCapabilities, ModelListing, ProbeFailure, ProbeVerdict, Protocol,
};
pub use provider::{
    apply_reasoning_override, ProviderConfig, ProviderKind, REPLAYS_ASSISTANT_REASONING_KEY,
};
pub use session::{ChatMessage, ChatRole, Session};
pub use transcript::{
    model_messages_from_transcript, validate_pairing, AssistantStep, CompactedTurns,
    CompactionInput, PairingError, ToolCallRecord, ToolResultRecord, Transcript, Turn, TurnState,
    UiMessage, UiToolCall,
};
pub use worldbook::{WorldBook, WorldBookEntry, WorldBookPosition};

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
