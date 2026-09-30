use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    provider::{AiProvider, ProviderError, StreamSink},
    types::{
        AssistantTurn, ContinuationSupport, ContinuationUpdate, ModelMessage, ModelRole,
        ProviderContinuation, ProviderIdentity, ProviderRequest, ReasoningSidecar, TokenUsage,
        ToolCall,
    },
};

#[derive(Clone, Debug)]
pub struct OpenAiCompatConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,

    /// Which endpoint and model this provider talks to.
    ///
    /// The adapter stamps it into every continuation state it produces, so the
    /// transcript can later tell "same endpoint, same model" from "a different
    /// OpenAI-compatible service that happens to use the same model name".
    pub identity: ProviderIdentity,

    /// Some third-party OpenAI-compatible endpoints do not understand
    /// the newer developer role. Map it to system unless explicitly enabled.
    pub supports_developer_role: bool,

    /// Whether this transport rejects an assistant tool call that comes back
    /// without the reasoning that produced it.
    ///
    /// DeepSeek's thinking mode is the documented case: a multi-round tool
    /// loop has to replay every previous assistant `reasoning_content`, and the
    /// API answers 400 when one is missing. Other OpenAI-compatible services
    /// have no such field, so the reasoning is left out of the wire entirely
    /// rather than sent under a name that service may not understand.
    ///
    /// This is also what keeps one provider's reasoning wire metadata from
    /// reaching another: the sidecar is stored provider-neutral in the
    /// transcript, and only a transport that declared the requirement is told
    /// the field name.
    pub requires_reasoning_content_for_tool_calls: bool,
}

impl OpenAiCompatConfig {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into();
        let model = model.into();

        Self {
            identity: ProviderIdentity {
                kind: sujiu_core::ProviderKind::OpenAiCompatible,
                provider_id: String::new(),
                base_url: base_url.clone(),
                model: model.clone(),
            },
            base_url,
            api_key: api_key.into(),
            model,
            max_tokens: None,
            temperature: None,
            supports_developer_role: false,
            requires_reasoning_content_for_tool_calls: false,
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
        // Cached token counts are the only way to tell whether a request reused
        // its prefix, so ask for them instead of guessing.
        body["stream_options"] = json!({ "include_usage": true });
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
            ModelMessage::AssistantToolCalls {
                content,
                calls,
                reasoning,
            } => {
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

                let mut message = json!({
                    "role": "assistant",
                    "content": content,
                    "tool_calls": calls,
                });

                // Two conditions, and both are needed.
                //
                // The transport has to have asked for the field, because
                // sending a name another service does not define is worse than
                // sending nothing. And the reasoning has to be the one this
                // provider produced: another provider's reasoning is that
                // provider's wire metadata, and handing it over under our field
                // name would put a foreign protocol's text where the endpoint
                // expects its own. The visible text of that same step still
                // travels, because visible text is portable.
                if self.config.requires_reasoning_content_for_tool_calls {
                    if let Some(reasoning) = reasoning
                        .as_ref()
                        .filter(|sidecar| sidecar.is_replayable_for(&self.config.identity))
                    {
                        message["reasoning_content"] = json!(reasoning.content);
                    }
                }

                message
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

/// Read token accounting out of an OpenAI-compatible `usage` object.
///
/// `prompt_tokens_details.cached_tokens` is the cache read count, which is the
/// only direct evidence that a request reused its prefix. Endpoints that do not
/// report a field simply leave it absent.
fn parse_usage(usage: &Value) -> Option<TokenUsage> {
    let input = usage.get("prompt_tokens").and_then(Value::as_u64);
    let output = usage.get("completion_tokens").and_then(Value::as_u64);
    let cached = usage
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64);
    let cache_write = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64);

    if input.is_none() && output.is_none() && cached.is_none() && cache_write.is_none() {
        return None;
    }

    Some(TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cached,
        cache_write_tokens: cache_write,
    })
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
    /// The reasoning this endpoint produced. It is kept here rather than only
    /// handed to the sink, because the provider that parsed it is the only
    /// thing that can say where it came from, and a request may only be given
    /// back the reasoning of the provider it is being sent to.
    reasoning: String,
    response_id: Option<String>,
    finish_reason: Option<String>,
    usage: Option<TokenUsage>,
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

