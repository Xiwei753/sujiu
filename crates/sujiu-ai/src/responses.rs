//! The OpenAI Responses transport.
//!
//! This is the transport the rest of the runtime would have been written around
//! if it had been built first, and it is why the protocol order puts it first.
//! Chat Completions can be *made* to carry continuation, reasoning and tool
//! state, but only by bolting fields onto a message shape that has nowhere to
//! put them. Responses has all three natively:
//!
//! - continuation is a server-side handle, named `previous_response_id`;
//! - reasoning is an output item the endpoint produced, not a string an
//!   assistant message happens to carry;
//! - a tool call is an item, and its result is the matching `call_id` item.
//!
//! So when an endpoint speaks this, the runtime uses it and stops pretending a
//! completion id is a handle.
//!
//! ## The portable base is still sent
//!
//! Native continuation does not replace the transcript. It lets a request skip
//! re-sending the part of it the endpoint already holds, which is what the
//! provider-side cache is for. The transcript stays the base: if the endpoint,
//! the protocol or the model changes, there is no handle to use and the whole
//! thing goes out again, and the conversation continues exactly where it left
//! off.
//!
//! ## What a handle records, and when it is believed
//!
//! After this response, the endpoint holds *everything* that produced it: the
//! prefix an earlier handle covered, the items sent now, and the output items
//! this response generated. So the new handle records the whole request as a
//! provider-neutral message prefix, plus a digest of that prefix, plus the
//! assistant message the response itself added. Tool results are not counted:
//! the endpoint has not seen them, which is exactly why the next round sends
//! them and nothing else.
//!
//! Reuse is *proved*, not assumed. A handle is only allowed to shorten a request
//! when the request's own leading messages still hash to the digest the handle
//! recorded. Anything that moves the prompt — a world-book entry that newly
//! matches, an edited persona or prompt profile, a compaction summary that
//! replaced history — changes that prefix, so the digest differs and the whole
//! transcript goes out again instead of being silently skipped against a
//! conversation the endpoint holds in its old shape.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use sujiu_core::{message_prefix_digest, EndpointCapabilities, Protocol};

use crate::{
    provider::{AiProvider, NullStreamSink, ProviderError, StreamSink},
    types::{
        AssistantTurn, ContinuationCoverage, ContinuationSupport, ContinuationUpdate, ModelMessage,
        ModelRole, ProviderContinuation, ProviderIdentity, ProviderRequest, ReasoningSidecar,
        TokenUsage, ToolCall,
    },
};

/// What this request will have made the endpoint hold, once its response lands.
///
/// Recorded for the *new* handle, and computed from this request rather than
/// inherited from the handle this request continued from. Recording the old
/// coverage instead is the mistake this type exists to prevent: the new
/// `response_id` would claim to cover only what the previous response had seen,
/// and the round after that would re-send input the endpoint already holds,
/// which is billed twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SentCoverage {
    /// Leading provider-neutral messages the endpoint will hold.
    messages: usize,
    /// Digest of exactly those messages.
    digest: String,
    /// Wire items those messages became, for auditing the conversion.
    wire_items: usize,
}

impl SentCoverage {
    fn of(messages: &[ModelMessage], wire_items: usize) -> Self {
        Self {
            messages: messages.len(),
            digest: message_prefix_digest(messages),
            wire_items,
        }
    }

