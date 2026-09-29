pub mod agent;
pub mod builtin;
pub mod context;
pub mod openai_compat;
pub mod provider;
pub mod tool;
pub mod types;

pub use agent::{AgentConfig, AgentError, AgentOutcome, AgentRuntime, CancelToken};
pub use context::{
    register_standard_context_tools, ContextHit, ContextSearchQuery, ContextStore,
    ContextStoreError, InMemoryContextStore, ListContextSourcesTool, ReadContextTool,
    SearchContextTool,
};
pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};
pub use provider::{AiProvider, NullStreamSink, ProviderError, StreamSink};
pub use tool::{Tool, ToolError, ToolRegistry};
pub use types::{
    messages_from_prompt_plan, AssistantTurn, ModelMessage, ModelRole, ProviderRequest,
    ToolAnnotations, ToolCall, ToolContent, ToolDefinition, ToolDiscovery, ToolOutput, ToolResult,
};
