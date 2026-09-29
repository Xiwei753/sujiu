use async_trait::async_trait;
use thiserror::Error;

use crate::types::{AssistantTurn, ProviderRequest, ToolCall, ToolResult};

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("provider returned HTTP {status}: {body}")]
    Http { status: u16, body: String },

    #[error("provider response was invalid: {0}")]
    InvalidResponse(String),

    /// The consumer stopped listening. This is not a failure: it is how a
    /// cancelled turn is reported, and a provider must not be asked to
    /// disguise it as a completed one.
    #[error("turn cancelled")]
    Cancelled,
}

/// Receives normalized streaming output while a provider turn is in flight.
///
/// The sink is the only channel between a provider adapter and the agent loop.
/// Adapters must translate their wire format into these events, and must never
/// let wire-level details escape through the sink.
pub trait StreamSink: Send {
    /// One visible chunk of assistant text.
    fn on_text_delta(&mut self, delta: &str);

    /// Optional thinking/reasoning chunk. Platforms may render it collapsed.
    fn on_reasoning_delta(&mut self, _delta: &str) {}

    /// The model asked for a tool. The tool has not run yet, so this is what
    /// lets a platform show "waiting for tool" before execution starts.
    fn on_tool_call_requested(&mut self, _call: &ToolCall) {}

    /// The runtime is executing the tool now.
    fn on_tool_call_started(&mut self, _call: &ToolCall) {}

    /// A tool call finished, successfully or not. The result is reported so the
    /// UI can render tool state without knowing the tool implementation.
    fn on_tool_call_finished(&mut self, _call: &ToolCall, _result: &ToolResult) {}

    /// Returning `false` asks the adapter to stop producing output. The agent
    /// loop still returns the accumulated turn.
    fn should_continue(&self) -> bool {
        true
    }
}

/// A sink that drops everything, for callers that do not stream.
pub struct NullStreamSink;

impl StreamSink for NullStreamSink {
    fn on_text_delta(&mut self, _delta: &str) {}
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, request: ProviderRequest) -> Result<AssistantTurn, ProviderError>;

    /// Stream one assistant turn into `sink`.
    ///
    /// Adapters that cannot stream inherit this fallback, which performs the
    /// normal completion and reports the finished text as a single delta. That
    /// keeps streaming available to every platform without letting provider
    /// capabilities leak into the agent loop.
    async fn stream(
        &self,
        request: ProviderRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<AssistantTurn, ProviderError> {
        let turn = self.complete(request).await?;

        if let Some(text) = turn.text.as_deref() {
            if !text.is_empty() {
                sink.on_text_delta(text);
            }
        }

        // A non-streaming provider can still be cancelled once it answers, and
        // it must report that the same way a streaming one does.
        if !sink.should_continue() {
            return Err(ProviderError::Cancelled);
        }

        Ok(turn)
    }
}
