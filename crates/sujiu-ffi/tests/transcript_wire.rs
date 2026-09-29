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
use sujiu_core::{ProviderConfig, ProviderKind};
use sujiu_ffi::events::{TurnEvent, TurnEventKind, TurnEventReporter};
use sujiu_ffi::runtime::{SendTurnRequest, SujiuRuntime};

/// One scripted answer. The server hands these out in order.
#[derive(Clone)]
enum Step {
    /// The model answers in words.
    Text(&'static str),
    /// The model asks for tools, in one step.
    Calls(&'static [(&'static str, &'static str, &'static str)]),
    /// The provider answers with an HTTP error.
    Http(u16),
    /// The provider claims to stream and then sends something that is not a
    /// stream at all, which is how a proxy failure usually shows up.
    Garbage,
}

struct ScriptedProvider {
    port: u16,
    requests: mpsc::Receiver<Value>,
    server: thread::JoinHandle<()>,
}

impl ScriptedProvider {
    /// `script` holds one step per request, in order. The last entry repeats,
    /// so a test that only cares about the first few requests does not have to
    /// predict how many there will be.
    fn start(script: Vec<Step>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel();

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
            server,
        }
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
        Step::Calls(calls) => {
            for (position, (id, name, arguments)) in calls.iter().enumerate() {
                // A tool call arrives in fragments, exactly as a real stream
                // does, so reassembly is exercised too.
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
        .set_provider_config(Some(ProviderConfig {
            id: "mock".to_string(),
            name: "Mock".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: base_url.to_string(),
            model: model.to_string(),
            credential_ref: None,
            extra: Default::default(),
        }))
        .expect("provider config");
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
