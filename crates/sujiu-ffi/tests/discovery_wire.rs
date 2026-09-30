//! What Sujiu can find out about an endpoint it has never been given.
//!
//! The configuration flow this file pins down is the one a user actually
//! works in: type an address and a key, and find out what is there, before
//! anything has been saved and before a model has been chosen. The earlier flow
//! made the endpoint un-saveable until a model was already known, and then asked
//! these same questions through the saved endpoint, so discovery ran last and
//! could never inform the decision it was supposed to inform.
//!
//! Everything runs against a loopback server, so the requests are the real
//! ones: a real model listing and a real capability probe per protocol.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use serde_json::{json, Value};
use sujiu_ffi::runtime::SujiuRuntime;

/// What the endpoint says about one route.
///
/// `Absent` is the interesting one: it answers 404 with a body that names the
/// path as unknown, which is the only answer allowed to move negotiation on to
/// the next protocol.
#[derive(Clone)]
enum Route {
    Absent,
    Ok(Value),
    Status(u16, Value),
    /// A raw body, for the routes a real turn streams from. The probe reads
    /// only the status, so a stream is also a valid answer to a probe.
    Stream(String),
}

/// A loopback endpoint that answers three routes and records every request.
struct EndpointServer {
    port: u16,
    seen: mpsc::Receiver<(String, Value)>,
}

impl EndpointServer {
    /// `listing` answers `GET /v1/models`; `responses` and `chat` answer the
    /// two capability probes.
    fn start(listing: Route, responses: Route, chat: Route) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Some(request) = read_request(&stream) else {
                    continue;
                };
                let route = if request.path.ends_with("/models") {
                    &listing
                } else if request.path.ends_with("/responses") {
                    &responses
                } else {
                    &chat
                };
                let _ = tx.send((request.path, request.body));
                write_route(&stream, route);
            }
        });

        Self { port, seen: rx }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    /// Every path the runtime asked about, in the order it asked.
    fn paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        while let Ok((path, _)) = self
            .seen
            .recv_timeout(std::time::Duration::from_millis(300))
        {
            paths.push(path);
        }
        paths
    }
}

/// One parsed request: the path it went to, and its body.
struct Request {
    path: String,
    body: Value,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut length = 0usize;
    let mut path = String::new();
    let mut line = String::new();

    while reader.read_line(&mut line).ok()? > 0 {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if trimmed.starts_with("GET ") || trimmed.starts_with("POST ") {
            // `GET /v1/models HTTP/1.1` — the path is the middle token. Taking
            // the rest of the line would leave " HTTP/1.1" glued to it, and a
            // path that no longer ends in its own name matches no route at all.
            path = trimmed
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
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
    // A model listing is a GET, so it has no body to speak of. An absent body
    // is a normal request shape, not a malformed one.
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).ok()?
    };

    Some(Request { path, body })
}

fn write_route(mut stream: &TcpStream, route: &Route) {
    let (status, content_type, body) = match route {
        Route::Absent => (
            404,
            "application/json",
            json!({"error": {"message": "unknown endpoint", "type": "invalid_request_error"}})
                .to_string(),
        ),
        Route::Ok(body) => (200, "application/json", body.to_string()),
        Route::Status(status, body) => (*status, "application/json", body.to_string()),
        Route::Stream(body) => (200, "text/event-stream", body.clone()),
    };

    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn runtime() -> SujiuRuntime {
    SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime")
}

fn models(ids: &[&str]) -> Value {
    json!({ "data": ids.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>() })
}

/// A probe answer that says the route is not there.
fn absent() -> Route {
    Route::Absent
}

/// A probe answer that says the route works.
fn answers() -> Route {
    Route::Ok(json!({ "id": "probe", "choices": [{ "message": { "content": "pong" } }] }))
}

/// A probe answer for the Responses route, in the shape that transport really
/// returns.
///
/// A 200 is all the probe reads, but a real turn goes to the same route and
/// parses the body. Answering it with a chat-completions shape would make this
/// test pass its negotiation assertions and then fail the turn for a reason
/// that has nothing to do with the thing under test.
fn responses_answers() -> Route {
    let mut body = String::new();
    for frame in [
        json!({"type": "response.created", "response": {"id": "resp_1", "status": "in_progress"}}),
        json!({"type": "response.output_text.delta", "delta": "pong"}),
        json!({"type": "response.completed", "response": {
            "id": "resp_1",
            "status": "completed",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "pong"}]}]
        }}),
    ] {
        body.push_str("data: ");
        body.push_str(&frame.to_string());
        body.push_str("\r\n\r\n");
    }
    Route::Stream(body)
}

