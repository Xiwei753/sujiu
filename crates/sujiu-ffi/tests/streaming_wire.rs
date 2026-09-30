//! Streaming and tool-loop behaviour against a wire-format server.
//!
//! The live provider test needs a credential. This one does not: it stands up a
//! minimal OpenAI-compatible SSE server on a loopback socket and drives the
//! whole runtime through it, so the wire format, the stream parser, the tool
//! loop and the event vocabulary are covered without a network.
//!
//! The server answers the first request with a streamed tool call and the
//! second, which carries the tool result, with streamed text. That is exactly
//! the round trip a real provider performs.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use serde_json::{json, Value};
use sujiu_core::{ProviderConfig, ProviderKind};
use sujiu_ffi::events::{TurnEvent, TurnEventKind, TurnEventReporter};
use sujiu_ffi::runtime::{SendTurnRequest, SujiuRuntime};

/// A request body the server received.
type Received = Value;

struct MockProvider {
    port: u16,
    requests: mpsc::Receiver<Received>,
    /// Capability probes, kept off the conversation channel so a test counting
    /// conversation rounds is not counting negotiation.
    probes: mpsc::Receiver<Received>,
    server: thread::JoinHandle<()>,
}
/// Whether a request is the runtime asking what this endpoint can do.
///
/// Negotiation is a real request, so a scripted server has to answer it. It is
/// recognised by its shape rather than by a marker the runtime sends, because
/// the runtime does not announce itself and a real endpoint cannot be expected
/// to. A probe is a one-token completion with a fixed greeting, which is the
/// smallest thing that still proves the route works.
fn is_capability_probe(request: &Value) -> bool {
    request["max_tokens"] == json!(1)
        && request["messages"].as_array().is_some_and(|messages| {
            messages.len() == 1
                && messages[0]["role"] == "user"
                && messages[0]["content"] == json!("ping")
        })
}

/// Answers a capability probe with the least a completion can be.
fn write_probe_response(stream: &mut TcpStream) {
    let body = json!({
        "id": "probe",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
    })
    .to_string();

    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
}

impl MockProvider {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel();
        let (probe_tx, probe_rx) = mpsc::channel();

        let server = thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let body = match read_request(&stream) {
                    Some(body) => body,
                    None => continue,
                };
                let Ok(value) = serde_json::from_str::<Value>(&body) else {
                    continue;
                };
                if is_capability_probe(&value) {
                    let _ = probe_tx.send(value);
                    let mut stream = stream;
                    write_probe_response(&mut stream);
                    continue;
                }

                let _ = tx.send(value.clone());
                write_response(&stream, &value);
            }
        });

        Self {
            port,
            requests: rx,
            probes: probe_rx,
            server,
        }
    }

    /// The next capability probe, if one arrives.
    fn next_probe(&self) -> Option<Received> {
        self.probes
            .recv_timeout(std::time::Duration::from_secs(10))
            .ok()
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn next_request(&self) -> Received {
        self.requests
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the runtime should have sent a request")
    }
}

/// Reads one HTTP request and returns its body.
fn read_request(stream: &TcpStream) -> Option<String> {
    let mut reader = BufReader::new(stream);
    let mut length = 0usize;
    let mut line = String::new();
    while reader.read_line(&mut line).ok()? > 0 {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok()?;
            }
        }
        line.clear();
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

/// Answers with a streamed tool call, or with streamed text once the tool
/// result is part of the conversation.
fn write_response(stream: &TcpStream, request: &Value) {
    let already_called_tool = request["messages"]
        .as_array()
        .map(|messages| messages.iter().any(|message| message["role"] == "tool"))
        .unwrap_or(false);

    let frames: Vec<String> = if already_called_tool {
        let text = "The night shift is quiet, and the console is the only thing awake.";
        let mut frames = text
            .split_inclusive(' ')
            .map(|word| sse(&json!({"choices": [{"delta": {"content": word}}]})))
            .collect::<Vec<_>>();
        frames.push(sse(
            &json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
        ));
        frames
    } else {
        vec![
            // A tool call arrives in fragments, exactly as a real stream does.
            sse(
                &json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call-1", "function": {"name": "search_context", "arguments": ""}}]}}]}),
            ),
            sse(
                &json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{\"query\":"}}]}}]}),
            ),
            sse(
                &json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"blinking light\"}"}}]}}]}),
            ),
            sse(&json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]})),
        ]
    };

    let mut body = String::new();
    for frame in frames {
        body.push_str(&frame);
    }
    body.push_str("data: [DONE]\n\n");

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut writer = stream;
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.flush();
}

fn sse(payload: &Value) -> String {
    format!("data: {payload}\n\n")
}

struct Recorder {
    events: Vec<TurnEvent>,
}

