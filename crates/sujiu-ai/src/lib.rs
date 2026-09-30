pub mod agent;
pub mod builtin;
pub mod context;
pub mod negotiate;
pub mod openai_compat;
pub mod provider;
pub mod responses;
pub mod tool;
pub mod types;

pub use agent::{AgentConfig, AgentOutcome, AgentRuntime, AgentStop, CancelToken};
pub use context::{
    register_standard_context_tools, ContextHit, ContextSearchQuery, ContextStore,
    ContextStoreError, InMemoryContextStore, ListContextSourcesTool, ReadContextTool,
    SearchContextTool,
};
pub use negotiate::{
    list_models, negotiate as negotiate_protocol, CapabilityCache, Negotiation, ProtocolAttempt,
};
pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};
pub use provider::{http_client, AiProvider, NullStreamSink, ProviderError, StreamSink};
pub use responses::{OpenAiResponsesProvider, ResponsesConfig};
pub use tool::{Tool, ToolError, ToolRegistry};
pub use types::{
    messages_from_prompt_plan, AssistantTurn, ModelMessage, ModelRole, ProviderContinuation,
    ProviderRequest, TokenUsage, ToolAnnotations, ToolCall, ToolCallState, ToolContent,
    ToolDefinition, ToolDiscovery, ToolOutput, ToolResult,
};

/// Provider data the runtime needs from the core.
pub use sujiu_core::{
    apply_reasoning_override, EndpointCapabilities, EndpointConfig, ModelListing, ProbeFailure,
    ProbeVerdict, Protocol,
};
