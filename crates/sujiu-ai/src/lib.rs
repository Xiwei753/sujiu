pub mod agent;
pub mod builtin;
pub mod context;
pub mod diagnostics;
pub mod negotiate;
pub mod openai_compat;
pub mod provider;
pub mod responses;
pub mod tool;
pub mod types;

pub use agent::{AgentConfig, AgentOutcome, AgentRuntime, AgentStop, CancelToken};
pub use context::{
    project_library, register_standard_context_tools, ContextHit, ContextSearchQuery, ContextStore,
    ContextStoreError, InMemoryContextStore, ListContextSourcesTool, ReadContextTool,
    SearchContextTool,
};
pub use diagnostics::{
    mask_secret, record as record_diagnostic, truncate, DiagnosticEntry, DiagnosticKind,
    DiagnosticLog,
};
pub use negotiate::{
    list_models, negotiate as negotiate_protocol, negotiate_asking, CachedListing, CapabilityCache,
    Negotiation, ProtocolAttempt, IMPLEMENTED_PROTOCOLS,
};
pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};
pub use provider::{http_client, AiProvider, NullStreamSink, ProviderError, StreamSink};
pub use responses::{OpenAiResponsesProvider, ResponsesConfig};
pub use tool::{Tool, ToolError, ToolRegistry};
pub use types::{
    message_prefix_digest, messages_from_prompt_plan, AssistantTurn, ContinuationCoverage,
    ModelMessage, ModelRole, ProviderContinuation, ProviderRequest, TokenUsage, ToolAnnotations,
    ToolCall, ToolCallState, ToolContent, ToolDefinition, ToolDiscovery, ToolOutput, ToolResult,
};

/// Provider data the runtime needs from the core.
pub use sujiu_core::{
    apply_reasoning_override, EndpointCapabilities, EndpointConfig, ModelListing, ProbeFailure,
    ProbeVerdict, Protocol,
};
