//! What the Responses transport actually puts on the wire.
//!
//! `transcript_wire` proves a conversation survives between turns on a Chat
//! Completions endpoint, which has no continuation handle at all. This file
//! looks at the transport that *does* have one, because that is where the
//! interesting claims live and where they can be got wrong quietly:
//!
//! - a second round names the handle and sends only what the endpoint has not
//!   seen, so nothing is billed twice;
//! - a handle is used only while the prompt it was recorded against is still the
//!   prompt in front of the model, and is dropped the moment it is not;
//! - provider state is matched against a protocol the endpoint actually said it
//!   speaks, never against a default;
//! - a conversation that stops fitting is compacted by the kernel, on a turn
//!   boundary, with every call still paired with its result;
//! - and every round of a turn is written into a ledger somebody can audit.
//!
//! Every claim is checked against a real HTTP body captured off a loopback
//! server. Nothing here asserts against state a test injected: a handle that was
//! recorded by hand would prove only that the reader agrees with the writer.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use serde_json::{json, Value};
use sujiu_core::{CompactionPolicy, EndpointConfig};
use sujiu_runtime::events::TurnEventReporter;
use sujiu_runtime::runtime::{SendTurnRequest, SujiuRuntime};

/// Token accounting one scripted response reports.
///
/// Per step rather than per server, because the whole point of the ledger is
/// that a turn is several requests and the number that gets billed is the sum of
/// what each of them said it cost.
#[derive(Clone, Copy)]
struct Usage {
    input: u64,
    output: u64,
    cached: Option<u64>,
    cache_write: Option<u64>,
}

impl Usage {
    const fn plain(input: u64, output: u64) -> Self {
        Self {
            input,
            output,
            cached: None,
            cache_write: None,
        }
    }

    const fn cached(input: u64, cached: u64, output: u64) -> Self {
        Self {
            input,
            output,
            cached: Some(cached),
            cache_write: None,
        }
    }

    fn json(self) -> Value {
        let mut usage = json!({ "input_tokens": self.input, "output_tokens": self.output });
        if let Some(cached) = self.cached {
            usage["input_tokens_details"] = json!({ "cached_tokens": cached });
        }
        if let Some(cache_write) = self.cache_write {
            usage["cache_creation_input_tokens"] = json!(cache_write);
        }
        usage
    }
}

/// One scripted answer. The server hands these out in order.
#[derive(Clone)]
enum Step {
    /// The model answers in words.
    Text { text: &'static str, usage: Usage },
    /// The model asks for tools, in one step.
    Calls {
        calls: &'static [(&'static str, &'static str, &'static str)],
        usage: Usage,
    },
}

impl Step {
    fn usage(&self) -> Usage {
        match self {
            Step::Text { usage, .. } | Step::Calls { usage, .. } => *usage,
        }
    }
}

/// Which protocol the mock answers on.
///
/// Switchable, because "the endpoint stopped speaking Responses" is only
/// interesting when the endpoint id, the base URL and the model are all exactly
/// what they were: then the protocol is the one thing that changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Serving {
    Responses,
    Chat,
}

/// A request as it arrived, with the route it arrived on.
#[derive(Debug, Clone)]
struct Captured {
    path: String,
    body: Value,
}

struct Mock {
    port: u16,
    /// Conversation requests, in arrival order.
    ///
    /// Buffered, so a test can read them after the fact. An unbuffered channel
    /// would block the server mid-response and hang the runtime it is serving.
    requests: mpsc::Receiver<Captured>,
    /// Negotiation probes, kept on their own channel so a test counting
    /// conversation rounds is not counting negotiation.
    probes: mpsc::Receiver<Captured>,
    responses: Arc<AtomicBool>,
    /// Answer every un-streamed request with a provider error.
    ///
    /// The only un-streamed conversation request the runtime makes is the one it
    /// makes to summarize a conversation, so this is how a summary is made to
    /// fail — which is the case where archiving turns anyway would destroy them.
    break_summaries: Arc<AtomicBool>,
    server: thread::JoinHandle<()>,
}

impl Mock {
    fn start(script: Vec<Step>) -> Self {
        Self::start_serving(script, Serving::Responses)
    }

    fn start_serving(script: Vec<Step>, serving: Serving) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::sync_channel(256);
        let (probe_tx, probe_rx) = mpsc::sync_channel(32);
        let responses = Arc::new(AtomicBool::new(serving == Serving::Responses));
        let serving_flag = responses.clone();
        let break_summaries = Arc::new(AtomicBool::new(false));
        let break_flag = break_summaries.clone();

