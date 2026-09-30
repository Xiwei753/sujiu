//! Deciding which wire protocol an endpoint speaks.
//!
//! The old answer was a lookup table: a host name plus a model name said which
//! extensions the endpoint wanted. That goes stale the moment a gateway renames
//! a model, and it cannot see a self-hosted service at all. So the question is
//! now asked of the endpoint instead, and the answer is cached per endpoint.
//!
//! Two rules shape the whole module.
//!
//! First, **a probe failure is not an answer**. A rate limit, an expired key and
//! a service outage all look like "the endpoint did not reply" from here, and
//! recording any of them as "this protocol is unsupported" would permanently
//! downgrade an endpoint because of a bad afternoon. Only a verdict that means
//! "this path is not here" is allowed to fall through to the next protocol.
//!
//! Second, **negotiation is about the endpoint, not the model**. The model name
//! is a parameter of a request, not a property of the service, and a gateway
//! that serves forty models behind one URL is one endpoint.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};

use sujiu_core::{classify, ModelListing, ProbeFailure, ProbeVerdict, Protocol, ProviderConfig};

/// What happened when we asked one protocol whether it was there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProtocolAttempt {
    pub protocol: Protocol,
    pub verdict: ProbeVerdict,
}

impl ProtocolAttempt {
    /// Whether this attempt justifies moving on to the next protocol.
    ///
    /// Only a real "no such endpoint" does. Everything else is a question we
    /// could not answer, and answering it by downgrading would be a guess.
    pub fn may_fall_through(&self) -> bool {
        self.verdict.may_fall_through()
    }
}

/// The outcome of negotiating with one endpoint.
#[derive(Clone, Debug, Serialize)]
pub struct Negotiation {
    /// The protocol the runtime will use, if one was both supported and
    /// implemented here.
    pub selected: Option<Protocol>,
    /// Every protocol the endpoint answered for, in probe order.
    pub supported: Vec<Protocol>,
    /// Protocols the endpoint could not be asked about conclusively, because
    /// the answer was a credential problem, a rate limit or an outage.
    pub undetermined: Vec<Protocol>,
    /// Protocols the endpoint does not have, or that we do not implement yet.
    ///
    /// A protocol can be in here because the endpoint supports it and this
    /// build has no adapter for it. That is worth showing rather than hiding,
    /// because it is the difference between "we degraded you" and "we cannot do
    /// that yet".
    pub not_implemented: Vec<Protocol>,
    /// A human-readable reason when nothing was selected.
    pub reason: Option<String>,
    /// Whether the result came from the cache.
    pub from_cache: bool,
    /// The classified reason the walk stopped, when it stopped on a question
    /// rather than on a "no such endpoint".
    pub failure: Option<ProbeFailure>,
}

impl Negotiation {
    fn failed(
        reason: impl Into<String>,
        failure: Option<ProbeFailure>,
        attempts: &[ProtocolAttempt],
    ) -> Self {
        Self {
            selected: None,
            supported: Vec::new(),
            undetermined: attempts
                .iter()
                .filter(|attempt| !attempt.verdict.is_supported())
                .map(|attempt| attempt.protocol)
                .collect(),
            not_implemented: Vec::new(),
            reason: Some(reason.into()),
            from_cache: false,
            failure,
        }
    }

    /// The reason a probe could not conclude, if that is why it failed.
    ///
    /// This is what a settings screen should show. "This endpoint needs a
    /// different API key" is actionable; "unsupported" is not, and sending a
    /// user to reconfigure a working endpoint because of a 429 is worse than
    /// doing nothing.
    pub fn failure_explanation(&self) -> Option<String> {
        self.reason.clone()
    }
}

/// What this build can actually talk to.
///
/// Negotiation intersects what the endpoint offers with what exists here, in
/// protocol priority order. An endpoint whose best protocol has no adapter yet
/// is reported as unimplemented rather than quietly downgraded, so the gap is
/// visible instead of being papered over by a less capable path.
pub const IMPLEMENTED_PROTOCOLS: &[Protocol] =
    &[Protocol::OpenAiChatCompletions, Protocol::AnthropicMessages];