        // Streamed usage arrives in its own chunk, after the choices, and is
        // only sent when the request asked for it.
        if let Some(usage) = chunk.get("usage").and_then(parse_usage) {
            self.usage = Some(usage);
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
            self.reasoning.push_str(reasoning);
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

    fn finish(self, config: &OpenAiCompatConfig) -> Result<AssistantTurn, ProviderError> {
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
            // Stamped with this endpoint's identity, so a later request only
            // gets it back when it is going to the provider that wrote it.
            reasoning: (!self.reasoning.trim().is_empty())
                .then(|| ReasoningSidecar::new(self.reasoning.clone(), config.identity.clone())),
            // The completion id is recorded, but it is not chainable state.
            //
            // `chat/completions` has no `previous_response_id` and no server
            // side conversation handle: the only way to continue a conversation
            // on this transport is to resend the transcript. Recording the id
            // as `Unsupported` is what stops `is_reusable_for` from treating it
            // as a continuation token, and reporting `Unchanged` says this
            // transport has no opinion about any handle instead of pretending
            // to have produced one.
            continuation: ContinuationUpdate::Replace(ProviderContinuation {
                identity: config.identity.clone(),
                support: ContinuationSupport::Unsupported,
                response_id: self.response_id,
                state: Default::default(),
            }),
            usage: self.usage,
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
            reasoning: choice
                .message
                .reasoning_content
                .filter(|text| !text.trim().is_empty())
                .map(|text| ReasoningSidecar::new(text, self.config.identity.clone())),
            continuation: ContinuationUpdate::Replace(ProviderContinuation {
                identity: self.config.identity.clone(),
                // See `StreamAccumulator::finish`: a `chat/completions`
                // completion id is a label, not a chainable handle.
                support: ContinuationSupport::Unsupported,
                response_id: response.id,
                state: Default::default(),
            }),
            usage: response.usage.as_ref().and_then(parse_usage),
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

            // A whole response can arrive in a single chunk, so stopping only
            // between chunks would let a cancelled turn keep streaming. Check
            // after every server-sent event instead.
            while let Some(newline) = pending.find('\n') {
                let line = pending[..newline].to_string();
                pending.drain(..=newline);
                accumulator.push_line(&line, sink)?;

                if !sink.should_continue() {
                    return Err(ProviderError::Cancelled);
                }
            }

            if !sink.should_continue() {
                return Err(ProviderError::Cancelled);
            }
        }

        if !pending.is_empty() {
            accumulator.push_line(&pending, sink)?;
        }

        accumulator.finish(&self.config)
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    id: Option<String>,
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<Value>,
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
    /// The thinking-mode reasoning this endpoint produced alongside the answer.
    #[serde(default)]
    reasoning_content: Option<String>,
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

        let config = OpenAiCompatConfig::new("https://example.invalid/v1", "secret", "model");
        (accumulator.finish(&config).unwrap(), sink)
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
            messages: vec![ModelMessage::user("hi")],
            ..ProviderRequest::default()
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
        assert_eq!(turn.finish_reason.as_deref(), Some("stop"));
        assert!(turn.tool_calls.is_empty());

        // The completion id is recorded, but not as chainable state. This
        // transport has no way to resume from it, so replaying it would be
        // meaningless and `is_reusable_for` must refuse it.
        let continuation = turn
            .continuation
            .produced()
            .expect("the response id must be kept");
        assert_eq!(continuation.response_id.as_deref(), Some("resp-1"));
        assert_eq!(continuation.identity.model, "model");
        assert_eq!(
            continuation.identity.kind,
            sujiu_core::ProviderKind::OpenAiCompatible
        );
        assert!(!continuation.support.is_chainable());
        assert!(!continuation.is_reusable_for(&continuation.identity));
    }

    #[test]
    fn streamed_usage_is_recorded_including_cache_reads() {
        let (turn, _sink) = feed(&[
            r#"data: {"id":"resp-1","choices":[{"delta":{"content":"hi"},"finish_reason":"stop"}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":1200,"completion_tokens":40,"prompt_tokens_details":{"cached_tokens":1024}}}"#,
        ]);

        let usage = turn.usage.expect("usage must be recorded");
        assert_eq!(usage.input_tokens, Some(1200));
        assert_eq!(usage.output_tokens, Some(40));
        // The cache read count is what shows whether the prefix was reused.
        assert_eq!(usage.cached_input_tokens, Some(1024));
    }

