use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    provider::{AiProvider, ProviderError},
    types::{AssistantTurn, ModelMessage, ModelRole, ProviderRequest, ToolCall},
};

#[derive(Clone, Debug)]
pub struct OpenAiCompatConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,

    /// Some third-party OpenAI-compatible endpoints do not understand
    /// the newer developer role. Map it to system unless explicitly enabled.
    pub supports_developer_role: bool,
}

impl OpenAiCompatConfig {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: None,
            temperature: None,
            supports_developer_role: false,
        }
    }
}

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    config: OpenAiCompatConfig,
}

impl OpenAiCompatProvider {
    pub fn new(config: OpenAiCompatConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }

    fn request_body(&self, request: ProviderRequest) -> Value {
        let messages = request
            .messages
            .into_iter()
            .map(|message| self.encode_message(message))
            .collect::<Vec<_>>();

        let tools = request
            .tools
            .into_iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
        });

        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = Value::String("auto".into());
        }

        if let Some(max_tokens) = self.config.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = self.config.temperature {
            body["temperature"] = json!(temperature);
        }

        body
    }

    fn encode_message(&self, message: ModelMessage) -> Value {
        match message {
            ModelMessage::Text { role, content } => {
                json!({
                    "role": self.encode_role(role),
                    "content": content,
                })
            }
            ModelMessage::AssistantToolCalls { content, calls } => {
                let calls = calls
                    .into_iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments.to_string(),
                            }
                        })
                    })
                    .collect::<Vec<_>>();

                json!({
                    "role": "assistant",
                    "content": content,
                    "tool_calls": calls,
                })
            }
            ModelMessage::ToolResult {
                call_id,
                name: _,
                output,
                is_error: _,
            } => {
                json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output.to_string(),
                })
            }
        }
    }

    fn encode_role(&self, role: ModelRole) -> &'static str {
        match role {
            ModelRole::System => "system",
            ModelRole::Developer if self.config.supports_developer_role => "developer",
            ModelRole::Developer => "system",
            ModelRole::User => "user",
            ModelRole::Assistant => "assistant",
        }
    }
}

#[async_trait]
impl AiProvider for OpenAiCompatProvider {
    async fn complete(&self, request: ProviderRequest) -> Result<AssistantTurn, ProviderError> {
        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&self.request_body(request))
            .send()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if !status.is_success() {
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let response: ChatCompletionResponse = serde_json::from_str(&body)
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;

        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::InvalidResponse("response had no choices".into()))?;

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|call| {
                let arguments = serde_json::from_str(&call.function.arguments).map_err(|error| {
                    ProviderError::InvalidResponse(format!(
                        "tool {} returned invalid JSON arguments: {error}",
                        call.function.name
                    ))
                })?;

                Ok(ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments,
                })
            })
            .collect::<Result<Vec<_>, ProviderError>>()?;

        Ok(AssistantTurn {
            text: choice.message.content.filter(|text| !text.is_empty()),
            tool_calls,
            finish_reason: choice.finish_reason,
            response_id: response.id,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    id: Option<String>,
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatAssistantMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatAssistantMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCall {
    id: String,
    function: ChatFunctionCall,
}

#[derive(Debug, Deserialize)]
struct ChatFunctionCall {
    name: String,
    arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn developer_role_can_fall_back_to_system() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "https://example.invalid/v1",
            "secret",
            "model",
        ));

        assert_eq!(provider.encode_role(ModelRole::Developer), "system");
    }
}