/// Ask an endpoint which protocols it speaks.
///
/// Probes run in `preferred` order and stop at the first supported one, so the
/// common case costs one request. `cache` is optional; without it every call
/// re-probes, which is the right behaviour for a user who just fixed their
/// network.
pub async fn negotiate(
    client: Option<&reqwest::Client>,
    config: &ProviderConfig,
    api_key: &str,
    cache: Option<&CapabilityCache>,
) -> Negotiation {
    let owned;
    let client = match client {
        Some(client) => client,
        None => {
            owned = reqwest::Client::new();
            &owned
        }
    };

    let key = config.capability_key();

    if let Some(cached) = cache.and_then(|cache| cache.get(&key)) {
        return cached;
    }

    let preferred = config.preferred_protocols();
    let mut attempts = Vec::new();

    for protocol in &preferred {
        let verdict = probe(client, config, api_key, *protocol).await;
        attempts.push(ProtocolAttempt {
            protocol: *protocol,
            verdict: verdict.clone(),
        });

        if verdict.is_supported() {
            let supported: Vec<Protocol> = attempts
                .iter()
                .filter(|attempt| attempt.verdict.is_supported())
                .map(|attempt| attempt.protocol)
                .collect();

            let not_implemented = supported
                .iter()
                .copied()
                .filter(|protocol| !IMPLEMENTED_PROTOCOLS.contains(protocol))
                .collect();

            let selected = Protocol::negotiate(&supported)
                .filter(|protocol| IMPLEMENTED_PROTOCOLS.contains(protocol));

            let negotiation = Negotiation {
                selected,
                supported,
                undetermined: Vec::new(),
                not_implemented,
                reason: selected.is_none().then(|| {
                    "the endpoint speaks a protocol this build has no adapter for yet".to_string()
                }),
                from_cache: false,
                failure: None,
            };

            if let Some(cache) = cache {
                cache.put(key, negotiation.clone());
            }

            return negotiation;
        }

        // A probe we could not conclude stops the walk as well, but for a
        // different reason: continuing would mean trying the next protocol
        // against a service that is currently refusing to talk, and the first
        // useful answer would be a misleading one.
        if !verdict.may_fall_through() {
            let failure = match &verdict {
                ProbeVerdict::Inconclusive(failure) => Some(failure.clone()),
                // Unreachable: the loop already returned for a supported
                // protocol, and an unsupported one falls through.
                ProbeVerdict::Supported | ProbeVerdict::Unsupported => None,
            };

            let negotiation = Negotiation::failed(
                failure.as_ref().map(|f| f.describe()).unwrap_or(""),
                failure,
                &attempts,
            );
            // Deliberately not cached: a bad key or a rate limit is not a fact
            // about the endpoint, and caching it would make a transient failure
            // permanent.
            return negotiation;
        }
    }

    Negotiation::failed(
        "the endpoint offered none of the protocols this build speaks",
        None,
        &attempts,
    )
}

/// Ask one protocol whether it is there.
///
/// A real, minimal request is the only honest probe: an endpoint that answers
/// it can also answer a conversation, and an endpoint that does not answer it
/// at all cannot be used for anything. The reply is classified, never assumed.
async fn probe(
    client: &reqwest::Client,
    config: &ProviderConfig,
    api_key: &str,
    protocol: Protocol,
) -> ProbeVerdict {
    let base = config.base_url.trim_end_matches('/');
    let url = format!("{base}{}", protocol.path());

    let body = match protocol {
        Protocol::OpenAiChatCompletions => json!({
            "model": config.model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }],
        }),
        Protocol::OpenAiResponses => json!({
            "model": config.model,
            "max_output_tokens": 16,
            "input": "ping",
        }),
        Protocol::AnthropicMessages => json!({
            "model": config.model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }],
        }),
    };

    let mut request = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body);

    if api_key.is_empty() {
        return ProbeVerdict::Inconclusive(sujiu_core::ProbeFailure::Credentials);
    }

    request = match protocol {
        Protocol::AnthropicMessages => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(api_key),
    };

    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            classify(Some(status), &text)
        }
        Err(error) => ProbeVerdict::Inconclusive(transport_failure(&error)),
    }
}

/// Ask an endpoint what models it serves.
///
/// Model discovery is deliberately separate from protocol negotiation. A user
/// who cannot list models can still type one, and an endpoint with no listing
/// route is still perfectly good at chat. The only thing a failed listing may
/// do is explain itself.
///
/// The strategies are tried in order and the first that answers wins; each one
/// is the same request shape, because every listed protocol family has a
/// listing route. `Ok(None)` means "no listing endpoint answered", which is a
/// normal outcome and not a failure of the credential.
pub async fn list_models(
    client: Option<&reqwest::Client>,
    config: &ProviderConfig,
    api_key: &str,
) -> (ModelListing, Vec<String>) {
    let owned;
    let client = match client {
        Some(client) => client,
        None => {
            owned = reqwest::Client::new();
            &owned
        }
    };

    let base = config.base_url.trim_end_matches('/');
    let url = format!("{base}/models");

    if api_key.is_empty() {
        return (ModelListing::PermissionDenied, Vec::new());
    }

    let response = match client
        .get(&url)
        .header("content-type", "application/json")
        .bearer_auth(api_key)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return (listing_failure(&error), Vec::new()),
    };

    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();

    if !(200..300).contains(&status) {
        return (listing_status_failure(status), Vec::new());
    }

    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return (ModelListing::Unreachable, Vec::new());
    };

    let models = value
        .get("data")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if models.is_empty() {
        // A 200 with nothing in it is a listing route that does not list
        // anything, which is still "no models here" rather than "no route".
        return (ModelListing::Available, Vec::new());
    }

    (ModelListing::Available, models)
}

