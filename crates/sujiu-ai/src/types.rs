//! Provider-neutral model types, owned by `sujiu-core`.
//!
//! They are re-exported here because `sujiu-ai` is where adapters and the
//! agent loop consume them. Keeping one definition means the persisted
//! transcript and the messages sent to a provider can never drift apart.

pub use sujiu_core::model::{
    messages_from_prompt_plan, AssistantTurn, ModelMessage, ModelRole, ProviderContinuation,
    ProviderRequest, TokenUsage, ToolAnnotations, ToolCall, ToolCallState, ToolContent,
    ToolDefinition, ToolDiscovery, ToolOutput, ToolResult,
};