impl TurnEventReporter for Recorder {
    fn report(&mut self, event: TurnEvent) {
        self.events.push(event);
    }
}

fn runtime_for(base_url: &str) -> SujiuRuntime {
    let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
    runtime
        .set_provider_config(Some(ProviderConfig {
            id: "mock".to_string(),
            name: "Mock".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: base_url.to_string(),
            model: "mock-model".to_string(),
            credential_ref: None,
            extra: Default::default(),
        }))
        .expect("provider config");
    runtime
}

/// The runtime owns its own tokio runtime and the C ABI blocks on it, so a test
/// does the same instead of nesting a second runtime inside an async context.
fn run_turn(runtime: &SujiuRuntime, request: SendTurnRequest, sink: &mut dyn TurnEventReporter) {
    runtime.tokio.block_on(runtime.send_turn(request, sink));
}

#[test]
fn a_turn_streams_text_after_a_streamed_tool_call() {
    let provider = MockProvider::start();
    let runtime = runtime_for(&provider.base_url());

    let mut recorder = Recorder { events: Vec::new() };
    run_turn(
        &runtime,
        SendTurnRequest {
            session_id: "session-1".to_string(),
            user_text: "What is the blinking light?".to_string(),
            provider: None,
            api_key: Some("test-key".to_string()),
        },
        &mut recorder,
    );

    // The first request advertises tools rather than the whole catalog.
    let first = provider.next_request();
    assert_eq!(
        first["stream"],
        json!(true),
        "the adapter must ask for a stream"
    );
    assert_eq!(first["model"], json!("mock-model"));
    let tool_names: Vec<String> = first["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| {
            tool["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert!(
        tool_names.contains(&"search_context".to_string()),
        "context tools should be offered, got {tool_names:?}"
    );
    assert!(
        tool_names.len() < 30,
        "the whole catalog must not be dumped into one request, got {}",
        tool_names.len()
    );

    let kinds: Vec<TurnEventKind> = recorder.events.iter().map(|event| event.kind).collect();
    println!("kinds: {kinds:?}");

    assert_eq!(kinds.first(), Some(&TurnEventKind::TurnStarted));
    assert_eq!(kinds.last(), Some(&TurnEventKind::TurnCompleted));
    assert!(
        kinds.contains(&TurnEventKind::ToolCallRequested),
        "the model asking for a tool must be visible before the tool runs: {kinds:?}"
    );
    assert!(kinds.contains(&TurnEventKind::ToolCallStarted));
    assert!(kinds.contains(&TurnEventKind::ToolCallFinished));
    assert!(kinds.contains(&TurnEventKind::TextDelta));

    // Requested must come before started: that is what lets a UI show
    // "waiting for tool" while the model is still streaming.
    let requested = kinds
        .iter()
        .position(|kind| *kind == TurnEventKind::ToolCallRequested)
        .expect("requested");
    let started = kinds
        .iter()
        .position(|kind| *kind == TurnEventKind::ToolCallStarted)
        .expect("started");
    assert!(
        requested < started,
        "requested must precede started: {kinds:?}"
    );

    // The tool ran for real: the result is back in the request history.
    let second = provider.next_request();
    let tool_messages: Vec<&Value> = second["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect();
    assert!(
        !tool_messages.is_empty(),
        "the tool result must be sent back"
    );
    assert!(
        !tool_messages[0]["content"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "the tool result should carry content"
    );

    let answer: String = recorder
        .events
        .iter()
        .filter(|event| event.kind == TurnEventKind::TextDelta)
        .filter_map(|event| event.text.clone())
        .collect();
    assert!(
        answer.contains("console is the only thing awake"),
        "streamed deltas must be reassembled in order, got {answer:?}"
    );

    // The turn is persisted, so the transcript survives it.
    let state = runtime.conversation_state("session-1").expect("state");
    assert!(
        state
            .messages
            .iter()
            .any(|message| message.text.contains("console is the only thing awake")),
        "the assistant reply must be kept in the session"
    );

    // The recorded call carries the title the tool itself defines. The runtime
    // must not re-derive one from the tool id: `search_context` would come back
    // as "Search Context", which discards the curated name and puts English
    // casing in the runtime, where display text does not belong.
    let call = state
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .find(|call| call.name == "search_context")
        .expect("the tool call was recorded");
    assert_eq!(
        call.title, "Search context",
        "the tool's own title must pass through unchanged"
    );

    drop(runtime);
    drop(provider.server);
}

/// Cancels from inside the turn, which is the only moment a cancel means
/// something: both `send_turn` and `cancel` take `&self`, so a reporter can
/// hold the same runtime and stop the stream it is being fed by.
struct CancellingReporter<'a> {
    runtime: &'a SujiuRuntime,
    deltas: usize,
    events: Vec<TurnEvent>,
}

impl TurnEventReporter for CancellingReporter<'_> {
    fn report(&mut self, event: TurnEvent) {
        if event.kind == TurnEventKind::TextDelta {
            self.deltas += 1;
            if self.deltas == 3 {
                self.runtime.cancel();
            }
        }
        self.events.push(event);
    }
}

/// The protocol is negotiated against the endpoint before a streamed turn
/// starts, and that request must not be mistaken for a conversation round.
///
/// Before negotiation existed the runtime picked its transport from the
/// provider kind alone, so a streamed turn began with its first real message.
/// Now the first thing an endpoint sees is a minimal probe, and a client that
/// cannot tell the two apart either counts the probe as a turn or answers the
/// conversation with the probe's reply.
#[test]
fn a_streamed_turn_negotiates_before_it_streams() {
    let provider = MockProvider::start();
    let runtime = runtime_for(&provider.base_url());

    let mut sink = Recorder { events: Vec::new() };
    run_turn(
        &runtime,
        SendTurnRequest {
            session_id: "session-1".to_string(),
            user_text: "Say something.".to_string(),
            provider: None,
            api_key: Some("test-key".to_string()),
        },
        &mut sink,
    );

    let probe = provider
        .next_probe()
        .expect("the turn should have negotiated the protocol first");
    assert!(
        is_capability_probe(&probe),
        "a capability probe is a minimal request, not a conversation round: {probe:?}"
    );
    assert_eq!(
        probe["messages"].as_array().map(Vec::len),
        Some(1),
        "a probe does not replay the session: {probe:?}"
    );

    // The conversation is a separate request that arrives after the probe.
    let turn = provider.next_request();
    assert!(
        !is_capability_probe(&turn),
        "the conversation must not be a probe: {turn:?}"
    );
    assert_eq!(turn["stream"], serde_json::json!(true));
    let kinds: Vec<TurnEventKind> = sink.events.iter().map(|event| event.kind).collect();
    assert!(
        kinds.contains(&TurnEventKind::TurnCompleted),
        "the turn still streams and completes: {kinds:?}"
    );
}

#[test]
fn cancelling_mid_stream_stops_the_turn() {
    let provider = MockProvider::start();
    let runtime = runtime_for(&provider.base_url());

    let mut reporter = CancellingReporter {
        runtime: &runtime,
        deltas: 0,
        events: Vec::new(),
    };
    run_turn(
        &runtime,
        SendTurnRequest {
            session_id: "session-1".to_string(),
            user_text: "Anything.".to_string(),
            provider: None,
            api_key: Some("test-key".to_string()),
        },
        &mut reporter,
    );

    let kinds: Vec<TurnEventKind> = reporter.events.iter().map(|event| event.kind).collect();
    println!("cancelled kinds: {kinds:?}");

    assert_eq!(kinds.first(), Some(&TurnEventKind::TurnStarted));
    assert_eq!(
        kinds.last(),
        Some(&TurnEventKind::TurnCancelled),
        "a cancelled turn must end as cancelled: {kinds:?}"
    );
    assert_eq!(
        reporter.deltas, 3,
        "the stream must stop at the cancel, not keep reporting deltas: {kinds:?}"
    );
    assert!(
        !kinds.contains(&TurnEventKind::TurnCompleted),
        "a cancelled turn must not also complete: {kinds:?}"
    );

    // A cancelled turn is still committed. The tool call that already finished
    // is a step the model was actually shown, so the next request has to carry
    // it — and carry its result with it. Dropping the turn instead would make
    // the model re-ask for the same search, and would leave the caller with a
    // tool call that has no result.
    //
    // This uses a session that is empty to begin with, so the check is about
    // the cancelled turn rather than about whatever the seed shipped with.
    let empty = runtime.create_session(Some("character-lin"));
    let mut reporter = CancellingReporter {
        runtime: &runtime,
        deltas: 0,
        events: Vec::new(),
    };
    run_turn(
        &runtime,
        SendTurnRequest {
            session_id: empty.clone(),
            user_text: "Anything.".to_string(),
            provider: None,
            api_key: Some("test-key".to_string()),
        },
        &mut reporter,
    );
    let after = runtime.conversation_state(&empty).expect("state");
    let assistant = after
        .messages
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .expect("a cancelled turn keeps the steps that already finished");
    assert_eq!(
        assistant.tool_calls[0].id, "call-1",
        "the call id must survive unchanged so the next request can pair it: {assistant:?}"
    );
    assert_eq!(
        assistant.tool_calls[0].status, "completed",
        "a call that produced a result is completed, not interrupted: {assistant:?}"
    );

    drop(provider.server);
}
