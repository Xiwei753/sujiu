use serde::{Deserialize, Serialize};
use serde_json::Value;
use sujiu_core::{ChatRole, PromptPlan};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    System,
    Developer,
    User,
    Assistant,
}

impl From<ChatRole> for ModelRole {
    fn from(value: ChatRole) -> Self {
        match value {
            ChatRole::System => Self::System,
            ChatRole::Developer => Self::Developer,
            ChatRole::User => Self::User,
            ChatRole::Assistant => Self::Assistant,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelMessage {
    Text {
        role: ModelRole,
        content: String,
    },
    AssistantToolCalls {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        calls: Vec<ToolCall>,
    },
    ToolResult {
        call_id: String,
        name: String,
        output: Value,
        is_error: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub always_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub name: String,
    pub output: Value,
    pub is_error: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProviderRequest {
    pub messages: Vec<ModelMessage>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AssistantTurn {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub response_id: Option<String>,
}

pub fn messages_from_prompt_plan(plan: &PromptPlan) -> Vec<ModelMessage> {
    plan.segments
        .iter()
        .map(|segment| ModelMessage::Text {
            role: segment.role.into(),
            content: segment.content.clone(),
        })
        .collect()
}
