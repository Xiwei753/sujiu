//! Live capability discovery against a real endpoint.
//!
//! `discovery_wire.rs` pins the same flow against a loopback server, which can
//! only be as honest as the answers this file author chose to give it. A real
//! gateway is less cooperative, and the properties that matter here are exactly
//! the ones a mock cannot demonstrate:
//!
//! - a real `/responses` route that answers a bare `404 page not found`, which
//!   must **not** be read as proof that the route is missing, and must not stop
//!   negotiation before a protocol that does work is found;
//! - a real model listing whose ids nobody chose in advance;
//! - a real credential passing through the diagnostic log, which is the only
//!   place the redaction guarantee can be checked end to end rather than in
//!   isolation.
//!
//! Ignored by default because it needs a credential and network access:
//!
//! ```text
//! SUJIU_TEST_BASE_URL=https://example/v1 \
//! SUJIU_TEST_API_KEY=... \
//!   cargo test -p sujiu-runtime --test live_discovery -- --ignored --nocapture
//! ```
//!
//! No model is set, which is the point: discovery has to work before the user
//! has chosen one.

use sujiu_ai::DiagnosticKind;
use sujiu_runtime::runtime::SujiuRuntime;

// Not #[tokio::test]: the runtime owns a tokio runtime, and dropping one inside
// an async context panics. The C ABI blocks on the same runtime, so this does
// too.
#[test]
#[ignore = "needs SUJIU_TEST_BASE_URL and SUJIU_TEST_API_KEY"]
fn a_live_endpoint_is_described_without_a_model_and_without_leaking_the_key() {
    let base_url = std::env::var("SUJIU_TEST_BASE_URL").expect("SUJIU_TEST_BASE_URL");
    let api_key = std::env::var("SUJIU_TEST_API_KEY").expect("SUJIU_TEST_API_KEY");

    let runtime = SujiuRuntime::new(sujiu_runtime::seed::seed()).expect("runtime");
    let exploration = runtime
        .tokio
        .block_on(runtime.discover_endpoint(&base_url, &api_key, true));

    println!("status: {}", exploration.status);
    println!("protocol: {}", exploration.protocol);
    println!("protocols: {:?}", exploration.protocols);
    println!("reason: {:?}", exploration.reason);
    println!("listing: {}", exploration.models.listing);
    println!("note: {}", exploration.models.note);
    println!("models found: {}", exploration.models.models.len());
    for model in exploration.models.models.iter().take(5) {
        println!("  - {}", model.id);
    }

    // A live endpoint that answers nothing is a legitimate outcome and is
    // reported as one. What must never happen is a *blank* answer, because a
    // blank one gives a screen nothing to render.
    assert!(
        !exploration.status.is_empty(),
        "a live probe must report one of the known statuses"
    );

    // The property this endpoint was chosen to test. Its `/responses` route and
    // its probe requests both come back as a bare `404 page not found`, because
    // the placeholder model it is probed with is not one it serves. Every
    // attempt is ambiguous, so the runtime learned nothing — and "learned
    // nothing" must not be reported as "this endpoint speaks nothing we know".
    //
    // The endpoint answers a real chat request perfectly well; that is the
    // whole reason this assertion is here rather than in a mock test.
    if exploration.protocol.is_empty() {
        assert_eq!(
            exploration.status, "undetermined",
            "an endpoint that declined to answer must not be reported as \
             unusable; reason was {:?}",
            exploration.reason
        );
    }

    // Manual model entry is what makes a failed listing survivable, so it is
    // checked against a real endpoint rather than trusted from the mock.
    assert!(
        exploration.models.manual_entry_allowed,
        "manual model entry must survive any listing outcome"
    );

    // Every outcome above belongs to the discovery half of the log. A chat
    // line here would mean the settings screen and the conversation are writing
    // to each other's history.
    let discovery = runtime.discovery_diagnostics(0);
    assert!(
        !discovery.is_empty(),
        "probing an endpoint must leave a discovery log"
    );
    for entry in &discovery {
        assert_eq!(
            entry.kind,
            DiagnosticKind::Discovery,
            "the filtered log returned a {} line",
            entry.kind.label()
        );
    }

    // The guarantee this whole feature rests on, checked on a real request
    // rather than on a string handed to `redact` directly.
    let rendered = runtime.diagnostics_text();
    println!("--- diagnostic log ---\n{rendered}");

    assert!(
        !rendered.contains(&api_key),
        "the diagnostic log leaked the API key in full"
    );
    assert!(
        rendered.contains(&base_url),
        "the diagnostic log does not name the endpoint it was asked about"
    );

    // A masked form is what identifies one key from another without being
    // usable. Only required when the key is long enough to mask meaningfully.
    if api_key.len() >= 12 {
        let masked = sujiu_ai::mask_secret(&api_key);
        assert!(
            rendered.contains(&masked),
            "expected the key to appear as {masked}"
        );
        assert!(
            !masked.contains(&api_key),
            "the masked form must not contain the whole key"
        );
    }
}
