pub mod agent;
pub mod openai_compat;
pub mod provider;
pub mod tool;
pub mod types;

pub use agent::{AgentConfig, AgentError, AgentOutcome, AgentRuntime};
pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};
pub use provider::{AiProvider, ProviderError};
pub use tool::{Tool, ToolError, ToolRegistry};
pub use types::{
    messages_from_prompt_plan, AssistantTurn, ModelMessage, ModelRole, ProviderRequest,
    ToolCall, ToolDefinition, ToolResult,
};