    fn as_coverage(self, assistant_messages: usize) -> ContinuationCoverage {
        ContinuationCoverage {
            sent: self.messages,
            digest: self.digest,
            assistant_messages,
            wire_items: self.wire_items,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResponsesConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,

    /// Stamped into every piece of provider state this adapter produces.
    ///
    /// The protocol is part of the identity on purpose. A response id from
    /// `/responses` means nothing to `/chat/completions`, so reusing it across
    /// a protocol change would be handing an endpoint a handle from a
    /// conversation it has never seen.
    pub identity: ProviderIdentity,

    pub capabilities: EndpointCapabilities,
}

impl ResponsesConfig {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into();
        let model = model.into();

        Self {
            identity: ProviderIdentity {
                protocol: Protocol::OpenAiResponses,
                endpoint_id: String::new(),
                base_url: base_url.clone(),
                model: model.clone(),
            },
            base_url,
            api_key: api_key.into(),
            model,
            max_output_tokens: None,
            temperature: None,
            capabilities: EndpointCapabilities::negotiate(&[Protocol::OpenAiResponses])
                .expect("the Responses protocol is implemented here"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct OpenAiResponsesProvider {
    client: reqwest::Client,
    config: ResponsesConfig,
}

impl OpenAiResponsesProvider {
    pub fn new(config: ResponsesConfig) -> Self {
        Self {
            client: crate::provider::http_client(),
            config,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/responses", self.config.base_url.trim_end_matches('/'))
    }

    /// How much of the transcript this endpoint provably already holds.
    ///
    /// Zero means send everything, which is always correct. It is what happens
    /// whenever the handle belongs to another endpoint or protocol, and also
    /// whenever the request's own leading messages no longer hash to the digest
    /// the handle recorded — the prompt moved since that response, so the
    /// endpoint's copy of it is no longer a prefix of what we would send now.
    fn covered_messages(&self, request: &ProviderRequest) -> usize {
        request
            .continuation
            .as_ref()
            .filter(|continuation| continuation.is_reusable_for(&self.config.identity))
            .and_then(|continuation| continuation.proven_covers(&request.messages))
            .unwrap_or(0)
    }

    /// How many input items one message becomes.
    ///
    /// Needed because the wire count is not the message count: an assistant
    /// turn that both spoke and called a tool is two items. The number is
    /// recorded beside the message prefix so a stored handle can be audited
    /// against what the protocol actually received, and it is never used to
    /// decide what to skip.
    fn item_count(&self, message: &ModelMessage) -> usize {
        match message {
            ModelMessage::Text { .. } | ModelMessage::ToolResult { .. } => 1,
            ModelMessage::Assistant { content, calls, .. } => {
                usize::from(content.as_deref().is_some_and(|text| !text.is_empty())) + calls.len()
            }
        }
    }

    /// The request body, and what this request will have made the endpoint hold.
    fn request_body(&self, request: ProviderRequest) -> (Value, SentCoverage) {
        let covered = self.covered_messages(&request);
        let coverage = SentCoverage::of(
            &request.messages,
            request
                .messages
                .iter()
                .map(|message| self.item_count(message))
                .sum(),
        );

        let input = request
            .messages
            .into_iter()
            .skip(covered)
            .flat_map(|message| self.encode_message(message))
            .collect::<Vec<_>>();

        let tools = request
            .tools
            .into_iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                })
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "model": self.config.model,
            "input": input,
        });

        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = Value::String("auto".into());
        }

        // The native handle. It replaces the covered prefix rather than sitting
        // on top of it, which is why the prefix is skipped above.
        //
        // Naming the handle is conditional on the same proof that skipped the
        // prefix, and that is the point: a handle is only named when the
        // endpoint's copy of this conversation is still a prefix of what we are
        // sending. Chaining onto a handle whose prompt has since changed would
        // silently prepend the endpoint's older, differently-shaped prefix to a
        // request that already re-sent the whole transcript.
        if covered > 0 {
            if let Some(response_id) = request
                .continuation
                .as_ref()
                .filter(|continuation| continuation.is_reusable_for(&self.config.identity))
                .and_then(|continuation| continuation.response_id.as_deref())
            {
                body["previous_response_id"] = Value::String(response_id.to_owned());
            }
        }

        if let Some(max_output_tokens) = self.config.max_output_tokens {
            body["max_output_tokens"] = json!(max_output_tokens);
        }
        if let Some(temperature) = self.config.temperature {
            body["temperature"] = json!(temperature);
        }

        (body, coverage)
    }

    /// One provider-neutral message may be several input items.
    ///
    /// An assistant turn that both said something and called a tool is a
    /// message item *and* a `function_call` item, because that is how this
    /// protocol represents it.
    fn encode_message(&self, message: ModelMessage) -> Vec<Value> {
        match message {
            ModelMessage::Text { role, content } => vec![json!({
                "role": self.encode_role(role),
                "content": content,
            })],
            ModelMessage::Assistant {
                content,
                calls,
                speaker: _,
                ..
            } => {
                // `reasoning` is deliberately dropped. On this transport the
                // reasoning is an output item the endpoint already holds, and
                // the handle we would have to replay it with is the same handle
                // covering the rest of this prefix. Re-sending it as text would
                // mean inventing a field this protocol does not have.
                //
                // `speaker` is dropped for the same reason. This protocol has
                // no field for who an assistant message speaks for, and putting
                // the id in the content would forge text the model did not
                // write. A multi-character conversation on this transport
                // therefore relies on the transcript to say who spoke; the
                // names are in the prompt's character definitions.
                let mut items = Vec::new();

                if let Some(content) = content.filter(|text| !text.is_empty()) {
                    items.push(json!({
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": content }],
                    }));
                }

                items.extend(calls.into_iter().map(|call| {
                    json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.arguments.to_string(),
                    })
                }));

                items
            }
            ModelMessage::ToolResult {
                call_id,
                name: _,
                content,
                structured_content,
                is_error,
            } => {
                let output = if content.is_empty() {
                    structured_content
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                } else if is_error {
                    json!({"error": content}).to_string()
                } else {
                    content
                };

                vec![json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                })]
            }
        }
    }

    fn encode_role(&self, role: ModelRole) -> &'static str {
        match role {
            ModelRole::System => "system",
            ModelRole::Developer => "developer",
            ModelRole::User => "user",
            ModelRole::Assistant => "assistant",
        }
    }
}

