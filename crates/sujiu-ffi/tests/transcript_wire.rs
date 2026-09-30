//! What one turn leaves behind, checked against the next request.
//!
//! `streaming_wire` proves the turn loop works. This file proves the turn
//! *survives*: that the tool steps of a finished turn are still in the next
//! request, that a call and its result stay paired, that a turn which stopped
//! early is committed rather than discarded, and that nothing already sent is
//! rewritten on the way out.
//!
//! Everything runs against a loopback OpenAI-compatible server, so the wire
//! bodies are the real thing rather than a projection of it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use serde_json::{json, Value};
use sujiu_core::EndpointConfig;
use sujiu_ffi::events::{TurnEvent, TurnEventKind, TurnEventReporter};
use sujiu_ffi::runtime::{SendTurnRequest, SujiuRuntime};

/// One scripted answer. The server hands these out in order.
#[derive(Clone)]
enum Step {
    /// The model answers in words.
    Text(&'static str),
    /// The model asks for tools, in one step.
    Calls(&'static [(&'static str, &'static str, &'static str)]),
    /// The model thinks out loud and then asks for tools, which is what a
    /// thinking-mode endpoint does.
    ReasoningThenCalls(
        &'static str,
        &'static [(&'static str, &'static str, &'static str)],
    ),
    /// The model thinks out loud and then answers in words, with no tool call
    /// at all. The reasoning still has to come back next request.
    ReasoningThenText(&'static str, &'static str),
    /// The provider answers with an HTTP error.
    Http(u16),
    /// The provider claims to stream and then sends something that is not a
    /// stream at all, which is how a proxy failure usually shows up.
    Garbage,
}

struct ScriptedProvider {
    port: u16,
    requests: mpsc::Receiver<Value>,
    /// Capability probes, kept off the conversation channel so a test counting
    /// conversation rounds is not counting negotiation.
    probes: mpsc::Receiver<Value>,
    server: thread::JoinHandle<()>,
}

/// Whether a request is the runtime asking what this endpoint can do.
///
/// Negotiation is a real request, so a scripted server has to answer it, and it
/// has to answer it without consuming a scripted step. It is recognised by its
/// shape rather than by a marker, because the runtime does not announce itself
/// and a real endpoint cannot be expected to.
fn is_capability_probe(request: &Value) -> bool {
    request["max_tokens"] == json!(1)
        && request["messages"].as_array().is_some_and(|messages| {
            messages.len() == 1
                && messages[0]["role"] == "user"
                && messages[0]["content"] == json!("ping")
        })
}

