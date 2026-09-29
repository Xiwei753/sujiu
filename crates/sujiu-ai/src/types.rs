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
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        structured_content: Option<Value>,
        is_error: bool,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub read_only_hint: bool,
    #[serde(default)]
    pub destructive_hint: bool,
    #[serde(default)]
    pub idempotent_hint: bool,
    #[serde(default)]
    pub open_world_hint: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDiscovery {
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub always_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default)]
    pub annotations: ToolAnnotations,

    /// Local-only discovery metadata. Provider adapters must not treat this as
    /// model-visible weighting.
    #[serde(default)]
    pub discovery: ToolDiscovery,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text {
        text: String,
    },
    Resource {
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Image {
        data: String,
        mime_type: String,
    },
    Audio {
        data: String,
        mime_type: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    #[serde(default)]
    pub content: Vec<ToolContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(default)]
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            structured_content: None,
            is_error: false,
        }
    }

    pub fn structured(value: Value) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: value.to_string(),
            }],
            structured_content: Some(value),
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            content: vec![ToolContent::Text {
                text: message.clone(),
            }],
            structured_content: Some(serde_json::json!({"error": message})),
            is_error: true,
        }
    }

    pub fn model_text(&self) -> String {
        let text = self
            .content
            .iter()
            .filter_map(|content| match content {
                ToolContent::Text { text } => Some(text.as_str()),
                ToolContent::Resource { text: Some(text), .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !text.is_empty() {
            text
        } else if let Some(structured) = &self.structured_content {
            structured.to_string()
        } else {
            String::new()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub name: String,
    #[serde(flatten)]
    pub output: ToolOutput,
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