/// The endpoint is explored from a URL and a key alone.
///
/// Nothing is saved, and no model needs to be known. The three questions the
/// runtime asks are the three a user would expect: can you list models, do you
/// speak Responses, do you speak Chat Completions.
#[test]
fn an_endpoint_is_explored_from_a_url_and_a_key_alone() {
    let server = EndpointServer::start(
        Route::Ok(models(&["harbour-large", "harbour-mini"])),
        absent(),
        answers(),
    );
    let runtime = runtime();

    let exploration = runtime
        .tokio
        .block_on(runtime.discover_endpoint(&server.base_url(), "sk-test"));

    let paths = server.paths();
    assert!(
        paths.iter().any(|path| path.ends_with("/models")),
        "a model list was never asked for: {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path.ends_with("/responses")),
        "the highest-capability protocol was never probed: {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path.ends_with("/chat/completions")),
        "the common denominator was never probed: {paths:?}"
    );

    assert_eq!(exploration.protocol, "openai_chat_completions");
    assert_eq!(
        exploration.models.listing, "available",
        "the list was there and has to be reported as found"
    );
    assert!(
        exploration
            .models
            .models
            .iter()
            .any(|model| model.id == "harbour-large"),
        "a discovered model has to reach the screen that will offer it: {:?}",
        exploration.models.models
    );

    assert!(
        runtime.endpoint().is_none(),
        "exploring an endpoint must not quietly store one"
    );
}

/// The model list and the protocol are two independent questions.
///
/// This endpoint will not enumerate anything and will still speak Chat
/// Completions. Reporting the protocol as unknown because the listing failed
/// would send the user looking for a protocol problem that does not exist.
#[test]
fn a_gateway_that_cannot_list_still_reports_the_protocol_it_speaks() {
    let server = EndpointServer::start(absent(), absent(), answers());
    let runtime = runtime();

    let exploration = runtime
        .tokio
        .block_on(runtime.discover_endpoint(&server.base_url(), "sk-test"));

    assert_eq!(exploration.protocol, "openai_chat_completions");
    assert_eq!(
        exploration.models.listing, "unavailable",
        "a missing listing route is exactly that, and not a protocol verdict"
    );
    assert!(
        exploration.models.manual_entry_allowed,
        "there is no endpoint that can refuse the user the right to type a model"
    );
}

/// A rejected key is a key problem, and is reported as one.
///
/// Both questions answered 401, and neither answer means the endpoint is
/// missing a protocol. A screen that showed "unsupported protocol" here would
/// send the user hunting for a setting that does not exist while the real
/// problem is a typo in the key.
#[test]
fn a_rejected_key_is_reported_as_a_key_problem() {
    let rejected = Route::Status(401, json!({"error": {"message": "invalid api key"}}));
    let server = EndpointServer::start(rejected.clone(), rejected.clone(), rejected);
    let runtime = runtime();

    let exploration = runtime
        .tokio
        .block_on(runtime.discover_endpoint(&server.base_url(), "sk-wrong"));

    assert_eq!(
        exploration.models.listing, "permission_denied",
        "a 401 on a listing is a permission verdict"
    );
    assert_eq!(
        exploration.protocol, "",
        "no protocol was settled on, and an empty string says so plainly"
    );
    let reason = exploration.reason.expect("a reason a user can read");
    assert!(
        reason.contains("key"),
        "the reason has to name the key, because the key is what is wrong: {reason}"
    );
    assert!(
        exploration.models.manual_entry_allowed,
        "refusing a key does not take the manual choice away either"
    );
}

