//! Live provider smoke test.
//!
//! This test talks to a real OpenAI-compatible endpoint and therefore needs a
//! credential and network access, so it is ignored by default. Run it
//! deliberately:
//!
//! ```text
//! SUJIU_TEST_BASE_URL=https://example/v1 \
//! SUJIU_TEST_API_KEY=... \
//! SUJIU_TEST_MODEL=some-model \
//!   cargo test -p sujiu-runtime --test live_provider -- --ignored --nocapture
//! ```
//!
//! The point is to prove that the shared runtime can carry a whole turn on its
//! own: prompt compilation, the provider adapter, streaming, the tool loop and
//! the normalized event vocabulary, with no frontend involved.

use std::sync::{Arc, Mutex};

use sujiu_core::EndpointConfig;
use sujiu_runtime::events::{TurnEvent, TurnEventKind, TurnEventReporter};
use sujiu_runtime::runtime::{SendTurnRequest, SujiuRuntime};

/// Collects events so the test can assert on the order they arrived in.
#[derive(Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<TurnEvent>>>,
}

impl Recorder {
    fn kinds(&self) -> Vec<TurnEventKind> {
        self.events
            .lock()
            .expect("recorder poisoned")
            .iter()
            .map(|event| event.kind)
            .collect()
    }

    /// Only the answer. Reasoning deltas carry text too, and folding them in
    /// makes the answer look like the model repeated itself.
    fn answer(&self) -> String {
        self.events
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter(|event| event.kind == TurnEventKind::TextDelta)
            .filter_map(|event| event.text.clone())
            .collect()
    }
}

impl TurnEventReporter for Recorder {
    fn report(&mut self, event: TurnEvent) {
        self.events.lock().expect("recorder poisoned").push(event);
    }
}

// Not #[tokio::test]: the runtime owns a tokio runtime, and dropping one inside
// an async context panics. The C ABI blocks on the same runtime, so this does
// too.
#[test]
#[ignore = "needs SUJIU_TEST_BASE_URL, SUJIU_TEST_API_KEY and SUJIU_TEST_MODEL"]
fn a_real_turn_streams_back_from_a_live_provider() {
    let base_url = std::env::var("SUJIU_TEST_BASE_URL").expect("SUJIU_TEST_BASE_URL");
    let api_key = std::env::var("SUJIU_TEST_API_KEY").expect("SUJIU_TEST_API_KEY");
    let model = std::env::var("SUJIU_TEST_MODEL").expect("SUJIU_TEST_MODEL");

    let runtime = SujiuRuntime::new(sujiu_runtime::seed::seed()).expect("runtime");
    runtime
        .set_endpoint(Some(EndpointConfig {
            id: "live".to_string(),
            name: "Live test".to_string(),
            base_url,
            selected_model: Some(model.clone()),
            credential_ref: None,
            overrides: Default::default(),
        }))
        .expect("provider config");

    let models = runtime.models();
    assert!(
        !models.is_empty(),
        "a configured provider must list a model"
    );
    assert!(
        models.iter().all(|entry| entry.configured),
        "a configured provider must report its model as configured"
    );

    let recorder = Recorder::default();
    let mut sink = recorder.clone();
    runtime.tokio.block_on(runtime.send_turn(
        SendTurnRequest {
            session_id: "session-1".to_string(),
            user_text: "In one short sentence: what are you listening for?".to_string(),
            provider: None,
            api_key: Some(api_key),
        },
        &mut sink,
    ));

    let kinds = recorder.kinds();
    println!("kinds: {kinds:?}");
    let answer = recorder.answer();
    println!("answer: {answer}");

    // A live failure is the one failure nobody can reproduce on demand, so the
    // log goes with it. Without these lines a red run over a real endpoint
    // reports only "expected TurnCompleted, got TurnFailed" and sends the next
    // person back to guessing.
    //
    // It prints on a green run too. A passing turn is the only thing that
    // proves which route negotiation actually took against this endpoint, and
    // that is the question a slow or misrouted endpoint keeps raising.
    println!("--- diagnostic log ---\n{}", runtime.diagnostics_text());

    assert_eq!(kinds.first(), Some(&TurnEventKind::TurnStarted));
    assert_eq!(kinds.last(), Some(&TurnEventKind::TurnCompleted));
    assert!(
        kinds.contains(&TurnEventKind::TextDelta),
        "expected streamed text, got {kinds:?}"
    );
    assert!(
        answer.split_whitespace().count() > 3,
        "expected a real answer, got {answer:?}"
    );
    // A real answer is prose, and a short creative one is still a real answer.
    // The previous check wanted the model name echoed back or more than forty
    // characters, which a live model has no reason to produce: asked what it
    // listens for, one replied "Static with a heartbeat in it." and the test
    // called that fake. It now asks the question that was actually meant -- are
    // there words here, or is this an error string wearing a success?
    let words = answer
        .split_whitespace()
        .filter(|word| word.chars().filter(|c| c.is_alphabetic()).count() >= 2)
        .count();
    assert!(words >= 2, "the answer is not prose, got {answer:?}");
}