        let server = thread::spawn(move || {
            let mut served = 0usize;

            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Some(captured) = read_request(&stream) else {
                    continue;
                };
                let path = captured.path.clone();
                let body = captured.body.clone();
                let on_responses = serving_flag.load(Ordering::SeqCst);

                if is_probe(&body) {
                    let _ = probe_tx.send(captured);
                    let mut stream = stream;
                    // A probe is answered for the route it asked about, not for
                    // the one the mock happens to prefer: negotiation asks about
                    // /responses first, so a Chat-only endpoint has to be able to
                    // answer "not here" or nothing after it is ever reached.
                    match (on_responses, path.ends_with("/responses")) {
                        (true, true) => write_probe_response(&mut stream),
                        (false, false) => write_chat_probe_response(&mut stream),
                        _ => write_missing_route(&mut stream),
                    }
                    continue;
                }

                // The kernel asking for a summary of turns it is about to archive
                // is not a round of the conversation, so it does not take one from
                // the script. Taking one would hand every later turn the wrong
                // step and the test would end up measuring the mock.
                if is_summarizer(&body) {
                    let _ = tx.send(captured);
                    let mut stream = stream;
                    if break_flag.load(Ordering::SeqCst) {
                        write_provider_error(&mut stream);
                    } else if on_responses {
                        write_responses_json(&mut stream, 0, &SUMMARY);
                    } else {
                        write_chat_json(&mut stream, 0, &SUMMARY);
                    }
                    continue;
                }

                let step = script
                    .get(served)
                    .unwrap_or_else(|| script.last().expect("a non-empty script"))
                    .clone();
                served += 1;
                let _ = tx.send(captured);
                let mut stream = stream;

                // A route the mock is not serving gets the only answer that may
                // move negotiation on: "that path is not here", naming the path.
                match (on_responses, path.ends_with("/responses")) {
                    (true, true) | (false, false) => {
                        if body["stream"] == json!(true) {
                            if on_responses {
                                write_responses_stream(&mut stream, served, &step);
                            } else {
                                write_chat_stream(&mut stream, served, &step);
                            }
                        } else if on_responses {
                            write_responses_json(&mut stream, served, &step);
                        } else {
                            write_chat_json(&mut stream, served, &step);
                        }
                    }
                    _ => write_missing_route(&mut stream),
                }
            }
        });

        Self {
            port,
            requests: rx,
            probes: probe_rx,
            responses,
            break_summaries,
            server,
        }
    }

    /// Change what the endpoint claims to speak, without moving it.
    fn serve(&self, serving: Serving) {
        self.responses
            .store(serving == Serving::Responses, Ordering::SeqCst);
    }

    /// Make every request that is not a stream fail at the provider.
    fn break_summaries(&self) {
        self.break_summaries.store(true, Ordering::SeqCst);
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn next_request(&self) -> Captured {
        self.requests
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the runtime should have sent a request")
    }

    fn next_probe(&self) -> Captured {
        self.probes
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("negotiation should have asked the endpoint something")
    }

    fn try_next_probe(&self) -> Option<Captured> {
        self.probes.try_recv().ok()
    }

    /// Everything sent so far, without waiting.
    fn drain_requests(&self) -> Vec<Captured> {
        let mut drained = Vec::new();
        while let Ok(captured) = self.requests.try_recv() {
            drained.push(captured);
        }
        drained
    }
}

/// Whether a request is the runtime asking what this endpoint can do.
///
/// Recognised by shape rather than by a marker, because the runtime does not
/// announce itself and a real endpoint cannot be expected to.
fn is_probe(body: &Value) -> bool {
    let pings = |message: &Value| message["role"] == "user" && message["content"] == json!("ping");
    (body["max_output_tokens"].is_number() && body["input"] == json!("ping"))
        || (body["max_tokens"] == json!(1)
            && body["messages"]
                .as_array()
                .is_some_and(|messages| messages.len() == 1 && pings(&messages[0])))
}

/// Whether a request is the kernel folding archived turns into a summary.
///
/// It is a real billed request, so it is answered by the mock and counted by the
/// test — but it is not a conversation round. What marks it is the absence of
/// tools and of streaming, which is also what the compaction diagnostic claims
/// about it, so a test that identifies it this way is checking the same property
/// the runtime documents.
fn is_summarizer(body: &Value) -> bool {
    body["stream"] != json!(true) && body.get("tools").is_none()
}

fn read_request(stream: &TcpStream) -> Option<Captured> {
    let mut reader = BufReader::new(stream);
    let mut length = 0usize;
    let mut path = String::new();
    let mut first = true;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if first {
            // "POST /v1/responses HTTP/1.1"
            path = trimmed
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            first = false;
            continue;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok()?;
            }
        }
    }

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Captured {
        path,
        body: serde_json::from_slice(&body).ok()?,
    })
}

fn frame(payload: &Value) -> String {
    format!("data: {payload}\n\n")
}

fn write_sse(stream: &mut TcpStream, frames: &[String]) {
    let mut body = frames.concat();
    body.push_str("data: [DONE]\n\n");
    write_raw(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream",
        &body,
    );
}

fn write_json(stream: &mut TcpStream, body: &Value) {
    write_raw(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json",
        &body.to_string(),
    );
}

fn write_raw(stream: &mut TcpStream, head: &str, body: &str) {
    let response = format!(
        "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// An ordinary provider-level failure, the kind a gateway returns for a request
/// it will not serve.
fn write_provider_error(stream: &mut TcpStream) {
    write_raw(
        stream,
        "HTTP/1.1 502 Bad Gateway\r\nContent-Type: application/json",
        &json!({"error": {"message": "upstream is unwell", "type": "server_error"}}).to_string(),
    );
}

/// Answers a capability probe with the least a completion can be.
fn write_probe_response(stream: &mut TcpStream) {
    write_json(
        stream,
        &json!({
            "id": "probe-resp",
            "status": "completed",
            "output": [{"type": "message", "role": "assistant",
                        "content": [{"type": "output_text", "text": "pong"}]}],
            "usage": {"input_tokens": 4, "output_tokens": 1},
        }),
    );
}

fn write_chat_probe_response(stream: &mut TcpStream) {
    write_json(
        stream,
        &json!({
            "id": "probe",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"},
                         "finish_reason": "stop"}],
        }),
    );
}