/// Whatever discovery finds, the one choice that is genuinely the user's stays
/// open.
#[test]
fn every_discovery_outcome_leaves_the_model_to_the_user() {
    let cases: Vec<(&str, Route, Route, Route)> = vec![
        ("found", Route::Ok(models(&["a", "b"])), absent(), answers()),
        ("no route", absent(), absent(), answers()),
        (
            "not allowed",
            Route::Status(403, json!({"error": "forbidden"})),
            absent(),
            answers(),
        ),
        (
            "throttled",
            Route::Status(429, json!({"error": "slow down"})),
            Route::Status(429, json!({"error": "slow down"})),
            answers(),
        ),
    ];

    for (label, listing, responses, chat) in cases {
        let server = EndpointServer::start(listing, responses, chat);
        let runtime = runtime();

        let exploration = runtime
            .tokio
            .block_on(runtime.discover_endpoint(&server.base_url(), "sk-test"));

        assert!(
            exploration.models.manual_entry_allowed,
            "{label}: discovery took the user's choice away"
        );
        assert!(
            !exploration.models.note.trim().is_empty(),
            "{label}: an outcome with no explanation leaves the user guessing"
        );
    }
}

/// An endpoint that speaks Responses gets the transport that carries
/// continuation, reasoning and tool state in its own shape.
///
/// The point of probing Responses first is only realised if a Responses-capable
/// endpoint is actually driven over Responses, rather than recognised and then
/// quietly downgraded to the older chat protocol.
#[test]
fn a_responses_capable_endpoint_is_chosen_over_the_older_one() {
    let server = EndpointServer::start(
        Route::Ok(models(&["harbour-reasoner"])),
        responses_answers(),
        answers(),
    );
    let runtime = runtime();

    let exploration = runtime
        .tokio
        .block_on(runtime.discover_endpoint(&server.base_url(), "sk-test"));

    assert_eq!(exploration.protocol, "openai_responses");
    assert_eq!(
        exploration.protocols,
        vec!["openai_responses".to_string()],
        "negotiation stops at the first protocol that works, so this is what was established, not a list of what else might be there"
    );

    // And a real turn then goes out over Responses, using the shape that
    // transport defines rather than the chat one.
    runtime
        .set_endpoint(Some(sujiu_core::EndpointConfig {
            id: "mock".to_string(),
            name: "Mock".to_string(),
            base_url: server.base_url(),
            selected_model: Some("harbour-reasoner".to_string()),
            credential_ref: None,
            overrides: Default::default(),
        }))
        .expect("the chosen endpoint can be saved");

    let session = runtime.create_session(Some("character-lin"));
    let mut recorder = Recorder::default();
    runtime.tokio.block_on(runtime.send_turn(
        sujiu_ffi::runtime::SendTurnRequest {
            session_id: session.clone(),
            user_text: "Say something.".to_string(),
            provider: None,
            api_key: Some("sk-test".to_string()),
        },
        &mut recorder,
    ));

    let conversation = runtime
        .conversation_state(&session)
        .expect("the turn produced a conversation");
    let answers: Vec<&str> = conversation
        .messages
        .iter()
        .map(|message| message.text.as_str())
        .collect();
    assert!(
        answers.iter().any(|text| text.contains("pong")),
        "the turn has to reach the endpoint that answered, or the negotiation was a performance: {answers:?}"
    );
    assert!(
        !recorder.failed,
        "a turn over a protocol the endpoint advertises cannot fail on the wire"
    );
}

/// Records turn events so a test can tell a finished turn from a failed one.
#[derive(Default)]
struct Recorder {
    failed: bool,
}

impl sujiu_ffi::events::TurnEventReporter for Recorder {
    fn report(&mut self, event: sujiu_ffi::events::TurnEvent) {
        if event.kind == sujiu_ffi::events::TurnEventKind::TurnFailed {
            self.failed = true;
        }
    }
}