/// Whether a request is the runtime probing for the newer Responses transport.
///
/// Negotiation tries that format first, so a scripted server has to be able to
/// say it does not have it. This mock serves the older chat protocol, and
/// answering "not here" is what makes the negotiation fall through to it.
fn is_responses_probe(request: &Value) -> bool {
    request["max_output_tokens"].is_number() && request["input"] == json!("ping")
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

/// A plain "that route is not here", which is the only answer that may move
/// negotiation on to the next format.
fn write_missing_route(stream: &mut TcpStream) {
    let body = json!({"error": {"message": "unknown endpoint", "type": "invalid_request_error"}})
        .to_string();

    let _ = write!(
        stream,
        "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
}

impl ScriptedProvider {
    /// `script` holds one step per request, in order. The last entry repeats,
    /// so a test that only cares about the first few requests does not have to
    /// predict how many there will be.
    fn start(script: Vec<Step>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel();
        let (probe_tx, probe_rx) = mpsc::channel();

        let server = thread::spawn(move || {
            let mut served = 0usize;

            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Some(body) = read_request(&stream) else {
                    continue;
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
                if is_responses_probe(&value) {
                    let _ = probe_tx.send(value);
                    let mut stream = stream;
                    write_missing_route(&mut stream);
                    continue;
                }

                let _ = tx.send(value);

                let step = script
                    .get(served)
                    .unwrap_or_else(|| script.last().expect("a non-empty script"))
                    .clone();
                served += 1;

                match step {
                    Step::Http(status) => write_http_error(&stream, status),
                    Step::Garbage => write_garbage(&stream),
                    answer => write_response(&stream, served, &answer),
                }
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
    fn next_probe(&self) -> Option<Value> {
        self.probes
            .recv_timeout(std::time::Duration::from_secs(10))
            .ok()
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn next_request(&self) -> Value {
        self.requests
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the runtime should have sent a request")
    }
}

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

/// Streams a step's tool calls, each split into fragments exactly as a real
/// stream delivers them, so reassembly is exercised rather than assumed.
fn write_tool_calls(frames: &mut Vec<String>, calls: &[(&str, &str, &str)]) {
    for (position, (id, name, arguments)) in calls.iter().enumerate() {
        frames.push(frame(&json!({"choices": [{"delta": {"tool_calls": [
            {"index": position, "id": id, "function": {"name": name, "arguments": ""}}
        ]}}]})));
        let split = arguments.len() / 2;
        let head: String = arguments.chars().take(split).collect();
        let tail: String = arguments.chars().skip(split).collect();
        frames.push(frame(&json!({"choices": [{"delta": {"tool_calls": [
            {"index": position, "function": {"arguments": head}}
        ]}}]})));
        frames.push(frame(&json!({"choices": [{"delta": {"tool_calls": [
            {"index": position, "function": {"arguments": tail}}
        ]}}]})));
    }
    frames.push(frame(
        &json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
    ));
}

/// Streams one step the way a real provider does, then reports usage so the
/// runtime can record what a cache read was worth.
fn write_response(stream: &TcpStream, index: usize, step: &Step) {
    let mut frames: Vec<String> = Vec::new();

    match step {
        // The failure steps never reach here: the server matches on them first
        // and answers with something that is not a stream.
        Step::Http(_) | Step::Garbage => return,
        Step::Text(text) => {
            for word in text.split_inclusive(' ') {
                frames.push(frame(&json!({"choices": [{"delta": {"content": word}}]})));
            }
            frames.push(frame(
                &json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
            ));
        }
        Step::ReasoningThenCalls(reasoning, calls) => {
            frames.push(frame(
                &json!({"choices": [{"delta": {"reasoning_content": reasoning}}]}),
            ));
            write_tool_calls(&mut frames, calls);
        }
        Step::ReasoningThenText(reasoning, text) => {
            frames.push(frame(
                &json!({"choices": [{"delta": {"reasoning_content": reasoning}}]}),
            ));
            for word in text.split_inclusive(' ') {
                frames.push(frame(&json!({"choices": [{"delta": {"content": word}}]})));
            }
            frames.push(frame(
                &json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
            ));
        }
        Step::Calls(calls) => write_tool_calls(&mut frames, calls),
    }

    frames.push(frame(&json!({
        "id": format!("resp-{index}"),
        "choices": [],
        "usage": {
            "prompt_tokens": 120,
            "completion_tokens": 12,
            "prompt_tokens_details": {"cached_tokens": 96}
        }
    })));

    let mut body = String::new();
    for payload in frames {
        body.push_str(&payload);
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

/// An ordinary provider-level failure, the kind a gateway returns when a
/// request is too large, rate limited, or simply broken.
fn write_http_error(stream: &TcpStream, status: u16) {
    let body =
        json!({"error": {"message": "upstream is unwell", "type": "server_error"}}).to_string();
    let response = format!(
        "HTTP/1.1 {status} Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut writer = stream;
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.flush();
}

/// A stream that stops being one half way through, which is what a dropped
/// connection looks like from the client side.
fn write_garbage(stream: &TcpStream) {
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 64\r\nConnection: close\r\n\r\nthis is not an event stream at all, not even close\r\n";
    let mut writer = stream;
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.flush();
}

fn frame(payload: &Value) -> String {
    format!("data: {payload}\n\n")
}

const SEARCH: Step = Step::Calls(&[("call-1", "search_context", r#"{"query":"blinking light"}"#)]);

#[derive(Default)]
struct Recorder {
    events: Vec<TurnEvent>,
}

impl TurnEventReporter for Recorder {
    fn report(&mut self, event: TurnEvent) {
        self.events.push(event);
    }
}

/// Cancels when a given event arrives, which is how a test stops a turn at a
/// precise point rather than only at the very beginning.
struct CancellingReporter<'a> {
    runtime: &'a SujiuRuntime,
    /// The event that triggers the cancellation.
    on: TurnEventKind,
    events: Vec<TurnEvent>,
    cancelled: bool,
}

impl TurnEventReporter for CancellingReporter<'_> {
    fn report(&mut self, event: TurnEvent) {
        if !self.cancelled && event.kind == self.on {
            self.cancelled = true;
            self.runtime.cancel();
        }
        self.events.push(event);
    }
}

fn runtime_for(base_url: &str, model: &str) -> SujiuRuntime {
    let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
    configure(&runtime, base_url, model);
    runtime
}

fn configure(runtime: &SujiuRuntime, base_url: &str, model: &str) {
    runtime
        .set_endpoint(Some(EndpointConfig {
            id: "mock".to_string(),
            name: "Mock".to_string(),
            base_url: base_url.to_string(),
            selected_model: Some(model.to_string()),
            credential_ref: None,
            overrides: Default::default(),
        }))
        .expect("provider config");
}

/// Points a runtime at a thinking-mode endpoint by declaring the capability.
///
/// The mock server is a loopback address, so no known endpoint profile applies
/// and the capability has to be stated. This is the product-level capability
/// key, not an adapter flag: the settings form and the bridge can both carry
/// it, and a known endpoint gets the same answer without it.
fn configure_thinking(runtime: &SujiuRuntime, base_url: &str, model: &str) {
    runtime
        .set_endpoint(Some(EndpointConfig {
            id: "mock".to_string(),
            name: "Mock".to_string(),
            base_url: base_url.to_string(),
            selected_model: Some(model.to_string()),
            credential_ref: None,
            overrides: serde_json::from_value(json!({
                sujiu_core::REPLAYS_ASSISTANT_REASONING_KEY: true
            }))
            .expect("the extra map is a serde value"),
        }))
        .expect("provider config");
}

fn thinking_runtime_for(base_url: &str, model: &str) -> SujiuRuntime {
    let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
    configure_thinking(&runtime, base_url, model);
    runtime
}

fn run_turn(runtime: &SujiuRuntime, session_id: &str, user_text: &str) -> Recorder {
    let mut recorder = Recorder::default();
    runtime.tokio.block_on(runtime.send_turn(
        SendTurnRequest {
            session_id: session_id.to_string(),
            user_text: user_text.to_string(),
            provider: None,
            api_key: Some("test-key".to_string()),
        },
        &mut recorder,
    ));
    recorder
}

/// The messages of a request, in wire order.
fn messages(request: &Value) -> Vec<Value> {
    request["messages"]
        .as_array()
        .expect("a request always carries messages")
        .clone()
}

fn text_of(message: &Value) -> String {
    message["content"].as_str().unwrap_or_default().to_string()
}

fn role_of(message: &Value) -> String {
    message["role"].as_str().unwrap_or_default().to_string()
}

fn tool_call_ids(message: &Value) -> Vec<String> {
    message["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .map(|call| call["id"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn roles_of(request: &Value) -> Vec<String> {
    messages(request).iter().map(role_of).collect()
}

/// The roles of the conversation itself, without the stable system prefix.
///
/// The prompt cache lives or dies on that prefix, so what a turn appends to it
/// is the part worth asserting on.
fn conversation_roles(request: &Value) -> Vec<String> {
    roles_of(request)
        .into_iter()
        .skip_while(|role| role == "system")
        .collect()
}

/// The tool results of a request, as `(tool_call_id, content)` pairs.
fn tool_results(request: &Value) -> Vec<(String, String)> {
    messages(request)
        .into_iter()
        .filter(|message| role_of(message) == "tool")
        .map(|message| {
            (
                message["tool_call_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                text_of(&message),
            )
        })
        .collect()
}

fn result_ids(request: &Value) -> Vec<String> {
    tool_results(request)
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

/// The `reasoning_content` of every assistant message in a request, in order.
///
/// Not just the tool-calling ones: an answer with no tool call still carries
/// its reasoning, and a helper that only looked at tool-call messages would
/// have hidden exactly the case that used to drop it.
fn reasoning_of(request: &Value) -> Vec<String> {
    messages(request)
        .into_iter()
        .filter(|message| message["reasoning_content"].is_string())
        .map(|message| {
            message["reasoning_content"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("sujiu-transcript-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

#[test]
fn a_second_turn_still_carries_the_tool_call_and_its_result() {
    let provider = ScriptedProvider::start(vec![
        SEARCH,
        Step::Text("The station keeps the light for a caller who has not spoken yet."),
    ]);
    let runtime = runtime_for(&provider.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the console light blinking?");
    let _ = provider.next_request();
    let _ = provider.next_request();

    // The turn that ends the tool loop is the one that shows the answer. What
    // the previous turn actually sends to the model is the next user turn.
    run_turn(&runtime, &session, "So who is on the other end?");
    let second = provider.next_request();

    // The second request is a continuation, not a fresh conversation.
    let body = messages(&second);
    let call_position = body
        .iter()
        .position(|message| !tool_call_ids(message).is_empty())
        .expect("the second request must still contain the assistant tool call");
    assert_eq!(
        tool_call_ids(&body[call_position]),
        vec!["call-1"],
        "the call id must be sent back unchanged: {body:?}"
    );

    let results = tool_results(&second);
    assert_eq!(results.len(), 1, "one call, one result: {results:?}");
    assert_eq!(
        results[0].0, "call-1",
        "the result must reference the call it answers: {results:?}"
    );
    assert!(
        results[0].1.contains("nightshift") || results[0].1.contains("blinking-caller"),
        "the result must be the real tool output, not a placeholder: {results:?}"
    );

    // Order: user, tool call, tool result, answer, then the new user message.
    assert_eq!(
        conversation_roles(&second),
        vec!["user", "assistant", "tool", "assistant", "user"],
        "the replayed turn must keep its order"
    );
    assert_eq!(
        text_of(body.last().expect("a request ends with the new input")),
        "So who is on the other end?",
        "the newest user message belongs at the tail"
    );

    drop(runtime);
    drop(provider.server);
}

#[test]
fn three_consecutive_tool_calls_are_all_paired_in_the_next_request() {
    let provider = ScriptedProvider::start(vec![
        SEARCH,
        Step::Calls(&[(
            "call-2",
            "read_context",
            r#"{"uri":"sujiu://context/lore/nightshift"}"#,
        )]),
        Step::Calls(&[("call-3", "list_context_sources", "{}")]),
        Step::Text("Three lookups later, the light is a caller holding the line open."),
    ]);
    let runtime = runtime_for(&provider.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Work out what the light means.");
    for _ in 0..4 {
        let _ = provider.next_request();
    }

    // As above, the finished turn only becomes part of a request once the user
    // speaks again.
    run_turn(&runtime, &session, "And who is waiting?");
    let last = provider.next_request();

    // Three assistant steps, each with its own call, each with its own result,
    // in the order the model asked for them.
    let call_ids: Vec<String> = messages(&last)
        .iter()
        .filter(|message| !tool_call_ids(message).is_empty())
        .flat_map(tool_call_ids)
        .collect();
    assert_eq!(
        call_ids,
        vec!["call-1", "call-2", "call-3"],
        "every call of the turn must still be there, in order"
    );
    assert_eq!(
        result_ids(&last),
        vec!["call-1", "call-2", "call-3"],
        "every call needs a result, in the same order"
    );
    assert_eq!(
        conversation_roles(&last),
        vec![
            "user",
            "assistant",
            "tool",
            "assistant",
            "tool",
            "assistant",
            "tool",
            "assistant",
            "user"
        ],
        "a three-step turn replays step by step"
    );

    drop(runtime);
    drop(provider.server);
}

#[test]
fn a_tool_error_is_still_paired_with_its_call() {
    let provider = ScriptedProvider::start(vec![
        Step::Calls(&[(
            "call-1",
            "read_context",
            r#"{"uri":"sujiu://context/lore/does-not-exist"}"#,
        )]),
        Step::Text("That record is not in the archive."),
    ]);
    let runtime = runtime_for(&provider.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    let recorder = run_turn(&runtime, &session, "Read me a shelf that is not there.");
    let _ = provider.next_request();
    let second = provider.next_request();

    let results = tool_results(&second);
    assert_eq!(
        results.len(),
        1,
        "a failed call still needs a result: {results:?}"
    );
    assert_eq!(results[0].0, "call-1");
    assert!(
        results[0].1.contains("error"),
        "the model has to be able to see that the call failed: {results:?}"
    );

    // The turn still answers, and the failure is visible to the platform.
    assert!(recorder
        .events
        .iter()
        .any(|event| event.kind == TurnEventKind::TurnCompleted));
    let state = runtime.conversation_state(&session).expect("state");
    let assistant = state
        .messages
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .expect("the failed call is still part of the conversation");
    assert_eq!(assistant.tool_calls[0].status, "failed");
    assert!(assistant.tool_calls[0].is_error);

    drop(runtime);
    drop(provider.server);
}

/// A turn whose tool round succeeded and whose next round hit an HTTP error is
/// the exact case that used to lose the whole turn. The call and its result
/// already reached the model, so dropping them would leave the next request
/// inconsistent with what the model has seen.
#[test]
fn a_provider_error_keeps_the_tool_round_that_already_succeeded() {
    for (label, failure) in [("http 500", Step::Http(500)), ("garbage", Step::Garbage)] {
        let provider = ScriptedProvider::start(vec![SEARCH, failure]);
        let runtime = runtime_for(&provider.base_url(), "mock-model");
        let session = runtime.create_session(Some("character-lin"));

        let recorder = run_turn(&runtime, &session, "Why is the light blinking?");
        let _ = provider.next_request();

        // The platform is told the turn failed, rather than being left hanging.
        assert!(
            recorder
                .events
                .iter()
                .any(|event| event.kind == TurnEventKind::TurnFailed),
            "{label}: a failed turn must still be reported to the platform"
        );

        // The completed tool round is committed.
        let state = runtime.conversation_state(&session).expect("state");
        let assistant = state
            .messages
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .unwrap_or_else(|| panic!("{label}: the tool round must be kept: {state:?}"));
        assert_eq!(assistant.tool_calls[0].id, "call-1", "{label}");
        assert_eq!(assistant.tool_calls[0].status, "completed", "{label}");
        assert!(!assistant.tool_calls[0].is_error, "{label}");

        // And the next turn still pairs that call with its result.
        run_turn(&runtime, &session, "Try again.");
        let next = provider.next_request();
        assert_eq!(
            result_ids(&next),
            vec!["call-1"],
            "{label}: the next request must still pair the call with its result"
        );

        drop(runtime);
        drop(provider.server);
    }
}

/// The same failure, but across a restart: the committed tool round has to
/// survive being written to disk, not just living in memory. A turn that
/// failed mid-loop is exactly the one most likely to be thrown away by a
/// naive "only persist successful turns" rule, so it is the one worth
/// reopening the app to check.
#[test]
fn a_turn_that_failed_mid_loop_still_survives_a_restart() {
    let directory = scratch_dir("failed-turn-restart");
    let path = directory.to_string_lossy().to_string();
    let session;

    {
        let provider = ScriptedProvider::start(vec![SEARCH, Step::Http(503)]);
        let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        runtime.use_directory(&path).expect("directory");
        configure(&runtime, &provider.base_url(), "mock-model");
        session = runtime.create_session(Some("character-lin"));

        run_turn(&runtime, &session, "Why is the light blinking?");
        let _ = provider.next_request();
    }

    // A brand new runtime, reading the same directory from disk.
    {
        let provider = ScriptedProvider::start(vec![Step::Text("Still here.")]);
        let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        runtime.use_directory(&path).expect("reopen");
        configure(&runtime, &provider.base_url(), "mock-model");

        let restored = runtime
            .conversation_state(&session)
            .expect("the session is restored");
        let assistant = restored
            .messages
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .unwrap_or_else(|| panic!("the committed call must survive the restart: {restored:?}"));
        assert_eq!(assistant.tool_calls[0].id, "call-1");
        assert_eq!(assistant.tool_calls[0].status, "completed");

        run_turn(&runtime, &session, "Try again.");
        let after = provider.next_request();
        assert_eq!(
            result_ids(&after),
            vec!["call-1"],
            "the next request must still pair the restored call with its result"
        );

        drop(runtime);
        drop(provider.server);
    }
}

#[test]
fn an_interrupted_call_is_committed_and_the_next_turn_continues_from_it() {
    // One step, two calls. The first runs, and the second never gets the chance:
    // this is the shape of a turn the user interrupted.
    let provider = ScriptedProvider::start(vec![Step::Calls(&[
        ("call-1", "search_context", r#"{"query":"blinking light"}"#),
        (
            "call-2",
            "read_context",
            r#"{"uri":"sujiu://context/lore/nightshift"}"#,
        ),
    ])]);
    let runtime = runtime_for(&provider.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    // Cancel the moment the first call reports back, so the second one is left
    // open by the model and never executed.
    let mut reporter = CancellingReporter {
        runtime: &runtime,
        on: TurnEventKind::ToolCallFinished,
        events: Vec::new(),
        cancelled: false,
    };
    runtime.tokio.block_on(runtime.send_turn(
        SendTurnRequest {
            session_id: session.clone(),
            user_text: "Why is the light blinking?".to_string(),
            provider: None,
            api_key: Some("test-key".to_string()),
        },
        &mut reporter,
    ));
    let _ = provider.next_request();

    assert!(
        reporter
            .events
            .iter()
            .any(|event| event.kind == TurnEventKind::TurnCancelled),
        "the turn reports itself as cancelled: {:?}",
        reporter.events
    );

    // Both calls are stored. The one that ran keeps its result, and the one
    // that did not carries a result saying so. Deleting either would leave the
    // next request with a call the model never saw answered.
    let after_cancel = runtime.conversation_state(&session).expect("state");
    let assistant = after_cancel
        .messages
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .expect("an interrupted call is still part of the conversation");
    let statuses: Vec<(&str, &str, bool)> = assistant
        .tool_calls
        .iter()
        .map(|call| (call.id.as_str(), call.status.as_str(), call.is_error))
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("call-1", "completed", false),
            ("call-2", "cancelled", true)
        ],
        "the finished call stays finished and the open one is marked as such"
    );

    // The next turn carries both, paired.
    run_turn(&runtime, &session, "Are you sure?");
    let next = provider.next_request();
    assert_eq!(
        result_ids(&next),
        vec!["call-1", "call-2"],
        "both calls must come back paired, in order: {next:?}"
    );
    let results = tool_results(&next);
    assert!(
        results[0].1.contains("nightshift") || results[0].1.contains("blinking-caller"),
        "the call that ran keeps its real result: {results:?}"
    );
    assert!(
        results[1].1.contains("cancel") || results[1].1.contains("not run"),
        "the result must explain that the call never ran: {results:?}"
    );

    drop(runtime);
    drop(provider.server);
}

#[test]
fn a_persisted_turn_still_reaches_the_model_after_a_reopen() {
    let provider = ScriptedProvider::start(vec![
        SEARCH,
        Step::Text("The station keeps the light for a caller who has not spoken yet."),
    ]);
    let directory = scratch_dir("reopen");
    let base_url = provider.base_url();

    let session = {
        let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        runtime
            .use_directory(directory.to_str().expect("utf-8 path"))
            .expect("directory");
        configure(&runtime, &base_url, "mock-model");
        let session = runtime.create_session(Some("character-lin"));
        run_turn(&runtime, &session, "Why is the console light blinking?");
        let _ = provider.next_request();
        let _ = provider.next_request();
        session
    };

    // A new runtime over the same directory is what a relaunch looks like.
    let reopened = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
    reopened
        .use_directory(directory.to_str().expect("utf-8 path"))
        .expect("directory");
    configure(&reopened, &base_url, "mock-model");

    run_turn(&reopened, &session, "Say more about the caller.");
    let after_restart = provider.next_request();
    assert_eq!(
        result_ids(&after_restart),
        vec!["call-1"],
        "a stored turn must reach the model after a relaunch"
    );

    let _ = std::fs::remove_dir_all(&directory);
    drop(reopened);
    drop(provider.server);
}

#[test]
fn a_compacted_session_still_pairs_every_call_the_model_can_see() {
    let provider = ScriptedProvider::start(vec![
        SEARCH,
        Step::Text("A caller who has not decided to speak."),
        Step::Calls(&[(
            "call-2",
            "read_context",
            r#"{"uri":"sujiu://context/lore/nightshift"}"#,
        )]),
        Step::Text("The station runs unattended."),
    ]);
    let runtime = runtime_for(&provider.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the light blinking?");
    for _ in 0..2 {
        let _ = provider.next_request();
    }
    run_turn(&runtime, &session, "Who keeps the shift?");
    for _ in 0..2 {
        let _ = provider.next_request();
    }

    // Compaction is allowed to rewrite history, so what matters afterwards is
    // that nothing it leaves behind breaks the protocol.
    assert!(
        runtime
            .compact_session(&session, 1, "The light is a caller holding the line.")
            .expect("compact"),
        "a two-turn session can be compacted down to one turn"
    );

    run_turn(&runtime, &session, "Is anyone there?");
    let after = provider.next_request();

    let roles = conversation_roles(&after);
    assert!(
        roles.first().map(String::as_str) == Some("user"),
        "the summary leads the history: {roles:?}"
    );
    assert!(
        text_of(&messages(&after)[3]).contains("holding the line"),
        "the model is told the earlier turns as a summary: {after:?}"
    );

    // The retained turn's call is still paired. Compaction cut on a turn
    // boundary, so it could not have separated a call from its result.
    assert_eq!(
        result_ids(&after),
        vec!["call-2"],
        "a call that survives compaction stays paired: {after:?}"
    );

    // And it refuses to rewrite a history that does not need it.
    assert!(
        !runtime
            .compact_session(&session, 8, "nothing to do")
            .expect("compact"),
        "compaction must not rewrite a history it does not need to touch"
    );

    drop(runtime);
    drop(provider.server);
}

#[test]
fn a_model_switch_stops_replaying_the_previous_continuation() {
    // The first model reports a response id and cache usage, so the runtime
    // stores continuation state that only model-a may reuse.
    let first_provider = ScriptedProvider::start(vec![
        SEARCH,
        Step::Text("A caller who has not decided to speak."),
    ]);
    let second_provider =
        ScriptedProvider::start(vec![Step::Text("Understood. The light is the wind.")]);

    let runtime = runtime_for(&first_provider.base_url(), "model-a");
    let session = runtime.create_session(Some("character-lin"));
    run_turn(&runtime, &session, "Why is the light blinking?");
    let _ = first_provider.next_request();
    let _ = first_provider.next_request();

    // The next turn runs on a different model.
    configure(&runtime, &second_provider.base_url(), "model-b");
    run_turn(&runtime, &session, "Are you sure?");
    let after_switch = second_provider.next_request();

    // Chat Completions has nowhere to put continuation state, so none may be
    // replayed. The transcript is what carries the conversation instead.
    assert!(
        after_switch.get("previous_response_id").is_none()
            && after_switch.get("response_id").is_none(),
        "no continuation may be replayed to a different model"
    );
    assert_eq!(
        result_ids(&after_switch),
        vec!["call-1"],
        "a model switch degrades to the transcript, not to a shorter history"
    );

    drop(runtime);
    drop(first_provider.server);
    drop(second_provider.server);
}

#[test]
fn a_second_turn_does_not_rewrite_what_the_first_turn_sent() {
    let provider = ScriptedProvider::start(vec![
        SEARCH,
        Step::Text("A caller who has not decided to speak."),
        Step::Calls(&[(
            "call-2",
            "read_context",
            r#"{"uri":"sujiu://context/lore/nightshift"}"#,
        )]),
        Step::Text("The station runs unattended after midnight."),
    ]);
    let runtime = runtime_for(&provider.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the light blinking?");
    let _ = provider.next_request();
    let second = provider.next_request();

    run_turn(&runtime, &session, "Who keeps the shift?");
    let _ = provider.next_request();
    let third = provider.next_request();

    // A prompt cache is only worth anything if the client leaves the prefix
    // alone, so the later request must start with everything the earlier one
    // started with, byte for byte.
    let second_body = messages(&second);
    let third_body = messages(&third);
    assert!(
        third_body.len() > second_body.len(),
        "the next turn only appends: {} then {}",
        second_body.len(),
        third_body.len()
    );
    assert_eq!(
        &third_body[..second_body.len()],
        &second_body[..],
        "a turn must be appended, never rewritten: the cache prefix changed"
    );

    // And the tool definitions are the same list in the same order, because a
    // reordered tool list invalidates the cached prefix just as much.
    assert_eq!(
        third["tools"], second["tools"],
        "the tool catalog and its order must stay stable across turns"
    );

    drop(runtime);
    drop(provider.server);
}

/// Thinking-mode endpoints reject a request whose earlier assistant tool call
/// comes back without the reasoning that produced it, so the reasoning a step
/// recorded has to be sent again on the next request. Storing it and never
/// replaying it would fail every such turn at the second round.
#[test]
fn the_reasoning_behind_a_tool_call_is_replayed_to_the_next_request() {
    const THINKING: Step = Step::ReasoningThenCalls(
        "The console light is a caller, so the context sources come first.",
        &[("call-1", "search_context", r#"{"query":"blinking light"}"#)],
    );
    let provider = ScriptedProvider::start(vec![
        THINKING,
        Step::Text("The light is a caller holding the line open."),
    ]);
    let runtime = thinking_runtime_for(&provider.base_url(), "deepseek-chat");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the console light blinking?");
    let _ = provider.next_request();
    let _ = provider.next_request();

    // As in the other tests, the finished turn only enters a request once the
    // user speaks again.
    run_turn(&runtime, &session, "So who is on the other end?");
    let second = provider.next_request();

    assert_eq!(
        reasoning_of(&second),
        vec!["The console light is a caller, so the context sources come first.".to_string()],
        "the reasoning that produced the call must be sent back: {second:?}"
    );

    drop(runtime);
    drop(provider.server);
}

/// The same reasoning, read back out of a persisted session, has to survive.
#[test]
fn replayed_reasoning_survives_a_restart() {
    const THINKING: Step = Step::ReasoningThenCalls(
        "The old king vanished beneath the western tower.",
        &[("call-1", "search_context", r#"{"query":"western tower"}"#)],
    );
    let provider = ScriptedProvider::start(vec![
        THINKING,
        Step::Text("The old king vanished beneath the western tower."),
    ]);

    let directory = scratch_dir("reasoning-restart");
    let session;
    {
        let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        let _ = runtime.use_directory(&directory.to_string_lossy());
        configure_thinking(&runtime, &provider.base_url(), "deepseek-chat");
        session = runtime.create_session(Some("character-wen"));
        run_turn(&runtime, &session, "What happened to the old king?");
        let _ = provider.next_request();
        let _ = provider.next_request();
    }

    {
        let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        let _ = runtime.use_directory(&directory.to_string_lossy());
        configure_thinking(&runtime, &provider.base_url(), "deepseek-chat");
        run_turn(&runtime, &session, "Who took the throne?");
        let second = provider.next_request();

        assert_eq!(
            reasoning_of(&second),
            vec!["The old king vanished beneath the western tower.".to_string()],
            "reasoning read back from a snapshot must still be replayed: {second:?}"
        );
    }

    drop(provider.server);
}

/// A thinking model that answers in words after thinking used to lose its
/// reasoning, because the sidecar only rode on the tool-calling shape of a
/// message. The endpoint still demands the reasoning back, so the next request
/// has to carry it even though no tool call is involved.
#[test]
fn an_answer_without_a_tool_call_still_replays_its_reasoning() {
    const THINKING: Step = Step::ReasoningThenText(
        "The harbour light answers on eleven minutes, like the compressor.",
        "The harbour light answers on eleven minutes.",
    );
    let provider = ScriptedProvider::start(vec![THINKING, Step::Text("The watch is unattended.")]);
    let runtime = thinking_runtime_for(&provider.base_url(), "deepseek-chat");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "When does the harbour light answer?");
    let _ = provider.next_request();

    run_turn(&runtime, &session, "And who keeps the watch?");
    let second = provider.next_request();

    assert_eq!(
        reasoning_of(&second),
        vec!["The harbour light answers on eleven minutes, like the compressor.".to_string()],
        "an answer with no tool call must still send its reasoning back: {second:?}"
    );

    drop(runtime);
    drop(provider.server);
}

/// The capability the settings screen saved has to reach the adapter, and the
/// per-turn request must not quietly drop it.
///
/// The runtime merges a per-turn provider over the saved one, so a turn that
/// resends an incomplete description used to override the saved capability
/// with a default and stop replaying reasoning on a perfectly good endpoint.
#[test]
fn a_saved_capability_reaches_the_adapter_through_an_ordinary_turn() {
    const THINKING: Step = Step::ReasoningThenCalls(
        "Eleven minutes, same as the compressor.",
        &[("call-1", "search_context", r#"{"query":"compressor"}"#)],
    );
    let provider = ScriptedProvider::start(vec![
        THINKING,
        Step::Text("Eleven minutes, same as the compressor."),
    ]);
    let runtime = thinking_runtime_for(&provider.base_url(), "deepseek-chat");
    let session = runtime.create_session(Some("character-lin"));

    // A per-turn provider that repeats only the fields a caller naturally has.
    let mut recorder = Recorder::default();
    runtime.tokio.block_on(runtime.send_turn(
        SendTurnRequest {
            session_id: session.clone(),
            user_text: "How long between compressor cycles?".to_string(),
            provider: Some(EndpointConfig {
                id: "mock".to_string(),
                name: "Mock".to_string(),
                base_url: provider.base_url(),
                selected_model: Some("deepseek-chat".to_string()),
                credential_ref: None,
                overrides: Default::default(),
            }),
            api_key: Some("test-key".to_string()),
        },
        &mut recorder,
    ));
    // The assertion has to be made on a request encoded under the PER-TURN
    // config, which is the second round of this turn: it is the only one whose
    // adapter was built from the merged provider. A later user turn goes back
    // to the stored config, so asserting there would pass even when the merge
    // threw the capability away.
    let _ = provider.next_request();
    let second_round = provider.next_request();
    assert_eq!(
        reasoning_of(&second_round),
        vec!["Eleven minutes, same as the compressor.".to_string()],
        "a turn that did not restate the capability must not lose it: {second_round:?}"
    );

    // Negotiation walked the protocol priority before that first round, and
    // every request in the walk is a minimal probe rather than a conversation
    // round. The first one is the newer transport, which this endpoint refuses,
    // and the second is the chat protocol it actually serves.
    let first = provider
        .next_probe()
        .expect("the first turn negotiates the protocol first");
    assert!(
        is_responses_probe(&first),
        "Responses is asked about first because it carries continuation and tool state natively: \
         {first:?}"
    );
    let second = provider
        .next_probe()
        .expect("a refused route is followed by the next protocol");
    assert!(
        is_capability_probe(&second),
        "the probe is a minimal request, not a conversation round: {second:?}"
    );

    // And the capability is still there for the turns after it.
    run_turn(&runtime, &session, "Was it always eleven minutes?");
    let _ = provider.next_request();

    drop(runtime);
    drop(provider.server);
}

/// A second compaction has to be told about the turns an earlier one archived,
/// otherwise the new summary cannot mention them and the older half of the
/// conversation silently leaves the model's view.
#[test]
fn a_second_compaction_is_given_the_older_turns_and_the_previous_summary() {
    const SEARCH_TWO: Step =
        Step::Calls(&[("call-1", "search_context", r#"{"query":"blinking light"}"#)]);
    let provider = ScriptedProvider::start(vec![
        SEARCH_TWO,
        Step::Text("The light is a caller holding the line."),
        SEARCH_TWO,
        Step::Text("The station runs unattended."),
    ]);
    let runtime = runtime_for(&provider.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the light blinking?");
    for _ in 0..2 {
        let _ = provider.next_request();
    }
    run_turn(&runtime, &session, "Who keeps the shift?");
    for _ in 0..2 {
        let _ = provider.next_request();
    }

    let first_input = runtime
        .compaction_input(&session, 1)
        .expect("compaction input")
        .expect("one turn is old enough to compact");
    assert_eq!(
        first_input.previous_summary, "",
        "nothing has been compacted yet, so there is no earlier summary"
    );
    assert_eq!(first_input.turns.len(), 1);
    assert!(
        first_input.to_prompt_text().contains("blinking"),
        "the input is usable as summarizer material: {:?}",
        first_input.to_prompt_text()
    );

    assert!(runtime
        .compact_session(&session, 1, "The light is a caller holding the line.")
        .expect("compact"));

    // A second compaction must be given the archived turn too, not only the
    // newly moved one.
    let second_input = runtime
        .compaction_input(&session, 0)
        .expect("compaction input")
        .expect("the retained turn is now old enough to compact as well");
    assert_eq!(
        second_input.previous_summary, "The light is a caller holding the line.",
        "the summary that already stands in for older turns has to be handed back"
    );
    assert_eq!(
        second_input.turns.len(),
        2,
        "both turns a new summary must cover: {:?}",
        second_input.turns
    );
    let prompt_text = second_input.to_prompt_text();
    assert!(prompt_text.contains("holding the line"), "{prompt_text}");
    assert!(
        prompt_text.contains("Who keeps the shift?"),
        "{prompt_text}"
    );
    assert!(
        prompt_text.contains("The station runs unattended."),
        "{prompt_text}"
    );

    drop(runtime);
    drop(provider.server);
}

/// A handle the provider retired has to stay retired across a restart.
///
/// The document below is the shape a transport with real continuation support
/// would leave behind: a turn whose first round replaced the handle and whose
/// second round cleared it. Chat Completions cannot produce that state itself,
/// because it has nowhere to put a handle, so the state is written the way such
/// a transport would have written it and then read back through the real
/// snapshot path.
///
/// What is being proved is that the session boundary preserves the *event*. If
/// loading or storing normalised `clear` into silence, the next turn would
/// search backwards, find the handle the clear retired, and believe it was
/// still live. The event surviving the round trip is exactly what stops that.
#[test]
fn a_handle_the_provider_retired_stays_retired_across_a_restart() {
    let provider = ScriptedProvider::start(vec![Step::Text("The line is dead. For now.")]);
    let directory = scratch_dir("cleared-handle-restart");
    let session = "session-cleared-handle";

    let document = json!({
        "version": 2,
        "characters": [],
        "sessions": [{
            "id": session,
            "characterId": "character-lin",
            "transcript": {
                "turns": [{
                    "id": "turn-1",
                    "user": "Is anyone there?",
                    "state": "completed",
                    "steps": [
                        {
                            "text": "Looking.",
                            "tool_calls": [],
                            "continuation": { "replace": {
                                "identity": {
                                    "kind": "open_ai_compatible",
                                    "provider_id": "chainable",
                                    "base_url": "https://chainable.example/v1",
                                    "model": "model-a"
                                },
                                "support": "response_id",
                                "response_id": "resp-retired"
                            } }
                        },
                        {
                            "text": "Still here.",
                            "tool_calls": [],
                            "continuation": "clear"
                        }
                    ]
                }]
            },
            "metadata": {}
        }],
        "sources": [],
        "records": [],
        "providerConfig": null
    });
    std::fs::write(directory.join("sujiu-runtime.json"), document.to_string())
        .expect("write the stored session");

    let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
    let _ = runtime.use_directory(&directory.to_string_lossy());
    configure(&runtime, &provider.base_url(), "model-a");

    run_turn(&runtime, session, "Say something.");
    let _ = provider.next_request();

    // The runtime rewrites the document on every persisted turn, so reading it
    // back now shows what the session boundary kept.
    let stored: Value = serde_json::from_str(
        &std::fs::read_to_string(directory.join("sujiu-runtime.json")).expect("read back"),
    )
    .expect("the stored document is still valid json");

    let steps = &stored["sessions"][0]["transcript"]["turns"][0]["steps"];
    assert_eq!(
        steps[0]["continuation"]["replace"]["response_id"], "resp-retired",
        "the handle that was produced is still recorded: {stored}"
    );
    assert_eq!(
        steps[1]["continuation"], "clear",
        "the answer that retired it has to survive being stored and reloaded, \
         otherwise the next turn finds the dead handle and believes it is live: \
         {stored}"
    );

    drop(runtime);
    drop(provider.server);
}