/// A plain "that route is not here", which is the only answer that may move
/// negotiation on to the next format.
fn write_missing_route(stream: &mut TcpStream) {
    write_raw(
        stream,
        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json",
        &json!({"error": {"message": "unknown endpoint /v1/responses",
                          "type": "invalid_request_error"}})
        .to_string(),
    );
}

fn write_responses_stream(stream: &mut TcpStream, index: usize, step: &Step) {
    let id = format!("resp-{index}");
    let mut frames = vec![frame(&json!({
        "type": "response.created",
        "response": {"id": id, "status": "in_progress"},
    }))];

    match step {
        Step::Text { text, .. } => {
            frames.push(frame(&json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {"type": "message", "role": "assistant", "content": []},
            })));
            // Split the way a real stream delivers it, so reassembly is exercised
            // rather than assumed.
            for word in text.split_inclusive(' ') {
                frames.push(frame(
                    &json!({"type": "response.output_text.delta", "delta": word}),
                ));
            }
        }
        Step::Calls { calls, .. } => {
            for (position, (call_id, name, arguments)) in calls.iter().enumerate() {
                frames.push(frame(&json!({
                    "type": "response.output_item.added",
                    "output_index": position,
                    "item": {"type": "function_call", "call_id": call_id, "name": name},
                })));
                let split = arguments.len() / 2;
                let head: String = arguments.chars().take(split).collect();
                let tail: String = arguments.chars().skip(split).collect();
                frames.push(frame(&json!({
                    "type": "response.function_call_arguments.delta",
                    "output_index": position,
                    "delta": head,
                })));
                frames.push(frame(&json!({
                    "type": "response.function_call_arguments.delta",
                    "output_index": position,
                    "delta": tail,
                })));
                frames.push(frame(&json!({
                    "type": "response.function_call_arguments.done",
                    "output_index": position,
                    "arguments": arguments,
                })));
            }
        }
    }

    frames.push(frame(&json!({
        "type": "response.completed",
        "response": {"id": id, "status": "completed", "usage": step.usage().json()},
    })));

    write_sse(stream, &frames);
}

/// The un-streamed answer, which is what a non-streaming request and the
/// summarizer both get.
fn write_responses_json(stream: &mut TcpStream, index: usize, step: &Step) {
    let output = match step {
        Step::Text { text, .. } => json!([{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text}],
        }]),
        Step::Calls { calls, .. } => Value::Array(
            calls
                .iter()
                .map(|(call_id, name, arguments)| {
                    json!({"type": "function_call", "call_id": call_id, "name": name,
                           "arguments": arguments})
                })
                .collect(),
        ),
    };

    write_json(
        stream,
        &json!({
            "id": format!("resp-{index}"),
            "status": "completed",
            "output": output,
            "usage": step.usage().json(),
        }),
    );
}

fn write_chat_stream(stream: &mut TcpStream, index: usize, step: &Step) {
    let mut frames: Vec<String> = Vec::new();

    match step {
        Step::Text { text, .. } => {
            for word in text.split_inclusive(' ') {
                frames.push(frame(&json!({"choices": [{"delta": {"content": word}}]})));
            }
            frames.push(frame(
                &json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
            ));
        }
        Step::Calls { calls, .. } => {
            for (position, (call_id, name, arguments)) in calls.iter().enumerate() {
                frames.push(frame(&json!({"choices": [{"delta": {"tool_calls": [
                    {"index": position, "id": call_id, "function": {"name": name, "arguments": ""}}
                ]}}]})));
                frames.push(frame(&json!({"choices": [{"delta": {"tool_calls": [
                    {"index": position, "function": {"arguments": arguments}}
                ]}}]})));
            }
            frames.push(frame(
                &json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
            ));
        }
    }

    frames.push(frame(&json!({
        "id": format!("chat-{index}"),
        "choices": [],
        "usage": {
            "prompt_tokens": step.usage().input,
            "completion_tokens": step.usage().output,
        },
    })));

    write_sse(stream, &frames);
}

fn write_chat_json(stream: &mut TcpStream, index: usize, step: &Step) {
    let message = match step {
        Step::Text { text, .. } => json!({"role": "assistant", "content": text}),
        Step::Calls { calls, .. } => json!({
            "role": "assistant",
            "content": Value::Null,
            "tool_calls": calls.iter().map(|(call_id, name, arguments)| json!({
                "id": call_id,
                "type": "function",
                "function": {"name": name, "arguments": arguments},
            })).collect::<Vec<_>>(),
        }),
    };

    write_json(
        stream,
        &json!({
            "id": format!("chat-{index}"),
            "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": step.usage().input,
                "completion_tokens": step.usage().output,
            },
        }),
    );
}

// ---------------------------------------------------------------------------
// Reading a captured request
// ---------------------------------------------------------------------------