/// Classify a transport failure so a dropped connection is not read as a
/// missing endpoint.
///
/// A timeout is `Unavailable` rather than anything stronger: the service may be
/// perfectly capable, it just did not answer in time, and writing that down as
/// a permanent capability would be a guess.
fn transport_failure(error: &reqwest::Error) -> sujiu_core::ProbeFailure {
    if error.is_timeout() {
        sujiu_core::ProbeFailure::Unavailable
    } else if error.is_connect() {
        sujiu_core::ProbeFailure::Unavailable
    } else {
        sujiu_core::ProbeFailure::Malformed
    }
}

fn listing_failure(error: &reqwest::Error) -> ModelListing {
    match transport_failure(error) {
        sujiu_core::ProbeFailure::Unavailable => ModelListing::Unreachable,
        _ => ModelListing::Unavailable,
    }
}

fn listing_status_failure(status: u16) -> ModelListing {
    match status {
        401 | 403 => ModelListing::PermissionDenied,
        404 | 405 | 501 => ModelListing::Unavailable,
        429 => ModelListing::RateLimited,
        _ => ModelListing::Unreachable,
    }
}

/// Negotiated capabilities for one endpoint, cached between turns.
#[derive(Default)]
pub struct CapabilityCache {
    entries: Mutex<HashMap<String, Negotiation>>,
}

impl CapabilityCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<Negotiation> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: String, negotiation: Negotiation) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(key, negotiation);
        }
    }

    /// Forget everything, so the next turn re-probes.
    ///
    /// This is what a user reaches for after fixing a key or changing a
    /// gateway's routing, and it is why the cache is allowed to exist at all:
    /// an answer you cannot refresh is an answer you are stuck with.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sujiu_core::{apply_reasoning_override, ProviderKind};

    fn config(model: &str) -> ProviderConfig {
        ProviderConfig {
            id: "provider-1".into(),
            name: "test".into(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://api.example.com/v1".into(),
            model: model.into(),
            credential_ref: None,
            extra: Default::default(),
        }
    }

    #[tokio::test]
    async fn a_probe_that_was_throttled_does_not_downgrade_the_endpoint() {
        let cache = CapabilityCache::new();

        // No server is running, so every attempt is inconclusive.
        let negotiation = negotiate(None, &config("m"), "sk-test", Some(&cache)).await;

        assert!(negotiation.selected.is_none());
        assert!(!negotiation.undetermined.is_empty());
        assert!(!cache.len() > 0, "a throttled probe must not be cached");
    }

    #[tokio::test]
    async fn a_probe_without_a_credential_is_a_credential_problem() {
        let negotiation = negotiate(None, &config("m"), "", Some(&CapabilityCache::new())).await;

        assert!(negotiation.selected.is_none());
        assert_eq!(negotiation.failure, Some(ProbeFailure::Credentials));
        assert!(negotiation
            .failure_explanation()
            .is_some_and(|reason| !reason.is_empty()));
    }

    #[tokio::test]
    async fn listing_a_model_never_forbids_typing_one() {
        let (listing, models) = list_models(None, &config("m"), "sk-test").await;

        assert!(models.is_empty());
        assert!(listing.allows_manual_entry());
    }

    #[test]
    fn an_inconclusive_attempt_is_never_a_reason_to_try_the_next_protocol() {
        let rate_limited = ProtocolAttempt {
            protocol: Protocol::OpenAiResponses,
            verdict: ProbeVerdict::Inconclusive(ProbeFailure::RateLimited),
        };
        let no_such_endpoint = ProtocolAttempt {
            protocol: Protocol::OpenAiResponses,
            verdict: ProbeVerdict::Unsupported,
        };

        assert!(!rate_limited.may_fall_through());
        assert!(no_such_endpoint.may_fall_through());
    }

    #[test]
    fn the_cache_is_asked_before_the_network_and_can_be_cleared() {
        let cache = CapabilityCache::new();
        assert!(cache.get("nothing").is_none());

        cache.put(
            "provider-1|https://api.example.com/v1".to_string(),
            Negotiation {
                selected: Some(Protocol::OpenAiChatCompletions),
                supported: vec![Protocol::OpenAiChatCompletions],
                undetermined: Vec::new(),
                not_implemented: Vec::new(),
                reason: None,
                from_cache: false,
                failure: None,
            },
        );

        assert_eq!(cache.len(), 1);
        assert!(cache
            .get("provider-1|https://api.example.com/v1")
            .and_then(|entry| entry.selected)
            .is_some());

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn a_negotiated_capability_can_still_be_stated_by_hand() {
        let capabilities =
            sujiu_core::EndpointCapabilities::negotiate(&[Protocol::OpenAiChatCompletions])
                .expect("chat completions is implemented");

        let forced = apply_reasoning_override(capabilities.clone(), Some(true));
        assert!(forced.replays_assistant_reasoning);
        assert_eq!(forced.protocol, Protocol::OpenAiChatCompletions);
    }
}
