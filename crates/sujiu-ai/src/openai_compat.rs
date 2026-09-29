use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    provider::{AiProvider, ProviderError, StreamSink},
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

    fn streaming_request_body(&self, request: ProviderRequest) -> Value {
        let mut body = self.request_body(request);
        body["stream"] = json!(true);
        body
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
                        "parameters": tool.input_schema,
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
                content,
                structured_content,
                is_error,
            } => {
                let content = if content.is_empty() {
                    structured_content
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                } else if is_error {
                    json!({"error": content}).to_string()
                } else {
                    content
                };

                json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": content,
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

/// Accumulates streamed tool call fragments. OpenAI-compatible endpoints split
/// the name and the JSON arguments across several deltas.
#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

/// Accumulates one streamed assistant turn from server-sent events.
///
/// The parser is deliberately transport free: it consumes decoded lines and
/// emits normalized deltas, so the wire format stays inside this module.
#[derive(Default)]
struct StreamAccumulator {
    text: String,
    response_id: Option<String>,
    finish_reason: Option<String>,
    tool_calls: BTreeMap<usize, ToolCallAccumulator>,
}

impl StreamAccumulator {
    fn push_line(&mut self, line: &str, sink: &mut dyn StreamSink) -> Result<(), ProviderError> {
        let Some(data) = line.trim().strip_prefix("data:") else {
            return Ok(());
        };

        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }

        let chunk: Value = serde_json::from_str(data)
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;

        if self.response_id.is_none() {
            self.response_id = chunk.get("id").and_then(Value::as_str).map(str::to_owned);
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return Ok(());
        };

        if let Some(reason) = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty() && *reason != "null")
        {
            self.finish_reason = Some(reason.to_owned());
        }

        let Some(delta) = choice.get("delta") else {
            return Ok(());
        };

        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            sink.on_reasoning_delta(reasoning);
        }

        if let Some(content) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            self.text.push_str(content);
            sink.on_text_delta(content);
        }

        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, call) in calls.iter().enumerate() {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|index| index as usize)
                    .unwrap_or(position);
                let requested = !self.tool_calls.contains_key(&index);
                let entry = self.tool_calls.entry(index).or_default();

                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    entry.id.push_str(id);
                }

                if let Some(function) = call.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        entry.name.push_str(name);
                    }
                    if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                        entry.arguments.push_str(arguments);
                    }
                }

                // The first fragment announces the call, so the platform can
                // show a waiting state while the model is still streaming.
                if requested {
                    sink.on_tool_call_requested(&ToolCall {
                        id: entry.id.clone(),
                        name: entry.name.clone(),
                        arguments: serde_json::Value::Null,
                    });
                }
            }
        }

        Ok(())
    }

    fn finish(self) -> Result<AssistantTurn, ProviderError> {
        let tool_calls = self
            .tool_calls
            .into_values()
            .map(|entry| {
                let arguments = serde_json::from_str(&entry.arguments).map_err(|error| {
                    ProviderError::InvalidResponse(format!(
                        "tool {} returned invalid JSON arguments: {error}",
                        entry.name
                    ))
                })?;

                Ok(ToolCall {
                    id: entry.id,
                    name: entry.name,
                    arguments,
                })
            })
            .collect::<Result<Vec<_>, ProviderError>>()?;

        Ok(AssistantTurn {
            text: (!self.text.is_empty()).then_some(self.text),
            tool_calls,
            finish_reason: self.finish_reason,
            response_id: self.response_id,
        })
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
                let arguments =
                    serde_json::from_str(&call.function.arguments).map_err(|error| {
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

    async fn stream(
        &self,
        request: ProviderRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<AssistantTurn, ProviderError> {
        let body = self.streaming_request_body(request);

        let mut response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .map_err(|error| ProviderError::Transport(error.to_string()))?;

            return Err(ProviderError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let mut pending = String::new();
        let mut accumulator = StreamAccumulator::default();

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?
        {
            pending.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline) = pending.find('\n') {
                let line = pending[..newline].to_string();
                pending.drain(..=newline);
                accumulator.push_line(&line, sink)?;
            }

            if !sink.should_continue() {
                break;
            }
        }

        if !pending.is_empty() {
            accumulator.push_line(&pending, sink)?;
        }

        accumulator.finish()
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

    #[derive(Default)]
    struct RecordingSink {
        deltas: Vec<String>,
        reasoning: Vec<String>,
        requested: Vec<String>,
        started: Vec<String>,
    }

    impl StreamSink for RecordingSink {
        fn on_text_delta(&mut self, delta: &str) {
            self.deltas.push(delta.to_owned());
        }

        fn on_reasoning_delta(&mut self, delta: &str) {
            self.reasoning.push(delta.to_owned());
        }

        fn on_tool_call_requested(&mut self, call: &ToolCall) {
            self.requested.push(call.name.clone());
        }

        fn on_tool_call_started(&mut self, call: &ToolCall) {
            self.started.push(call.name.clone());
        }
    }

    fn feed(lines: &[&str]) -> (AssistantTurn, RecordingSink) {
        let mut sink = RecordingSink::default();
        let mut accumulator = StreamAccumulator::default();

        for line in lines {
            accumulator.push_line(line, &mut sink).unwrap();
        }

        (accumulator.finish().unwrap(), sink)
    }

    #[test]
    fn developer_role_can_fall_back_to_system() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "https://example.invalid/v1",
            "secret",
            "model",
        ));

        assert_eq!(provider.encode_role(ModelRole::Developer), "system");
    }

    #[test]
    fn streaming_request_adds_the_stream_flag() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "https://example.invalid/v1",
            "secret",
            "model",
        ));

        let request = ProviderRequest {
            messages: vec![ModelMessage::Text {
                role: ModelRole::User,
                content: "hi".into(),
            }],
            tools: Vec::new(),
        };

        assert!(provider
            .request_body(request.clone())
            .get("stream")
            .is_none());
        assert_eq!(
            provider.streaming_request_body(request)["stream"],
            json!(true)
        );
    }

    #[test]
    fn streaming_text_is_normalized_into_deltas() {
        let (turn, sink) = feed(&[
            ": keep-alive",
            r#"data: {"id":"resp-1","choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ]);

        assert_eq!(sink.deltas, vec!["Hel".to_string(), "lo".to_string()]);
        assert_eq!(turn.text.as_deref(), Some("Hello"));
        assert_eq!(turn.response_id.as_deref(), Some("resp-1"));
        assert_eq!(turn.finish_reason.as_deref(), Some("stop"));
        assert!(turn.tool_calls.is_empty());
    }

    #[test]
    fn a_streamed_tool_call_is_announced_before_the_turn_finishes() {
        let mut sink = RecordingSink::default();
        let mut accumulator = StreamAccumulator::default();

        // The first fragment only carries the id and the name, the arguments
        // arrive later. That is exactly why the request is announced on the
        // first fragment: the platform can show a waiting state immediately.
        accumulator
            .push_line(
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"search_context","arguments":"{\"query\":\"tower\"}"}}]}}]}"#,
                &mut sink,
            )
            .unwrap();

        assert_eq!(sink.requested, vec!["search_context".to_string()]);
        assert!(
            sink.started.is_empty(),
            "the tool must not be reported as running yet"
        );

        let turn = accumulator.finish().unwrap();

        // A second fragment for the same call must not announce it twice.
        assert_eq!(sink.requested.len(), 1);
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call-1");
        assert_eq!(turn.tool_calls[0].name, "search_context");
    }

    #[test]
    fn streaming_tool_call_fragments_are_reassembled() {
        let (turn, _sink) = feed(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"search_","arguments":"{\"query\":"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"context","arguments":"\"lore\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ]);

        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call-1");
        assert_eq!(turn.tool_calls[0].name, "search_context");
        assert_eq!(turn.tool_calls[0].arguments["query"], "lore");
        assert_eq!(turn.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn streaming_reasoning_is_reported_separately() {
        let (turn, sink) = feed(&[
            r#"data: {"choices":[{"delta":{"reasoning_content":"think"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"answer"}}]}"#,
        ]);

        assert_eq!(sink.reasoning, vec!["think".to_string()]);
        assert_eq!(turn.text.as_deref(), Some("answer"));
    }

    #[test]
    fn invalid_stream_payload_is_a_provider_error() {
        let mut sink = RecordingSink::default();
        let mut accumulator = StreamAccumulator::default();

        let error = accumulator
            .push_line("data: {not json}", &mut sink)
            .expect_err("malformed payload must be rejected");

        assert!(matches!(error, ProviderError::InvalidResponse(_)));
    }
}