/// The input items of a Responses request, in wire order.
fn items(request: &Captured) -> Vec<Value> {
    request.body["input"]
        .as_array()
        .expect("a Responses request carries input items")
        .clone()
}

/// The messages of a Chat Completions request, in wire order.
fn messages(request: &Captured) -> Vec<Value> {
    request.body["messages"]
        .as_array()
        .expect("a Chat Completions request carries messages")
        .clone()
}

fn item_of(item: &Value) -> &str {
    item["type"].as_str().unwrap_or_default()
}

fn call_ids(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter(|item| item_of(item) == "function_call")
        .map(|item| item["call_id"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn result_ids(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter(|item| item_of(item) == "function_call_output")
        .map(|item| item["call_id"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Every `function_call` item has its `function_call_output` in the same input.
///
/// Only meaningful for a request that carried the whole conversation: a request
/// that named a handle deliberately left the calls it already made in the
/// endpoint's copy.
fn assert_every_call_is_answered(items: &[Value], label: &str) {
    let mut answered = result_ids(items);
    for call in call_ids(items) {
        let position = answered.iter().position(|id| id == &call);
        assert!(
            position.is_some(),
            "{label}: the call {call} is in the request with no result answering it: {items:?}"
        );
        answered.remove(position.expect("just found"));
    }
}

fn text_items(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter(|item| item_of(item).is_empty() && item["role"].is_string())
        .map(|item| match &item["content"] {
            Value::String(text) => text.clone(),
            // An assistant message is a list of output parts.
            Value::Array(_) => item["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default(),
            _ => String::new(),
        })
        .collect()
}

/// How much prompt text a request carried.
fn input_chars(request: &Captured) -> usize {
    let mut total = 0usize;

    for item in &items(request) {
        total += match item_of(item) {
            "function_call" => item["arguments"].as_str().unwrap_or_default().len(),
            "function_call_output" => item["output"].as_str().unwrap_or_default().len(),
            _ => text_items(std::slice::from_ref(item))
                .into_iter()
                .map(|text| text.chars().count())
                .sum(),
        };
    }

    total
}

fn contains(items: &[Value], needle: &str) -> bool {
    items.iter().any(|item| {
        item.to_string().contains(needle)
            || text_items(std::slice::from_ref(item))
                .iter()
                .any(|text| text.contains(needle))
    })
}

// ---------------------------------------------------------------------------
// Driving a runtime
// ---------------------------------------------------------------------------

/// A look-up the seeded conversation can answer, so the tool loop runs for real.
const SEARCH: Step = Step::Calls {
    calls: &[("call-1", "search_context", r#"{"query":"blinking light"}"#)],
    usage: Usage::cached(1200, 900, 40),
};

const ANSWER: Step = Step::Text {
    text: "The station keeps the light for a caller who has not spoken yet.",
    usage: Usage::plain(90, 15),
};

/// What the summarizer says, whatever it was asked to fold up.
///
/// A summarizer that could only echo the turns it was given would prove nothing
/// about the conversation it replaces, so the tests look for this text in later
/// summaries to show the earlier material travelled forward.
const SUMMARY: Step = Step::Text {
    text: "The station runs unattended; the water takes the lowest shelves first.",
    usage: Usage::plain(400, 60),
};

#[derive(Default)]
struct Recorder;

impl TurnEventReporter for Recorder {
    fn report(&mut self, _event: sujiu_runtime::events::TurnEvent) {}
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

fn runtime_for(base_url: &str, model: &str) -> SujiuRuntime {
    let runtime = SujiuRuntime::new(sujiu_runtime::seed::seed()).expect("runtime");
    configure(&runtime, base_url, model);
    runtime
}

fn run_turn(runtime: &SujiuRuntime, session_id: &str, user_text: &str) {
    let mut recorder = Recorder;
    runtime.tokio.block_on(runtime.send_turn(
        SendTurnRequest {
            session_id: session_id.to_string(),
            user_text: user_text.to_string(),
            provider: None,
            api_key: Some("test-key".to_string()),
        },
        &mut recorder,
    ));
}

// ---------------------------------------------------------------------------
// A: the handle, and what is not sent again
// ---------------------------------------------------------------------------

/// The double-billing bug, end to end.
///
/// The second round of a tool-using turn is where a stale coverage count does
/// its damage: the endpoint already holds everything the first round sent, so
/// the second request must name the handle and carry only the tool result. If
/// the runtime instead re-sends the prefix, the user is billed twice for the
/// same conversation, and nothing in the response says so.
///
/// The coverage under test is derived from the first round's own body. Nothing
/// here is injected, so this cannot pass by agreeing with itself.
#[test]
fn the_second_round_names_the_handle_and_sends_only_the_tool_result() {
    let mock = Mock::start(vec![SEARCH, ANSWER]);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the console light blinking?");

    let first = mock.next_request();
    let second = mock.next_request();

    // Round one has nothing to continue from.
    assert!(
        first.body.get("previous_response_id").is_none(),
        "the first request of a turn has no handle to name: {:?}",
        first.body
    );
    assert!(
        items(&first).iter().any(|item| item["role"] == "user"),
        "the first request carries the conversation: {:?}",
        items(&first)
    );

    // Round two names the handle the first response produced...
    assert_eq!(
        second.body["previous_response_id"].as_str(),
        Some("resp-1"),
        "the second round continues from the response it was answering: {:?}",
        second.body
    );

    // ...and sends only what the endpoint has not seen. The tool result is the
    // one thing it has not seen, and the transcript prefix is the one thing it
    // has.
    let sent = items(&second);
    assert_eq!(
        sent.len(),
        1,
        "one thing happened between the rounds, so one item goes out: {sent:?}"
    );
    let result = &sent[0];
    assert_eq!(item_of(result), "function_call_output");
    assert_eq!(
        result["call_id"].as_str(),
        Some("call-1"),
        "the result must answer the call it belongs to: {result}"
    );
    let output = result["output"].as_str().unwrap_or_default();
    assert!(
        output.contains("nightshift") || output.contains("blinking-caller"),
        "the result must be the real tool output: {result}"
    );

    // The call itself is the endpoint's to remember now. Sending it again would
    // be the same input twice.
    assert!(
        !call_ids(&sent).contains(&"call-1".to_string()),
        "the call the endpoint already produced must not be sent back: {sent:?}"
    );

    // And the tool the model asked for was really run locally: the answer says
    // so, and the transcript kept both halves of the step.
    let state = runtime.conversation_state(&session).expect("state");
    let call = state
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .find(|call| call.id == "call-1")
        .expect("the call is part of the conversation");
    assert_eq!(call.status, "completed");
    assert!(!call.result_text.is_empty(), "and it has its real result");

    drop(runtime);
    drop(mock.server);
}

/// Where a handle stops working, which is at the end of the turn that produced it.
///
/// The bug this guards against was always inside one turn: the second round of a
/// tool turn re-sending what the first round had already sent. That is fixed, and
/// the fix is visible in the test above. Carrying the handle into the *next* user
/// turn buys something much smaller — a smaller request body, not a smaller bill,
/// because a chained prefix is still billed as input — and it buys that at the
/// price of a session that cannot be reopened after the endpoint forgets the
/// response. A handle is a claim that something still exists on the other side.
///
/// The transcript is sent whole, which is also the only answer that cannot be
/// wrong about what the endpoint has.
#[test]
fn a_new_user_turn_starts_from_the_transcript_rather_than_the_last_handle() {
    let mock = Mock::start(vec![SEARCH, ANSWER, ANSWER]);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the console light blinking?");
    let _ = mock.next_request();
    let _ = mock.next_request();

    // Nothing about the prompt moved: same world-book hits, same persona, same
    // profile. A prefix digest would still match, so this is the case where
    // cross-turn chaining would look safe and be relied on.
    run_turn(&runtime, &session, "So who is on the other end?");
    let next = mock.next_request();

    assert!(
        next.body.get("previous_response_id").is_none(),
        "a handle does not outlive the turn that produced it: {:?}",
        next.body
    );

    let sent = items(&next);
    assert_every_call_is_answered(&sent, "a new user turn");
    assert!(
        contains(&sent, "Why is the console light blinking?"),
        "the conversation goes out again, because the endpoint's copy cannot be \
         assumed to still exist: {sent:?}"
    );
    assert!(
        contains(&sent, "So who is on the other end?"),
        "and the new message is the only thing this turn adds: {sent:?}"
    );
    // The call the first turn made is the proof that the whole transcript
    // travelled: the endpoint needs it to make sense of the result beside it.
    assert_eq!(
        call_ids(&sent),
        vec!["call-1".to_string()],
        "the earlier call and its result are part of what is sent: {sent:?}"
    );

    drop(runtime);
    drop(mock.server);
}

// ---------------------------------------------------------------------------
// B: a prompt that moved must not be chained
// ---------------------------------------------------------------------------

/// The reason a coverage count is not enough on its own.
///
/// The seeded world book has a keyed entry about water that the first question
/// does not match, and the second one does. That entry is injected into the
/// prompt, so the endpoint's copy of the conversation is no longer a prefix of
/// what would be sent now. Reusing the handle would splice the endpoint's older,
/// differently-shaped prompt onto a request that already re-sent everything —
/// and every one of those tokens is billed.
#[test]
fn a_prompt_that_moved_is_sent_whole_instead_of_chained() {
    let mock = Mock::start(vec![SEARCH, ANSWER, ANSWER]);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    // The first question matches the radio entry but says nothing about water.
    run_turn(&runtime, &session, "Who else listens to this frequency?");
    let _ = mock.next_request();
    let _ = mock.next_request();

    // The second one does, so the prompt gains an entry the endpoint has never
    // been sent.
    run_turn(
        &runtime,
        &session,
        "When the flood reaches the archive, who says so?",
    );
    let second = mock.next_request();

    assert!(
        second.body.get("previous_response_id").is_none(),
        "a prompt that moved cannot be continued from the endpoint's older copy: {:?}",
        second.body
    );

    let sent = items(&second);
    assert!(
        contains(&sent, "Water reaches the lowest shelves first"),
        "the entry that newly matched is in the request: {sent:?}"
    );
    assert_every_call_is_answered(&sent, "after a prompt change");
    assert!(
        contains(&sent, "So who is on the other end?")
            || contains(&sent, "Who else listens to this frequency?"),
        "the whole conversation goes out again, because the endpoint's copy is not \
         a prefix of this one: {sent:?}"
    );

    drop(runtime);
    drop(mock.server);
}

// ---------------------------------------------------------------------------
// C: the protocol is decided before provider state is matched
// ---------------------------------------------------------------------------

/// Provider state is provider-specific, and this endpoint is the same endpoint.
///
/// Nothing about the configuration changes between the two turns: same id, same
/// base URL, same model. The only thing that moved is what the endpoint speaks.
/// If the runtime matched stored state against a protocol it assumed rather than
/// the one that was negotiated, this is where a Responses handle would be handed
/// to a Chat Completions request — and the request would carry a conversation
/// the endpoint has never seen.
#[test]
fn the_protocol_is_decided_before_any_provider_state_is_matched() {
    let mock = Mock::start(vec![SEARCH, ANSWER, ANSWER]);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    // A Responses turn, which leaves a real handle behind.
    run_turn(&runtime, &session, "Why is the console light blinking?");
    let _ = mock.next_request();
    let _ = mock.next_request();

    // The same endpoint, the same model, now speaking Chat Completions.
    mock.serve(Serving::Chat);
    configure(&runtime, &mock.base_url(), "mock-model");

    run_turn(&runtime, &session, "So who is on the other end?");
    let after = mock.next_request();

    assert!(
        after.path.ends_with("/chat/completions"),
        "negotiation has to follow the endpoint, not a default: {}",
        after.path
    );
    assert!(
        after.body.get("previous_response_id").is_none(),
        "a Responses handle must not be replayed to a Chat Completions request: {:?}",
        after.body
    );
    assert!(
        after.body["messages"].is_array(),
        "the conversation travels as the transcript instead: {:?}",
        after.body
    );

    let sent = messages(&after);
    let ids: Vec<String> = sent
        .iter()
        .filter(|message| {
            !message["tool_calls"]
                .as_array()
                .is_none_or(|calls| calls.is_empty())
        })
        .flat_map(|message| {
            message["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|call| call["id"].as_str().unwrap_or_default().to_string())
        })
        .collect();
    assert_eq!(
        ids,
        vec!["call-1"],
        "the transcript carries the conversation instead of the handle: {sent:?}"
    );
    let results: Vec<String> = sent
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| {
            message["tool_call_id"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(
        results,
        vec!["call-1"],
        "with its result, or the model would be shown a call it never saw answered: {sent:?}"
    );

    // And the other direction: a Chat turn leaves state that is explicitly not
    // chainable, so switching back to Responses starts from the transcript too.
    mock.serve(Serving::Responses);
    configure(&runtime, &mock.base_url(), "mock-model");
    run_turn(&runtime, &session, "Are you still there?");
    let back = mock.next_request();

    assert!(
        back.path.ends_with("/responses"),
        "the endpoint speaks Responses again: {}",
        back.path
    );
    assert!(
        back.body.get("previous_response_id").is_none(),
        "a Chat completion id is not a Responses handle: {:?}",
        back.body
    );

    drop(runtime);
    drop(mock.server);
}

/// Negotiation is asked what it can be asked, and the answer is read off the
/// wire rather than off a default.
///
/// The probe for the newer transport goes out first, because it carries
/// continuation and tool state natively, and the mock refuses it. A runtime that
/// had already decided it speaks Responses would either fail the turn or send a
/// conversation to a route that does not exist.
#[test]
fn the_transport_is_chosen_by_asking_the_endpoint() {
    let mock = Mock::start(vec![ANSWER]);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Is anyone there?");

    let first = mock.next_probe();
    assert!(
        first.path.ends_with("/responses"),
        "Responses is asked about first: {}",
        first.path
    );
    assert!(
        first.body["input"] == json!("ping"),
        "the probe is a minimal request, not a conversation round: {:?}",
        first.body
    );
    assert!(mock.try_next_probe().is_none(), "one probe was enough");

    let request = mock.next_request();
    assert!(
        request.path.ends_with("/responses"),
        "the turn uses what the endpoint answered: {}",
        request.path
    );

    drop(runtime);
    drop(mock.server);
}

// ---------------------------------------------------------------------------
// D: the kernel compacts
// ---------------------------------------------------------------------------

/// A conversation that stops fitting is compacted by the runtime, not by
/// whichever screen happened to start the turn.
///
/// The claims are all about what must survive: every call keeps its result, the
/// archived turns are still in the transcript, the summaries accumulate instead
/// of replacing each other, and the prompt stops growing with the number of
/// turns.
#[test]
fn a_long_conversation_is_compacted_by_the_kernel_without_breaking_pairing() {
    const TURNS: usize = 6;

    // Every turn calls a tool and then answers, so there is something to pair at
    // every step of the archive.
    let mut script = Vec::new();
    for _ in 0..TURNS {
        script.push(SEARCH);
        script.push(Step::Text {
            text: "The station runs unattended after midnight.",
            usage: Usage::plain(80, 12),
        });
    }

    let mock = Mock::start(script);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    // A budget small enough to reach in a test, and a window of one recent turn
    // so the effect on the prompt is unmistakable.
    runtime.set_compaction_policy(CompactionPolicy {
        max_input_chars: 6_000,
        keep_recent_turns: 1,
    });
    let session = runtime.create_session(Some("character-lin"));

    for turn in 0..TURNS {
        run_turn(
            &runtime,
            &session,
            &format!(
                "Tell me about shift {} and the water on the lower shelves.",
                turn
            ),
        );
    }

    let sent = mock.drain_requests();
    let summaries: Vec<&Captured> = sent
        .iter()
        .filter(|request| {
            request.body["stream"] != json!(true)
                && request.body.get("tools").is_none()
                && request.body["input"]
                    .as_array()
                    .is_some_and(|items| items.first().is_some_and(|item| item["role"] == "system"))
        })
        .collect();

    assert!(
        !summaries.is_empty(),
        "a conversation over budget must be compacted by the kernel: {} requests were sent",
        sent.len()
    );

    // The summary is a request of its own, and it is an honest one: no tools, so
    // the model cannot wander off, and no handle, so it is not resumed from the
    // conversation it is being asked to fold up.
    for summary in &summaries {
        assert!(
            summary.body.get("previous_response_id").is_none(),
            "the summarizer must not be continued from the transcript it is summarizing: {:?}",
            summary.body
        );
        assert!(
            summary.body["input"]
                .as_array()
                .is_some_and(|items| items.len() == 2),
            "the summary is one instruction and the material: {:?}",
            summary.body
        );
    }

    // The second summary has to be given the first one, or the older half of the
    // conversation quietly leaves the model's view. The archive only holds the
    // most recent turn by then, so the earlier half is only reachable through the
    // summary the runtime is handing back.
    assert!(
        summaries.len() >= 2,
        "a long session compacts more than once: {} summarizer request(s)",
        summaries.len()
    );
    let material = |request: &Captured| {
        items(request)
            .last()
            .and_then(|item| item["content"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        summaries[1..]
            .iter()
            .all(|request| material(request).contains("Summary of the conversation so far")),
        "a summary after the first is told what the first one already said, or the older \
         half of the conversation leaves the model's view: {:?}",
        material(&summaries[1])
    );
    assert!(
        !material(&summaries[0]).contains("Summary of the conversation so far"),
        "there is nothing to stand on before the first compaction: {:?}",
        material(&summaries[0])
    );

    // And the turn the first summary already stands for must not be back in the
    // request. That is the difference between a summary that accumulates and a
    // summarizer that re-reads the whole conversation on every compaction: the
    // bill grows with the session instead of with the new turns, and eventually
    // the summarizer is the thing that runs out of context — which fails
    // compaction permanently, right when the conversation needs it.
    assert!(
        summaries[1..]
            .iter()
            .all(|request| !material(request).contains("shift 0 and the water")),
        "archived history the summary already carries must not be re-billed: {:?}",
        material(&summaries[1])
    );
    assert!(
        summaries[1..].iter().all(|request| {
            material(request).contains("The station runs unattended; the water")
        }),
        "what replaced it is what travels forward: {:?}",
        material(&summaries[1])
    );

    // Nothing was deleted. Compaction moves turns out of the prompt; the
    // transcript is still the whole conversation, every call still has its
    // result, and the state still says they completed.
    let state = runtime.conversation_state(&session).expect("state");
    let calls: Vec<&sujiu_runtime::runtime::ToolCallSummary> = state
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .collect();
    assert_eq!(
        calls.len(),
        TURNS,
        "every archived turn keeps its call: {}",
        state.messages.len()
    );
    for call in &calls {
        assert_eq!(
            call.status, "completed",
            "an archived call keeps the result it was given"
        );
        assert!(!call.result_text.is_empty());
    }

    // The prompt stopped tracking the number of turns. Whatever the endpoint is
    // holding, the model is being shown the fixed prompt plus a bounded window.
    let last = sent.last().expect("the last request of the session");
    let users = items(last)
        .iter()
        .filter(|item| item["role"] == "user")
        .count();
    assert!(
        users <= 2,
        "a compacted prompt keeps a bounded window, not the session: {users} user message(s) \
         in the last request after {TURNS} turns: {:?}",
        items(last)
    );
    assert!(
        contains(&items(last), "The station runs unattended after midnight.")
            || contains(&items(last), "shift 5"),
        "the most recent turn is still there in full: {:?}",
        items(last)
    );

    // The number that matters is not how many turns fit, it is how much prompt the
    // endpoint is asked to re-read. Compare the session's first full send with its
    // last: after six turns and two compactions, the second is the smaller of the
    // two, which is the property that keeps a long session from growing without
    // bound. A count of turns alone would not show this.
    let peak = sent
        .iter()
        .filter(|request| {
            request.body["stream"] == json!(true)
                && request.body.get("previous_response_id").is_none()
        })
        .map(input_chars)
        .max()
        .expect("at least one request carried the whole conversation");
    assert!(
        input_chars(last) < peak,
        "the prompt stopped growing: the largest full send carried {peak} character(s), the \
         last one {} after {TURNS} turns",
        input_chars(last)
    );

    // Every request that carried the whole conversation paired every call in it.
    for request in &sent {
        if request.body["stream"] != json!(true)
            || request.body.get("previous_response_id").is_some()
        {
            continue;
        }
        assert_every_call_is_answered(&items(request), "a full-send request");
    }

    drop(runtime);
    drop(mock.server);
}

/// A summary that could not be written must leave the transcript alone.
///
/// Folding turns into nothing would move them out of the model's reach while
/// still charging for them, which is worse than a prompt that is too long. So the
/// turn carries on with the transcript it has, the failure is recorded rather
/// than swallowed, and not one turn is archived behind a summary that is not
/// there.
#[test]
fn a_summary_that_fails_leaves_the_conversation_alone() {
    let mut script = Vec::new();
    for _ in 0..5 {
        script.push(SEARCH);
        script.push(Step::Text {
            text: "The station runs unattended after midnight.",
            usage: Usage::plain(80, 12),
        });
    }

    let mock = Mock::start(script);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    runtime.set_compaction_policy(CompactionPolicy {
        max_input_chars: 6_000,
        keep_recent_turns: 1,
    });
    // From now on the only un-streamed request — the summarizer — fails.
    mock.break_summaries();
    let session = runtime.create_session(Some("character-lin"));

    for turn in 0..5 {
        run_turn(
            &runtime,
            &session,
            &format!(
                "Tell me about shift {} and the water on the lower shelves.",
                turn
            ),
        );
    }

    let attempted = mock
        .drain_requests()
        .iter()
        .filter(|request| {
            request.body["stream"] != json!(true) && request.body.get("tools").is_none()
        })
        .count();
    assert!(
        attempted > 0,
        "the budget was reached and a summary was attempted"
    );

    // The reason is recorded, because "the conversation got shorter for no
    // visible reason" is the failure a user would report and never be believed
    // about.
    let entry = runtime
        .diagnostics(0)
        .into_iter()
        .find(|entry| entry.stage == "compaction_failed")
        .expect("a failed summary has to be reported");
    assert_eq!(
        entry.fields.get("stage").map(String::as_str),
        Some("summarize")
    );

    // And nothing was archived. The prompt stays long, the transcript stays
    // whole, and every call still has its result.
    let state = runtime.conversation_state(&session).expect("state");
    let users = state
        .messages
        .iter()
        .filter(|message| message.role == sujiu_core::ChatRole::User)
        .count();
    assert_eq!(
        users, 5,
        "a failed summary must archive nothing: {users} user message(s)"
    );
    let calls: Vec<&sujiu_runtime::runtime::ToolCallSummary> = state
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .collect();
    assert_eq!(calls.len(), 5, "and no call lost its result");
    assert!(calls.iter().all(|call| call.status == "completed"));

    drop(runtime);
    drop(mock.server);
}

// ---------------------------------------------------------------------------
// E: the ledger
// ---------------------------------------------------------------------------

/// What a turn was billed.
///
/// A turn with tools is several requests, and the number that is charged is the
/// sum over them. A ledger that only kept the last round would make an
/// expensive turn look like a cheap one — which is the same failure as the
/// double-billed prefix, run backwards: the request looks cheaper than it was,
/// so nothing looks wrong.
#[test]
fn every_round_of_a_turn_is_entered_in_the_usage_ledger() {
    let mock = Mock::start(vec![
        Step::Calls {
            calls: &[("call-1", "search_context", r#"{"query":"blinking light"}"#)],
            usage: Usage::cached(1_200, 900, 40),
        },
        Step::Text {
            text: "The station keeps the light for a caller who has not spoken yet.",
            usage: Usage::plain(90, 15),
        },
    ]);
    let runtime = runtime_for(&mock.base_url(), "mock-model");
    let session = runtime.create_session(Some("character-lin"));

    run_turn(&runtime, &session, "Why is the console light blinking?");

    let entry = runtime
        .diagnostics(0)
        .into_iter()
        .find(|entry| entry.stage == "turn_usage")
        .expect("a turn that spent tokens has to say so");
    let field = |name: &str| {
        entry
            .fields
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} is part of a usage record: {:?}", entry.fields))
    };

    assert_eq!(field("protocol"), "openai_responses", "{entry:?}");
    assert_eq!(field("model"), "mock-model", "{entry:?}");
    assert_eq!(field("rounds"), "2", "{entry:?}");
    assert_eq!(field("tool_rounds"), "1", "{entry:?}");
    assert_eq!(field("rounds_reported"), "2/2", "{entry:?}");
    // The sum, not the last round.
    assert_eq!(field("input_tokens"), "1290", "{entry:?}");
    assert_eq!(field("cached_input_tokens"), "900", "{entry:?}");
    assert_eq!(field("output_tokens"), "55", "{entry:?}");
    assert_eq!(
        field("cache_write_tokens"),
        "",
        "nothing was reported: {entry:?}"
    );
    assert_eq!(field("complete"), "true", "{entry:?}");

    // No price. The kernel reports what the endpoint said it consumed and stops
    // there; converting it to money is a decision for whoever pays the bill.
    let rendered = entry.message.to_lowercase();
    for currency in ["$", "usd", "cost", "price", "¥", "€"] {
        assert!(
            !rendered.contains(currency),
            "the kernel must not price a turn: {entry:?}"
        );
    }

    drop(runtime);
    drop(mock.server);
}
