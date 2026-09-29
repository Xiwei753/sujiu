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
//!   cargo test -p sujiu-ffi --test live_provider -- --ignored --nocapture
//! ```
//!
//! The point is to prove that the shared runtime can carry a whole turn on its
//! own: prompt compilation, the provider adapter, streaming, the tool loop and
//! the normalized event vocabulary, with no frontend involved.

use std::sync::{Arc, Mutex};

use sujiu_core::{ProviderConfig, ProviderKind};
use sujiu_ffi::events::{TurnEvent, TurnEventKind, TurnEventReporter};
use sujiu_ffi::runtime::{SendTurnRequest, SujiuRuntime};

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

    fn text(&self) -> String {
        self.events
            .lock()
            .expect("recorder poisoned")
            .iter()
            .filter_map(|event| event.text.clone())
            .collect()
    }
}

impl TurnEventReporter for Recorder {
    fn report(&mut self, event: TurnEvent) {
        self.events.lock().expect("recorder poisoned").push(event);
    }
}

#[tokio::test]
#[ignore = "needs SUJIU_TEST_BASE_URL, SUJIU_TEST_API_KEY and SUJIU_TEST_MODEL"]
async fn a_real_turn_streams_back_from_a_live_provider() {
    let base_url = std::env::var("SUJIU_TEST_BASE_URL").expect("SUJIU_TEST_BASE_URL");
    let api_key = std::env::var("SUJIU_TEST_API_KEY").expect("SUJIU_TEST_API_KEY");
    let model = std::env::var("SUJIU_TEST_MODEL").expect("SUJIU_TEST_MODEL");

    let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
    runtime
        .set_provider_config(Some(ProviderConfig {
            id: "live".to_string(),
            name: "Live test".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            base_url,
            model: model.clone(),
            credential_ref: None,
            extra: Default::default(),
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
    runtime
        .send_turn(
            SendTurnRequest {
                session_id: "session-1".to_string(),
                user_text: "In one short sentence: what are you listening for?".to_string(),
                provider: None,
                api_key: Some(api_key),
            },
            &mut sink,
        )
        .await;

    let kinds = recorder.kinds();
    println!("kinds: {kinds:?}");
    let answer = recorder.text();
    println!("answer: {answer}");

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
    assert!(
        answer
            .to_lowercase()
            .contains(&model.split('/').next().unwrap().to_lowercase())
            || answer.len() > 40,
        "the answer does not look like model output"
    );
}
