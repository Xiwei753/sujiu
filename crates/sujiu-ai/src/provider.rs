use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

use crate::types::{AssistantTurn, ProviderRequest, ToolCall, ToolResult};

/// How long to wait for a connection, and how long to wait for the next byte.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// The HTTP client every provider call goes through.
///
/// A default client has no timeout at all, which turns a service that accepts a
/// connection and then says nothing into a turn that never ends: the user
/// watches a stop button they have to find and press themselves, and nothing in
/// the logs says why. An endpoint that has gone quiet is a fact about the
/// endpoint, and the runtime should be the one to notice it.
///
/// The read timeout is the gap between reads, not the length of the response,
/// so a long answer that keeps streaming is never cut off. Only silence counts.
///
/// The figure is a compromise between two failures that look identical on the
/// wire. A free-tier endpoint that queues a request for 60-80 seconds before
/// emitting its first byte sends exactly the same bytes as a socket that died,
/// and the runtime cannot tell them apart. 60 seconds cut off a working endpoint
/// that needed 62; 120 seconds is the measured compromise for it, paid for with
/// a dead endpoint taking twice as long to admit it. Raising it further trades
/// more of the second failure for less of the first, and the point where that
/// trade stops being worth it is a judgement call rather than a measurement.
pub fn http_client() -> reqwest::Client {
    http_client_with(READ_TIMEOUT)
}

/// The same client with a chosen read timeout.
///
/// Separated from [`http_client`] so the silence-is-bounded behaviour can be
/// proven in a test without waiting out the production figure.
pub fn http_client_with(read_timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(read_timeout)
        .build()
        .unwrap_or_default()
}

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

        // The turn completed, so it is returned even if the sink has stopped.
        // Throwing it away here would drop work the model actually did, and the
        // agent loop is what decides whether a stopped turn counts as finished.
        // An adapter that can still be interrupted mid-answer reports that as
        // `Cancelled` instead, because there is no complete turn to return.
        Ok(turn)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    /// A listener that accepts the connection and then never says anything.
    ///
    /// This is what a model endpoint does when it has accepted a request it is
    /// never going to answer, which is not a hypothetical: it is what a real
    /// gateway did when asked for a model its account could not reach.
    fn silent_endpoint() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        std::thread::spawn(move || {
            // Hold the connection open and read nothing into it, so the client
            // is waiting on a response rather than on a connection.
            if let Ok((stream, _)) = listener.accept() {
                let mut stream = stream;
                let mut buffer = [0u8; 1024];
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
                    }
                }
            }
        });
        base
    }

    #[tokio::test]
    async fn an_endpoint_that_stops_answering_becomes_an_error_rather_than_a_hang() {
        let base = silent_endpoint();
        let client = http_client_with(Duration::from_millis(300));
        let error = client
            .get(format!("{base}/v1/models"))
            .send()
            .await
            .expect_err("a silent endpoint must not return a response");

        // This is the shape the rest of the runtime already knows how to read:
        // a timeout is a transport problem, not a protocol the endpoint lacks.
        assert!(error.is_timeout(), "expected a timeout, got: {error}");
    }
}