/// Read token accounting out of a Responses `usage` object.
///
/// The field names differ from Chat Completions on purpose: this transport
/// calls them `input_tokens` and `output_tokens` because it takes `input` and
/// produces `output`, not messages. Both spellings are read so an endpoint that
/// answers with either is still accounted for rather than silently reporting
/// nothing.
fn parse_usage(usage: &Value) -> Option<TokenUsage> {
    let read = |primary: &[&str], fallback: &[&str]| {
        primary
            .iter()
            .chain(fallback.iter())
            .find_map(|field| usage.get(*field).and_then(Value::as_u64))
    };

    let input = read(&["input_tokens"], &["prompt_tokens"]);
    let output = read(&["output_tokens"], &["completion_tokens"]);
    let cached = ["input_tokens_details", "prompt_tokens_details"]
        .iter()
        .find_map(|container| {
            usage
                .get(*container)
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_u64)
        });
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

#[derive(Default)]
struct FunctionCallAccumulator {
    call_id: String,
    name: String,
    arguments: String,
}

/// Accumulates one streamed response from server-sent events.
///
/// The transport free: it consumes decoded lines and emits normalized deltas.
#[derive(Default)]
struct ResponseStream {
    text: String,
    reasoning: String,
    response_id: Option<String>,
    status: Option<String>,
    usage: Option<TokenUsage>,
    calls: BTreeMap<usize, FunctionCallAccumulator>,
}

impl ResponseStream {
    fn push_line(&mut self, line: &str, sink: &mut dyn StreamSink) -> Result<(), ProviderError> {
        let Some(data) = line.trim().strip_prefix("data:") else {
            return Ok(());
        };

        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }

        let event: Value = serde_json::from_str(data)
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;

        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match kind {
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.text.push_str(delta);
                        sink.on_text_delta(delta);
                    }
                }
            }
            // The summary is the part of a reasoning item that is safe to show
            // and to keep; the encrypted remainder stays with the endpoint.
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.reasoning.push_str(delta);
                        sink.on_reasoning_delta(delta);
                    }
                }
            }
            "response.output_item.added" => {
                if let Some(item) = event.get("item") {
                    self.absorb_item(item, sink);
                }
            }
            "response.function_call_arguments.delta" => {
                let index = self.index_of(&event);
                let delta = event.get("delta").and_then(Value::as_str);
                self.push_arguments(index, delta, sink);
            }
            "response.function_call_arguments.done" => {
                // The completed arguments replace what arrived in fragments.
                // Keeping both would concatenate a whole JSON document onto its
                // own prefix.
                if let Some(arguments) = event.get("arguments").and_then(Value::as_str) {
                    let index = self.index_of(&event);
                    let entry = self.calls.entry(index).or_default();
                    if entry.call_id.is_empty() {
                        entry.call_id = format!("call-{index}");
                    }
                    entry.arguments = arguments.to_owned();
                }
            }
            "response.output_item.done" => {
                if let Some(item) = event.get("item") {
                    self.absorb_item(item, sink);
                }
            }
            "response.created" | "response.in_progress" => {
                if self.response_id.is_none() {
                    self.response_id = response_of(&event)
                        .and_then(|response| response.get("id").and_then(Value::as_str))
                        .map(str::to_owned);
                }
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                if let Some(response) = response_of(&event) {
                    if self.response_id.is_none() {
                        self.response_id = response
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                    if let Some(status) = response.get("status").and_then(Value::as_str) {
                        self.status = Some(status.to_owned());
                    }
                    if let Some(usage) = response.get("usage").and_then(parse_usage) {
                        self.usage = Some(usage);
                    }
                    // Backfill from the finished output. A stream that dropped
                    // a delta, or an endpoint that answers the whole item at
                    // once, still produces a complete turn.
                    for item in response
                        .get("output")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        self.absorb_item(item, sink);
                    }
                }
            }
            "error" => {
                let message = event
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("the endpoint reported an error mid-stream");
                return Err(ProviderError::InvalidResponse(message.to_owned()));
            }
            _ => {}
        }

        Ok(())
    }

    fn index_of(&self, event: &Value) -> usize {
        event
            .get("output_index")
            .and_then(Value::as_u64)
            .map(|index| index as usize)
            .unwrap_or(self.calls.len())
    }

    fn push_arguments(&mut self, index: usize, delta: Option<&str>, sink: &mut dyn StreamSink) {
        let requested = !self.calls.contains_key(&index);
        let entry = self.calls.entry(index).or_default();

        if entry.call_id.is_empty() {
            entry.call_id = format!("call-{index}");
        }
        if let Some(delta) = delta.filter(|delta| !delta.is_empty()) {
            entry.arguments.push_str(delta);
        }

        if requested {
            sink.on_tool_call_requested(&ToolCall {
                id: entry.call_id.clone(),
                name: entry.name.clone(),
                arguments: serde_json::Value::Null,
            });
        }
    }

    /// Take one output item, whichever event carried it.
    fn absorb_item(&mut self, item: &Value, sink: &mut dyn StreamSink) {
        let index = item
            .get("output_index")
            .and_then(Value::as_u64)
            .map(|index| index as usize)
            .unwrap_or(self.calls.len());

        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                let call_id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();

                let requested = !self.calls.contains_key(&index);
                let entry = self.calls.entry(index).or_default();
                if !call_id.is_empty() {
                    entry.call_id = call_id;
                }
                if !name.is_empty() {
                    entry.name = name;
                }
                if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
                    if !arguments.is_empty() {
                        entry.arguments = arguments.to_owned();
                    }
                }

                if requested {
                    sink.on_tool_call_requested(&ToolCall {
                        id: entry.call_id.clone(),
                        name: entry.name.clone(),
                        arguments: serde_json::Value::Null,
                    });
                }
            }
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if part.get("type").and_then(Value::as_str) == Some("output_text") {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.is_empty() && !self.text.contains(text) {
                                self.text.push_str(text);
                            }
                        }
                    }
                }
            }
            Some("reasoning") => {
                for part in item
                    .get("summary")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        if !text.is_empty() && !self.reasoning.contains(text) {
                            self.reasoning.push_str(text);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// How many assistant messages this response added to the endpoint's copy.
    ///
    /// One turn of this protocol is one assistant message, however many items it
    /// was split into. A response that produced nothing at all adds nothing, so
    /// its handle must not claim a message that was never created.
    fn produced_assistant_messages(text: &str, tool_calls: usize) -> usize {
        usize::from(!text.is_empty() || tool_calls > 0)
    }

    fn finish(
        self,
        config: &ResponsesConfig,
        sent: SentCoverage,
    ) -> Result<AssistantTurn, ProviderError> {
        let tool_calls = self
            .calls
            .into_values()
            .filter(|entry| !entry.name.is_empty())
            .map(|entry| {
                let arguments = if entry.arguments.trim().is_empty() {
                    Value::Object(serde_json::Map::new())
                } else {
                    serde_json::from_str(&entry.arguments).map_err(|error| {
                        ProviderError::InvalidResponse(format!(
                            "tool {} returned invalid JSON arguments: {error}",
                            entry.name
                        ))
                    })?
                };

                Ok(ToolCall {
                    id: entry.call_id,
                    name: entry.name,
                    arguments,
                })
            })
            .collect::<Result<Vec<_>, ProviderError>>()?;

        // This is the whole reason the transport exists. The response id is a
        // real handle: the endpoint still holds the conversation, so the next
        // round can name it instead of resending the prefix.
        //
        // What it holds is measured from *this* request, never carried over
        // from the handle this request continued from. That is the whole
        // difference between skipping the tool results and skipping nothing:
        // the new response absorbed every message sent to produce it, plus the
        // assistant message it generated.
        let assistant_messages =
            ResponseStream::produced_assistant_messages(&self.text, tool_calls.len());
        let continuation = ProviderContinuation::new(
            config.identity.clone(),
            ContinuationSupport::ResponseId,
            self.response_id,
        )
        .remembering_coverage(sent.as_coverage(assistant_messages));

        Ok(AssistantTurn {
            text: (!self.text.is_empty()).then_some(self.text),
            tool_calls,
            finish_reason: self.status,
            reasoning: (!self.reasoning.trim().is_empty())
                .then(|| ReasoningSidecar::new(self.reasoning.clone(), config.identity.clone())),
            continuation: ContinuationUpdate::Replace(continuation),
            usage: self.usage,
        })
    }
}

fn response_of(event: &Value) -> Option<&Value> {
    event
        .get("response")
        .filter(|response| response.is_object())
        .or_else(|| event.get("response"))
}

#[async_trait]
impl AiProvider for OpenAiResponsesProvider {
    async fn complete(&self, request: ProviderRequest) -> Result<AssistantTurn, ProviderError> {
        let (body, sent) = self.request_body(request);

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if !status.is_success() {
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: text,
            });
        }

        let response: ResponsesResponse = serde_json::from_str(&text)
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;

        let mut stream = ResponseStream::default();
        let mut sink = NullStreamSink;
        for item in &response.output {
            stream.absorb_item(item, &mut sink);
        }

        stream.response_id = response.id.clone();
        stream.status = response.status.clone();
        stream.usage = response.usage.as_ref().and_then(parse_usage);

        stream.finish(&self.config, sent)
    }

    async fn stream(
        &self,
        request: ProviderRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<AssistantTurn, ProviderError> {
        let (mut body, sent) = self.request_body(request);
        body["stream"] = json!(true);

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
        let mut stream = ResponseStream::default();

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?
        {
            pending.push_str(&String::from_utf8_lossy(&chunk));

            // A whole response can arrive in one chunk, so checking only between
            // chunks would let a cancelled turn keep streaming.
            while let Some(newline) = pending.find('\n') {
                let line = pending[..newline].to_string();
                pending.drain(..=newline);
                stream.push_line(&line, sink)?;

                if !sink.should_continue() {
                    return Err(ProviderError::Cancelled);
                }
            }

            if !sink.should_continue() {
                return Err(ProviderError::Cancelled);
            }
        }

        if !pending.is_empty() {
            stream.push_line(&pending, sink)?;
        }

        stream.finish(&self.config, sent)
    }
}

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    output: Vec<Value>,
    #[serde(default)]
    usage: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ToolAnnotations, ToolDefinition, ToolDiscovery};

    /// A sink that remembers what the transport announced.
    ///
    /// The transport free streaming parsers exist to normalize; this is the
    /// surface they normalize onto.
    #[derive(Default)]
    struct WatchedSink {
        text: String,
        reasoning: String,
        announced: usize,
    }

    impl StreamSink for WatchedSink {
        fn on_text_delta(&mut self, delta: &str) {
            self.text.push_str(delta);
        }

        fn on_reasoning_delta(&mut self, delta: &str) {
            self.reasoning.push_str(delta);
        }

        fn on_tool_call_requested(&mut self, _call: &ToolCall) {
            self.announced += 1;
        }
    }

    fn config() -> ResponsesConfig {
        ResponsesConfig::new("https://api.example.com/v1", "sk-test", "tester")
    }

    /// The body is a property of the provider, not of the config alone,
    /// because how much of the transcript a handle already covers is part of
    /// deciding the body.
    fn body(request: ProviderRequest) -> Value {
        OpenAiResponsesProvider::new(config())
            .request_body(request)
            .0
    }

    /// A handle as a real response would have produced it.
    ///
    /// Built the way the adapter builds one — from the messages the response
    /// actually absorbed — because a hand-written count is exactly what made
    /// this transport re-send input it already had.
    fn handle(
        covered: &[ModelMessage],
        response_id: &str,
        assistant_messages: usize,
    ) -> ProviderContinuation {
        ProviderContinuation::new(
            config().identity.clone(),
            ContinuationSupport::ResponseId,
            Some(response_id.into()),
        )
        .remembering_coverage(ContinuationCoverage {
            sent: covered.len(),
            digest: message_prefix_digest(covered),
            assistant_messages,
            wire_items: covered.len(),
        })
    }

    fn request(messages: Vec<ModelMessage>) -> ProviderRequest {
        ProviderRequest {
            messages,
            ..ProviderRequest::default()
        }
    }

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            title: None,
            description: "look something up".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
            output_schema: None,
            annotations: ToolAnnotations::default(),
            discovery: ToolDiscovery::default(),
        }
    }

    #[test]
    fn a_tool_call_and_its_result_are_separate_items() {
        // The whole point of this transport: a call is an item with a `call_id`,
        // and its result is the item carrying the same id.
        let body = body(request(vec![
            ModelMessage::user("find the tide"),
            ModelMessage::Assistant {
                speaker: None,
                content: None,
                reasoning: None,
                calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "search_context".into(),
                    arguments: json!({ "query": "tide" }),
                }],
            },
            ModelMessage::ToolResult {
                call_id: "call-1".into(),
                name: "search_context".into(),
                content: "high at four".into(),
                structured_content: None,
                is_error: false,
            },
        ]));

        let input = body["input"].as_array().expect("input items");

        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call-1");
        assert_eq!(input[1]["name"], "search_context");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call-1");
        assert_eq!(input[2]["output"], "high at four");
    }

    #[test]
    fn an_assistant_turn_that_also_spoke_becomes_two_items() {
        let body = body(request(vec![ModelMessage::Assistant {
            speaker: None,
            content: Some("let me check".into()),
            reasoning: None,
            calls: vec![ToolCall {
                id: "call-1".into(),
                name: "search_context".into(),
                arguments: json!({}),
            }],
        }]));

        let input = body["input"].as_array().expect("input items");

        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "assistant");
        assert_eq!(input[0]["content"][0]["type"], "output_text");
        assert_eq!(input[1]["type"], "function_call");
    }

    #[test]
    fn a_native_handle_replaces_the_prefix_it_already_holds() {
        // The point of the transport: a later request does not resend what the
        // endpoint already has, it names the conversation instead.
        let shared = vec![
            ModelMessage::system("you are a watchkeeper"),
            ModelMessage::user("find the tide"),
        ];

        // The endpoint's own answer sits after the messages it was sent, because
        // it produced it. Both are in the endpoint's copy of the conversation,
        // which is why the handle counts the answer separately.
        let mut next = request(vec![
            ModelMessage::system("you are a watchkeeper"),
            ModelMessage::user("find the tide"),
            ModelMessage::assistant("high at four"),
            ModelMessage::user("and tomorrow?"),
        ]);
        next.continuation = Some(handle(&shared, "resp_1", 1));

        let body = body(next);
        let input = body["input"].as_array().expect("input items");

        assert_eq!(body["previous_response_id"], "resp_1");
        // Only the message the endpoint has not seen.
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["content"], "and tomorrow?");
    }

    #[test]
    fn a_prompt_that_moved_is_sent_whole_instead_of_chained() {
        // The reason a handle carries a digest rather than only a count.
        //
        // A world-book entry matched on this turn, or a persona was edited, and
        // the shared prefix is now a different one. The endpoint still holds the
        // conversation as it was; naming its handle would splice that old shape
        // onto a request, so nothing is skipped and the handle is not named.
        let first = request(vec![
            ModelMessage::system("you are a watchkeeper"),
            ModelMessage::user("find the tide"),
        ]);

        let mut second = first.clone();
        second.messages[0] = ModelMessage::system("you are a lighthouse keeper");
        second.messages.push(ModelMessage::user("and tomorrow?"));
        second.continuation = Some(handle(&first.messages, "resp_1", 1));

        let body = body(second);

        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"].as_array().expect("input items").len(), 3);
    }

    #[test]
    fn a_bare_coverage_count_cannot_shorten_anything() {
        // A stored count from before coverage carried a digest says how many
        // messages were sent without saying what they were. It cannot prove the
        // same messages are in front of the model now, so it is not read.
        let messages = vec![
            ModelMessage::user("find the tide"),
            ModelMessage::user("and tomorrow?"),
        ];

        let mut continuation = ProviderContinuation::new(
            config().identity.clone(),
            ContinuationSupport::ResponseId,
            Some("resp_1".into()),
        );
        continuation.state.insert(
            sujiu_core::SENT_MESSAGES_KEY.to_owned(),
            Value::from(1usize),
        );

        let mut req = request(messages.clone());
        req.continuation = Some(continuation);

        let body = body(req);

        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"].as_array().expect("input items").len(), 2);
    }

    #[test]
    fn a_handle_from_another_endpoint_never_shortens_a_request() {
        let messages = vec![
            ModelMessage::user("find the tide"),
            ModelMessage::user("and tomorrow?"),
        ];

        let mut other = handle(&messages, "resp_1", 1);
        other.identity.base_url = "https://somewhere-else.example/v1".into();

        let mut request = request(messages);
        request.continuation = Some(other);

        let body = body(request);

        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"].as_array().expect("input items").len(), 2);
    }

    #[test]
    fn a_chat_completions_id_is_not_a_handle_this_transport_accepts() {
        let messages = vec![ModelMessage::user("one"), ModelMessage::user("two")];

        // Same endpoint and model, different protocol.
        let mut chat = handle(&messages, "resp_1", 1);
        chat.identity.protocol = Protocol::OpenAiChatCompletions;

        let mut request = request(messages);
        request.continuation = Some(chat);

        let body = body(request);

        assert!(body.get("previous_response_id").is_none());
        assert_eq!(body["input"].as_array().expect("input items").len(), 2);
    }

    #[test]
    fn tools_are_sent_in_the_shape_this_transport_defines() {
        let mut request = request(vec![ModelMessage::user("find the tide")]);
        request.tools = vec![tool("search_context")];

        let body = body(request);
        let tools = body["tools"].as_array().expect("tools");

        // Flat, not nested under `function`. That is the difference between
        // the two protocols' tool schema.
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "search_context");
        assert!(tools[0].get("function").is_none());
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn a_streamed_answer_and_its_reasoning_are_kept_apart() {
        let mut sink = WatchedSink::default();
        let mut stream = ResponseStream::default();

        for line in [
            r#"data: {"type":"response.created","response":{"id":"resp_1","status":"in_progress"}}"#,
            r#"data: {"type":"response.reasoning_summary_text.delta","delta":"counting"}"#,
            r#"data: {"type":"response.output_text.delta","delta":"The "}"#,
            r#"data: {"type":"response.output_text.delta","delta":"watch is unattended."}"#,
            r#"data: {"type":"response.completed","response":{"id":"resp_1","status":"completed","usage":{"input_tokens":40,"output_tokens":9,"input_tokens_details":{"cached_tokens":32}}}}"#,
        ] {
            stream.push_line(line, &mut sink).expect("a valid event");
        }

        let turn = stream
            .finish(&config(), SentCoverage::of(&[], 0))
            .expect("the turn completes");

        assert_eq!(turn.text.as_deref(), Some("The watch is unattended."));
        assert_eq!(
            turn.reasoning
                .as_ref()
                .map(|sidecar| sidecar.content.as_str()),
            Some("counting")
        );
        assert_eq!(sink.reasoning, "counting");
        assert_eq!(turn.usage.expect("usage").cached_input_tokens, Some(32));
        assert_eq!(
            turn.continuation
                .produced()
                .expect("a handle")
                .response_id
                .as_deref(),
            Some("resp_1")
        );
    }

    #[test]
    fn a_function_call_reassembles_from_fragments() {
        let mut sink = WatchedSink::default();
        let mut stream = ResponseStream::default();

        for line in [
            r#"data: {"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_9","name":"search_context","arguments":""}}"#,
            r#"data: {"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"query\":"}"#,
            r#"data: {"type":"response.function_call_arguments.delta","output_index":0,"delta":"\"tide\"}"}"#,
            r#"data: {"type":"response.function_call_arguments.done","output_index":0,"arguments":"{\"query\":\"tide\"}"}"#,
            r#"data: {"type":"response.completed","response":{"id":"resp_2","status":"completed"}}"#,
        ] {
            stream.push_line(line, &mut sink).expect("a valid event");
        }

        let turn = stream
            .finish(&config(), SentCoverage::of(&[], 0))
            .expect("the turn completes");

        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call_9");
        assert_eq!(turn.tool_calls[0].name, "search_context");
        assert_eq!(turn.tool_calls[0].arguments, json!({ "query": "tide" }));
        // Announced once, while the model was still streaming.
        assert_eq!(sink.announced, 1);
    }

    #[test]
    fn the_completed_output_backfills_what_never_streamed() {
        let mut sink = WatchedSink::default();
        let mut stream = ResponseStream::default();

        // No deltas at all: one endpoint answered the whole item at once.
        stream
            .push_line(
                r#"data: {"type":"response.completed","response":{"id":"resp_3","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"High at four."}]},{"type":"function_call","call_id":"call_1","name":"search_context","arguments":"{\"query\":\"tide\"}"}]}}"#,
                &mut sink,
            )
            .expect("a valid event");

        let turn = stream
            .finish(&config(), SentCoverage::of(&[], 0))
            .expect("the turn completes");

        assert_eq!(turn.text.as_deref(), Some("High at four."));
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call_1");
    }

    #[test]
    fn the_handle_records_what_this_response_actually_absorbed() {
        // Not what the previous handle covered. After this response the endpoint
        // holds the seven messages sent plus the assistant message it produced,
        // so the next round sends only what the endpoint has not seen.
        let sent = vec![
            ModelMessage::system("you are a watchkeeper"),
            ModelMessage::user("one"),
            ModelMessage::user("two"),
            ModelMessage::user("three"),
            ModelMessage::user("four"),
            ModelMessage::user("five"),
            ModelMessage::user("six"),
        ];

        let turn = ResponseStream {
            text: "seven it is".into(),
            response_id: Some("resp_4".into()),
            status: Some("completed".into()),
            ..ResponseStream::default()
        }
        .finish(&config(), SentCoverage::of(&sent, 8))
        .expect("the turn completes");

        let coverage = turn
            .continuation
            .produced()
            .expect("a handle")
            .coverage()
            .expect("coverage this transport can prove");

        assert_eq!(coverage.sent, 7);
        assert_eq!(coverage.digest, message_prefix_digest(&sent));
        assert_eq!(coverage.assistant_messages, 1);
        assert_eq!(coverage.wire_items, 8);
    }

    #[test]
    fn a_response_that_produced_nothing_claims_no_assistant_message() {
        let turn = ResponseStream {
            response_id: Some("resp_5".into()),
            status: Some("completed".into()),
            ..ResponseStream::default()
        }
        .finish(&config(), SentCoverage::of(&[], 0))
        .expect("the turn completes");

        assert_eq!(
            turn.continuation
                .produced()
                .expect("a handle")
                .coverage()
                .expect("coverage")
                .assistant_messages,
            0
        );
    }

    #[test]
    fn the_next_round_sends_only_what_the_endpoint_has_not_seen() {
        // Round two of a real tool loop, reconstructed from what round one
        // would have returned: seven messages in, one assistant message with a
        // call out, one result back. The endpoint holds the input and the call
        // it generated; it has never seen the result.
        let sent = vec![ModelMessage::user("find the tide")];

        let mut round_two = request(vec![
            ModelMessage::user("find the tide"),
            ModelMessage::Assistant {
                speaker: None,
                content: None,
                reasoning: None,
                calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "search_context".into(),
                    arguments: json!({ "query": "tide" }),
                }],
            },
            ModelMessage::ToolResult {
                call_id: "call_1".into(),
                name: "search_context".into(),
                content: "high at four".into(),
                structured_content: None,
                is_error: false,
            },
        ]);
        round_two.continuation = Some(handle(&sent, "resp_1", 1));

        let body = body(round_two);
        let input = body["input"].as_array().expect("input items");

        assert_eq!(body["previous_response_id"], "resp_1");
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_1");
    }

    #[test]
    fn an_error_event_stops_the_turn_instead_of_looking_like_a_short_answer() {
        let mut sink = WatchedSink::default();
        let mut stream = ResponseStream::default();

        // The error event itself has to be the failure. Treating it as one
        // more line to absorb would finish the turn as an empty answer, and an
        // endpoint that failed looks exactly like a model that said nothing.
        let error = stream
            .push_line(
                r#"data: {"type":"error","message":"the model is overloaded"}"#,
                &mut sink,
            )
            .expect_err("an error event is not a normal event");

        // The stream loop turns this into a failed turn; it never reaches the
        // point of reporting a short answer.
        assert!(error.to_string().contains("overloaded"), "{error}");
    }
}