    #[test]
    fn streaming_request_asks_for_usage() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "https://example.invalid/v1",
            "secret",
            "model",
        ));

        let body = provider.streaming_request_body(ProviderRequest {
            messages: vec![ModelMessage::user("hi")],
            ..ProviderRequest::default()
        });

        assert_eq!(body["stream_options"]["include_usage"], json!(true));
    }

    #[test]
    fn continuation_state_is_never_sent_to_a_different_provider() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "https://example.invalid/v1",
            "secret",
            "model",
        ));

        // Chat Completions has no place for another provider's continuation
        // items, so the wire body must not grow one. The normalized messages
        // already carry everything the model needs.
        let body = provider.request_body(ProviderRequest {
            messages: vec![ModelMessage::user("hi")],
            continuation: Some(ProviderContinuation {
                identity: ProviderIdentity {
                    kind: sujiu_core::ProviderKind::Anthropic,
                    provider_id: "other".into(),
                    base_url: "https://other.invalid".into(),
                    model: "other".into(),
                },
                support: ContinuationSupport::ResponseId,
                response_id: Some("resp_1".into()),
                state: Default::default(),
            }),
            ..ProviderRequest::default()
        });

        assert!(body.get("previous_response_id").is_none());
        assert!(body.get("continuation").is_none());
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

        let config = OpenAiCompatConfig::new("https://example.invalid/v1", "secret", "model");
        let turn = accumulator.finish(&config).unwrap();

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

    /// A thinking-mode endpoint answers a tool call, and its follow-up request
    /// has to carry the reasoning back. A transport that never declared the
    /// requirement must not be handed a field it does not know, so the same
    /// message encodes two different ways.
    #[test]
    fn assistant_reasoning_is_replayed_only_where_the_endpoint_requires_it() {
        let demanding_config = OpenAiCompatConfig {
            requires_reasoning_content_for_tool_calls: true,
            ..OpenAiCompatConfig::new("https://example.invalid/v1", "secret", "deepseek-chat")
        };
        let assistant = ModelMessage::AssistantToolCalls {
            content: Some("looking it up".into()),
            calls: vec![ToolCall {
                id: "call-1".into(),
                name: "search_context".into(),
                arguments: json!({"query": "lore"}),
            }],
            reasoning: Some(ReasoningSidecar::new(
                "The western tower is the likely place.",
                demanding_config.identity.clone(),
            )),
        };

        let demanding = OpenAiCompatProvider::new(demanding_config);
        let indifferent = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "https://example.invalid/v1",
            "secret",
            "model",
        ));

        let request = ProviderRequest {
            messages: vec![ModelMessage::user("hi"), assistant],
            ..ProviderRequest::default()
        };

        let encoded = demanding.request_body(request.clone());
        let assistant_message = encoded["messages"]
            .as_array()
            .expect("messages")
            .last()
            .expect("the assistant message is the last one");
        assert_eq!(
            assistant_message["reasoning_content"],
            json!("The western tower is the likely place.")
        );

        let encoded = indifferent.request_body(request);
        let assistant_message = encoded["messages"]
            .as_array()
            .expect("messages")
            .last()
            .expect("the assistant message is the last one");
        assert!(
            assistant_message.get("reasoning_content").is_none(),
            "an endpoint that never asked for it must not receive one: {assistant_message:?}"
        );
    }

    /// Visible text crosses providers; the reasoning sidecar does not. `field`
    /// is a field of one protocol, and sending the thinking that produced a call
    /// to a different vendor's endpoint is sending private model state to
    /// somebody who did not produce it and did not ask for it.
    #[test]
    fn one_providers_reasoning_is_never_encoded_into_anothers_request() {
        let provider_a = OpenAiCompatConfig {
            requires_reasoning_content_for_tool_calls: true,
            ..OpenAiCompatConfig::new("https://a.example/v1", "secret", "reasoner-a")
        };
        let provider_b = OpenAiCompatConfig {
            requires_reasoning_content_for_tool_calls: true,
            ..OpenAiCompatConfig::new("https://b.example/v1", "secret", "reasoner-b")
        };

        let request = ProviderRequest {
            messages: vec![ModelMessage::AssistantToolCalls {
                content: Some("looking it up".into()),
                calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "search_context".into(),
                    arguments: json!({"query": "lore"}),
                }],
                reasoning: Some(ReasoningSidecar::new(
                    "provider A thought about the western tower",
                    provider_a.identity.clone(),
                )),
            }],
            ..ProviderRequest::default()
        };

        let to_a = OpenAiCompatProvider::new(provider_a.clone()).request_body(request.clone());
        let to_b = OpenAiCompatProvider::new(provider_b).request_body(request);

        let a_message = &to_a["messages"][0];
        assert_eq!(
            a_message["reasoning_content"],
            json!("provider A thought about the western tower")
        );

        let b_message = &to_b["messages"][0];
        assert!(
            b_message.get("reasoning_content").is_none(),
            "a different endpoint must not receive another provider's reasoning: {b_message:?}"
        );
        // The transcript itself still carries it, and the visible text still
        // travels, so only the wire field is withheld.
        assert_eq!(
            b_message["tool_calls"][0]["id"],
            json!("call-1"),
            "the tool call itself is portable and must survive the switch"
        );
    }

    /// An empty sidecar is worse than none: a blank reasoning block reads as a
    /// truncated one.
    #[test]
    fn a_blank_reasoning_sidecar_is_not_sent() {
        let config = OpenAiCompatConfig {
            requires_reasoning_content_for_tool_calls: true,
            ..OpenAiCompatConfig::new("https://example.invalid/v1", "secret", "deepseek-chat")
        };
        let provider = OpenAiCompatProvider::new(config.clone());

        let request = ProviderRequest {
            messages: vec![ModelMessage::AssistantToolCalls {
                content: None,
                calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "search_context".into(),
                    arguments: json!({}),
                }],
                reasoning: Some(ReasoningSidecar::new("   ", config.identity.clone())),
            }],
            ..ProviderRequest::default()
        };

        let encoded = provider.request_body(request);
        let assistant_message = &encoded["messages"][0];
        assert!(assistant_message.get("reasoning_content").is_none());
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
