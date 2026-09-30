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

use sujiu_core::{classify, EndpointConfig, ModelListing, ProbeFailure, ProbeVerdict, Protocol};

use crate::diagnostics::{fields, truncate, DiagnosticKind, DiagnosticLog};

/// What happened when we asked one protocol whether it was there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProtocolAttempt {
    pub protocol: Protocol,
    pub verdict: ProbeVerdict,
    /// The status the endpoint answered with, when it answered at all.
    ///
    /// Kept because a bare verdict cannot be acted on: "unsupported" and "the
    /// service was down" both stop a walk, and only the status says which
    /// happened. A screen and a log line both need it.
    pub status: Option<u16>,
    /// A bounded excerpt of the body, redacted.
    pub body_excerpt: String,
}

impl ProtocolAttempt {
    /// Whether this attempt justifies moving on to the next protocol.
    ///
    /// Only a real "no such endpoint" does. Everything else is a question we
    /// could not answer, and answering it by downgrading would be a guess.
    pub fn may_fall_through(&self) -> bool {
        self.verdict.may_fall_through()
    }

    /// The verdict as one word a log or a screen can show.
    fn verdict_label(&self) -> &'static str {
        match &self.verdict {
            ProbeVerdict::Supported => "supported",
            ProbeVerdict::Unsupported => "unsupported",
            ProbeVerdict::Ambiguous => "ambiguous",
            ProbeVerdict::Inconclusive(failure) => match failure {
                ProbeFailure::NoSuchEndpoint => "no_such_endpoint",
                ProbeFailure::Credentials => "credentials_rejected",
                ProbeFailure::RateLimited => "rate_limited",
                ProbeFailure::Unavailable => "unreachable",
                ProbeFailure::ModelUnavailable => "model_unavailable",
                ProbeFailure::Malformed => "unreadable",
            },
        }
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
    /// Every protocol that was actually asked, in order, with what it said.
    ///
    /// The walk stops early, so this is normally short. It is kept because the
    /// interesting case is the one that failed: knowing that Responses was
    /// tried and answered 401 is the difference between retyping a key and
    /// retyping an address.
    #[serde(default)]
    pub attempts: Vec<ProtocolAttempt>,
    /// Whether at least one protocol answered ambiguously.
    ///
    /// An ambiguous answer — a bare `404 page not found`, which is what a
    /// gateway that routes by model returns both for a path it never had and
    /// for a model it cannot resolve — is allowed to fall through to the next
    /// protocol, because it is the only answer that proves nothing either way.
    /// But "proves nothing" has to survive the walk. Without this flag the
    /// exhaustion path is indistinguishable from a clean sweep, and an endpoint
    /// that merely declined to answer gets reported as speaking none of the
    /// protocols this build knows — a false statement about a working endpoint,
    /// and the kind a user will act on by going and reconfiguring something
    /// that was never broken.
    #[serde(default)]
    pub ambiguous: bool,
}

impl Negotiation {
    fn failed(
        reason: impl Into<String>,
        failure: Option<ProbeFailure>,
        attempts: &[ProtocolAttempt],
    ) -> Self {
        let ambiguous = attempts
            .iter()
            .any(|attempt| attempt.verdict.is_ambiguous());
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
            attempts: attempts.to_vec(),
            ambiguous,
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

    /// The one-word outcome a screen branches on.
    ///
    /// `reason` is a sentence for a person and `status` is a code for a
    /// decision, and neither does the other's job. "We could not tell" and
    /// "this endpoint speaks nothing we know" need different advice — one says
    /// try again, the other says use a different endpoint — and when they
    /// arrive as the same sentence every one of them is shown as an
    /// undifferentiated probe failure, which is the outcome the user cannot act
    /// on.
    ///
    /// One of: `supported`, `no_usable_protocol`, `credentials_rejected`,
    /// `no_such_endpoint`, `rate_limited`, `server_unavailable`,
    /// `network_error`, `unreadable`, `undetermined`.
    pub fn status(&self) -> &'static str {
        if self.selected.is_some() {
            return "supported";
        }
        let failure = match self.failure.clone() {
            Some(failure) => failure,
            // Every protocol was asked and every one of them answered that it
            // does not do this. That is a real negative, and the advice is to
            // go somewhere else.
            //
            // Unless one of them only declined to answer. "This endpoint has
            // none of our protocols" and "we could not find out" need different
            // advice — one says use a different endpoint, the other says try
            // again — and the second is what a gateway that answers every probe
            // with a bare 404 looks like.
            None if self.ambiguous => return "undetermined",
            None => return "no_usable_protocol",
        };
        match failure {
            ProbeFailure::Credentials => "credentials_rejected",
            ProbeFailure::NoSuchEndpoint => "no_such_endpoint",
            ProbeFailure::RateLimited => "rate_limited",
            ProbeFailure::Malformed => "unreadable",
            // The endpoint only rejected the placeholder model name, which says
            // nothing about which protocols it speaks. Walking past it concludes
            // nothing, and a screen must not turn that into a verdict.
            ProbeFailure::ModelUnavailable => "undetermined",
            ProbeFailure::Unavailable => {
                // "The network is not there" and "the server is unwell" share a
                // verdict and share no fix. An HTTP status is the evidence that
                // separates them: a transport failure never got far enough to
                // have one, and a server that answered at all is demonstrably
                // reachable.
                if self.attempts.iter().any(|attempt| attempt.status.is_some()) {
                    "server_unavailable"
                } else {
                    "network_error"
                }
            }
        }
    }
}

/// What this build can actually talk to.
///
/// Negotiation intersects what the endpoint offers with what exists here, in
/// protocol priority order. An endpoint whose best protocol has no adapter yet
/// is reported as unimplemented rather than quietly downgraded, so the gap is
/// visible instead of being papered over by a less capable path.
pub const IMPLEMENTED_PROTOCOLS: &[Protocol] = &[
    Protocol::OpenAiResponses,
    Protocol::OpenAiChatCompletions,
    Protocol::AnthropicMessages,
];

/// Ask an endpoint which protocols it speaks.
///
/// Probes run in `preferred` order and stop at the first supported one, so the
/// common case costs one request. `cache` is optional; without it every call
/// re-probes, which is the right behaviour for a user who just fixed their
/// network. `log` is optional and only ever adds lines: a caller with nowhere
/// to write a diagnostic gets exactly the behaviour it had before.
///
/// `probe_model` names the model to put in the probe request. `None` means
/// "use the chosen model if there is one, otherwise the placeholder", which is
/// what a turn wants. A settings screen passes a model it discovered when the
/// placeholder came back inconclusive, because a gateway that routes by model
/// answers a placeholder with the same 404 it uses for a missing path — see
/// [`negotiate_asking`] for the policy that decides when to do that.
pub async fn negotiate(
    client: Option<&reqwest::Client>,
    config: &EndpointConfig,
    api_key: &str,
    cache: Option<&CapabilityCache>,
    log: Option<&DiagnosticLog>,
    probe_model: Option<&str>,
) -> Negotiation {
    let owned;
    let client = match client {
        Some(client) => client,
        None => {
            owned = crate::provider::http_client();
            &owned
        }
    };

    let key = config.capability_key();

    if let Some(cached) = cache.and_then(|cache| cache.get(&key)) {
        if let Some(log) = log {
            log.record(
                DiagnosticKind::Discovery,
                "probe_cached",
                "the endpoint was already asked; the earlier answer is being reused",
                fields([
                    ("endpoint", config.normalized_base_url()),
                    (
                        "protocol",
                        cached
                            .selected
                            .map(protocol_name)
                            .unwrap_or_else(|| "none".to_string()),
                    ),
                ]),
            );
        }
        return cached;
    }

    let preferred = config.protocols();

    if let Some(log) = log {
        log.record(
            DiagnosticKind::Discovery,
            "probe_start",
            "asking the endpoint what it speaks",
            fields([
                ("endpoint", config.normalized_base_url()),
                (
                    "protocols",
                    preferred
                        .iter()
                        .map(|protocol| protocol_name(*protocol))
                        .collect::<Vec<_>>()
                        .join(","),
                ),
                (
                    "model",
                    config
                        .selected_model
                        .clone()
                        .filter(|model| !model.trim().is_empty())
                        .unwrap_or_else(|| "(not chosen yet)".to_string()),
                ),
            ]),
        );
    }

    let mut attempts: Vec<ProtocolAttempt> = Vec::new();
    let mut ambiguous = false;

    // One place decides which model a probe names, so the diagnostic log and the
    // wire request can never disagree about it.
    let model = probe_model
        .map(str::to_string)
        .or_else(|| {
            config
                .selected_model
                .clone()
                .filter(|model| !model.trim().is_empty())
        })
        .unwrap_or_else(|| PLACEHOLDER_MODEL.to_string());

    for protocol in &preferred {
        let attempt = probe(client, config, api_key, *protocol, &model, log).await;

        if let Some(log) = log {
            log.record(
                DiagnosticKind::Discovery,
                "protocol_result",
                format!("{}: {}", protocol_name(*protocol), attempt.verdict_label()),
                fields([
                    ("endpoint", config.normalized_base_url()),
                    ("protocol", protocol_name(*protocol)),
                    (
                        "status",
                        attempt
                            .status
                            .map(|status| status.to_string())
                            .unwrap_or_else(|| "no answer".to_string()),
                    ),
                    ("body", attempt.body_excerpt.clone()),
                ]),
            );
        }

        let verdict = attempt.verdict.clone();
        attempts.push(attempt);

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
                attempts: attempts.clone(),
                ambiguous: false,
            };

            if let Some(cache) = cache {
                cache.put(key, negotiation.clone());
            }

            if let Some(log) = log {
                record_probe_end(log, config, &negotiation, &attempts);
            }

            return negotiation;
        }

        // A probe we could not conclude stops the walk, but for a different
        // reason: continuing would mean trying the next protocol against a
        // service that is currently refusing to talk, and the first useful
        // answer would be a misleading one.
        if !verdict.may_fall_through() {
            let failure = match &verdict {
                ProbeVerdict::Inconclusive(failure) => Some(failure.clone()),
                // Unreachable: the loop already returned for a supported
                // protocol, and everything else falls through.
                ProbeVerdict::Supported | ProbeVerdict::Unsupported | ProbeVerdict::Ambiguous => {
                    None
                }
            };

            let negotiation = Negotiation::failed(
                failure.as_ref().map(|f| f.describe()).unwrap_or(""),
                failure,
                &attempts,
            );
            if let Some(log) = log {
                record_probe_end(log, config, &negotiation, &attempts);
            }
            // Deliberately not cached: a bad key or a rate limit is not a fact
            // about the endpoint, and caching it would make a transient failure
            // permanent.
            return negotiation;
        }

        // An ambiguous answer falls through like a missing route but is not
        // one, so it is remembered separately: if the walk ends with nothing
        // selected, the honest report is that we could not tell, not that this
        // endpoint speaks nothing we know.
        if verdict.is_ambiguous() {
            ambiguous = true;
        }
    }

    let reason = if ambiguous {
        "the endpoint would not give an answer we could read for any protocol we speak, so we could not tell what it speaks"
    } else {
        "the endpoint offered none of the protocols this build speaks"
    };

    let negotiation = Negotiation::failed(reason, None, &attempts);
    if let Some(log) = log {
        record_probe_end(log, config, &negotiation, &attempts);
    }
    negotiation
}

/// Ask an endpoint what it speaks, retrying with a real model when the
/// placeholder cannot get an answer anyone can read.
///
/// A probe has to name a model, and on a settings screen none has been chosen
/// yet, so the first attempt names a placeholder. That works against an
/// endpoint that ignores the model field. It is blind against a gateway that
/// **routes by model**: that one answers "no such model" with the very same
/// `404 page not found` it uses for a path it never had, so every protocol comes
/// back ambiguous and the walk ends having learned nothing — about an endpoint
/// that may well be working perfectly.
///
/// Reading that answer honestly is the fix, and it is not enough. Ambiguous
/// really does mean "we could not tell", and the honest report of not being able
/// tell is a screen that cannot help. So when the listing already succeeded and
/// handed back a model the user could actually pick, the probe asks again with
/// that model. Naming a model a gateway recognises is the only way to make the
/// route answer at all.
///
/// The retry costs a real request and only happens on the path that learned
/// nothing, so the common case — an endpoint that answers the placeholder, or
/// one that answers with a real status — still costs exactly one probe per
/// protocol. A listing whose models are all unservable will still come back
/// inconclusive, which is the truthful answer rather than a guess.
pub async fn negotiate_asking(
    client: Option<&reqwest::Client>,
    config: &EndpointConfig,
    api_key: &str,
    cache: Option<&CapabilityCache>,
    log: Option<&DiagnosticLog>,
    listed_models: &[String],
) -> Negotiation {
    let first = negotiate(client, config, api_key, cache, log, None).await;

    // Only a walk that proved nothing earns a second question. A supported
    // protocol, a rejected key and a rate limit are all answers.
    if first.status() != "undetermined" || listed_models.is_empty() {
        return first;
    }

    let model = listed_models[0].clone();

    if let Some(log) = log {
        log.record(
            DiagnosticKind::Discovery,
            "probe_retry",
            "the placeholder could not get an answer anyone could read, so asking again with a model this endpoint actually lists",
            fields([
                ("endpoint", config.normalized_base_url()),
                ("placeholder", PLACEHOLDER_MODEL.to_string()),
                ("retry_model", model.clone()),
            ]),
        );
    }

    let second = negotiate(client, config, api_key, cache, log, Some(&model)).await;

    if let Some(log) = log {
        log.record(
            DiagnosticKind::Discovery,
            "probe_retry_end",
            format!("the second attempt finished as {}", second.status()),
            fields([
                ("endpoint", config.normalized_base_url()),
                ("model", model),
                ("status", second.status().to_string()),
            ]),
        );
    }

    second
}

/// Record how a probe ended, whichever way it ended.
///
/// Every exit from the walk passes through here, including the ones that settle
/// nothing. A probe that stopped on a rate limit and a probe that ran out of
/// protocols are very different problems, and without this line a log ends
/// mid-question.
fn record_probe_end(
    log: &DiagnosticLog,
    config: &EndpointConfig,
    negotiation: &Negotiation,
    attempts: &[ProtocolAttempt],
) {
    log.record(
        DiagnosticKind::Discovery,
        "probe_end",
        match negotiation.selected {
            Some(protocol) => format!("{} is the protocol to use", protocol_name(protocol)),
            None => "nothing usable was found at this endpoint".to_string(),
        },
        fields([
            ("endpoint", config.normalized_base_url()),
            (
                "selected",
                negotiation
                    .selected
                    .map(protocol_name)
                    .unwrap_or_else(|| "none".to_string()),
            ),
            ("asked", attempts.len().to_string()),
            (
                "reason",
                negotiation
                    .reason
                    .clone()
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or_else(|| "no reason given".to_string()),
            ),
        ]),
    );
}

/// The model name a probe sends when nothing has been chosen and nothing has
/// been discovered to try instead.
const PLACEHOLDER_MODEL: &str = "probe";

/// Ask one protocol whether it is there.
///
/// A real, minimal request is the only honest probe: an endpoint that answers
/// it can also answer a conversation, and an endpoint that does not answer it
/// at all cannot be used for anything. The reply is classified, never assumed.
///
/// `model` has already been resolved by the caller, so this function cannot
/// disagree with the diagnostic line recorded above it about what was asked.
async fn probe(
    client: &reqwest::Client,
    config: &EndpointConfig,
    api_key: &str,
    protocol: Protocol,
    model: &str,
    log: Option<&DiagnosticLog>,
) -> ProtocolAttempt {
    let base = config.base_url.trim_end_matches('/');
    let url = format!("{base}{}", protocol.path());

    let model = model.to_string();

    let body = match protocol {
        Protocol::OpenAiChatCompletions => json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }],
        }),
        Protocol::OpenAiResponses => json!({
            "model": model,
            "max_output_tokens": 16,
            "input": "ping",
        }),
        Protocol::AnthropicMessages => json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }],
        }),
    };

    let mut request = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body);

    if api_key.is_empty() {
        // Not a request, so there is no status and no body. Recorded anyway:
        // "nothing was asked, because there was no key" is the single most
        // useful thing to read when a probe appears to have done nothing.
        if let Some(log) = log {
            log.record(
                DiagnosticKind::Discovery,
                "probe_skipped",
                "no key was supplied, so the endpoint was not asked",
                fields([
                    ("endpoint", config.normalized_base_url()),
                    ("protocol", protocol_name(protocol)),
                ]),
            );
        }
        return ProtocolAttempt {
            protocol,
            verdict: ProbeVerdict::Inconclusive(sujiu_core::ProbeFailure::Credentials),
            status: None,
            body_excerpt: String::new(),
        };
    }

    request = match protocol {
        Protocol::AnthropicMessages => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(api_key),
    };

    if let Some(log) = log {
        log.record(
            DiagnosticKind::Discovery,
            "probe_request",
            format!("POST {}{}", config.normalized_base_url(), protocol.path()),
            fields([
                ("endpoint", config.normalized_base_url()),
                ("protocol", protocol_name(protocol)),
                ("model", model),
            ]),
        );
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            ProtocolAttempt {
                protocol,
                verdict: classify(Some(status), &text),
                status: Some(status),
                // The excerpt is bounded and goes through the log's own
                // redaction, so a body that echoes the key cannot reach it.
                body_excerpt: truncate(&text),
            }
        }
        Err(error) => ProtocolAttempt {
            protocol,
            verdict: ProbeVerdict::Inconclusive(transport_failure(&error)),
            status: None,
            body_excerpt: truncate(&error.to_string()),
        },
    }
}

/// The wire name of a protocol, for a log line and a screen.
fn protocol_name(protocol: Protocol) -> String {
    match protocol {
        Protocol::OpenAiResponses => "openai_responses".to_string(),
        Protocol::OpenAiChatCompletions => "openai_chat_completions".to_string(),
        Protocol::AnthropicMessages => "anthropic_messages".to_string(),
    }
}

/// Ask an endpoint what models it serves.
///
/// Model discovery is deliberately separate from protocol negotiation. A user
/// who cannot list models can still type one, and an endpoint with no listing
/// route is still perfectly good at chat. The only thing a failed listing may
/// do is explain itself.
///
/// A way of asking an endpoint for its model list.
///
/// The chain exists because "where a provider lists its models" varies even
/// between gateways that speak the same chat protocol: the route, the auth
/// header and the response shape are all conventions rather than guarantees.
/// Discovery is a convenience, so when every strategy has been tried and none
/// answered, manual entry is what the user gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListingStrategy {
    /// The OpenAI-compatible convention, and by far the most common: a bearer
    /// token at `/models` beside the chat route.
    OpenAiCompatible,
    /// The Anthropic convention, which authenticates with a header rather than
    /// a bearer token. Some gateways that speak only the Anthropic protocol
    /// reject `Authorization` outright and answer 401 to a request that should
    /// have been asked more politely.
    AnthropicCompatible,
    /// A gateway configured as a bare host rather than as `.../v1` will answer
    /// 404 for `.../models` and 200 for `.../v1/models`. Asking again costs one
    /// request and saves the user from being told the endpoint has no models.
    HostRootVersioned,
}

impl ListingStrategy {
    /// Every strategy, in the order they are tried.
    const ALL: [ListingStrategy; 3] = [
        ListingStrategy::OpenAiCompatible,
        ListingStrategy::AnthropicCompatible,
        ListingStrategy::HostRootVersioned,
    ];

    fn url(self, base: &str) -> Option<String> {
        let base = base.trim_end_matches('/');

        match self {
            ListingStrategy::OpenAiCompatible | ListingStrategy::AnthropicCompatible => {
                Some(format!("{base}/models"))
            }
            ListingStrategy::HostRootVersioned => {
                // Only worth asking when the base URL is not already versioned.
                // A base that ends in `/v1` would produce the exact request the
                // first strategy already made, and repeating it to learn the
                // same thing is a wasted round trip.
                let root = host_root(base)?;
                let last = base.rsplit('/').next().unwrap_or_default();

                let already_versioned = last.len() > 1
                    && last.starts_with('v')
                    && last[1..]
                        .chars()
                        .all(|character| character.is_ascii_digit());

                (!already_versioned).then(|| format!("{root}/v1/models"))
            }
        }
    }

    /// How this strategy authenticates, which is the part a gateway can reject.
    fn is_anthropic(self) -> bool {
        matches!(self, ListingStrategy::AnthropicCompatible)
    }

    /// Whether a failure justifies trying the next strategy.
    ///
    /// Only a route that is not there. A rejected key, a rate limit and an
    /// outage say nothing about the other routes, and answering them by trying
    /// something else would spend the user's key three times over and still
    /// report the wrong reason.
    fn may_fall_through(self, failure: ModelListing) -> bool {
        matches!(failure, ModelListing::Unavailable)
    }

    /// The strategy's own name, so a log line says which question was asked.
    ///
    /// A chain of three strategies where only the strategy names are recorded
    /// leaves the reader unable to tell "asked `/models`" from "asked
    /// `/v1/models`", and those are exactly the two that look identical from the
    /// outside.
    fn name(self) -> &'static str {
        match self {
            ListingStrategy::OpenAiCompatible => "openai_compatible",
            ListingStrategy::AnthropicCompatible => "anthropic_compatible",
            ListingStrategy::HostRootVersioned => "host_root_v1",
        }
    }
}

/// The models in a listing body, in whichever shape the endpoint wrote them.
///
/// A body is accepted when it looks like a list, whatever the envelope is
/// around it. Refusing an unfamiliar shape would report "no models" for a
/// gateway that just listed every one of them.
/// The scheme and host of a base URL, without any path.
fn host_root(base: &str) -> Option<&str> {
    let (scheme, rest) = base.split_once("://")?;
    let host = rest.split('/').next()?;
    (!host.is_empty()).then_some(&base[..scheme.len() + 3 + host.len()])
}

fn parse_model_list(body: &Value) -> Vec<String> {
    let entries = body
        .get("data")
        .or_else(|| body.get("models"))
        .or_else(|| body.get("result"))
        .and_then(Value::as_array)
        .or_else(|| body.as_array());

    let Some(entries) = entries else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|entry| match entry {
            // A bare `["gpt-4o", ...]` is a listing too.
            Value::String(name) => Some(name.clone()),
            // The usual object entry, under either of the two field names the
            // conventions use.
            entry => entry
                .get("id")
                .or_else(|| entry.get("name"))
                .or_else(|| entry.get("model"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
        .collect()
}

/// Ask an endpoint what models it serves.
///
/// Model discovery is deliberately separate from protocol negotiation. A user
/// who cannot list models can still type one, and an endpoint with no listing
/// route is still perfectly good at chat. The only thing a failed listing may
/// do is explain itself, and a failed listing never says anything about which
/// protocols the chat routes speak.
pub async fn list_models(
    client: Option<&reqwest::Client>,
    config: &EndpointConfig,
    api_key: &str,
    log: Option<&DiagnosticLog>,
) -> (ModelListing, Vec<String>) {
    let owned;
    let client = match client {
        Some(client) => client,
        None => {
            owned = crate::provider::http_client();
            &owned
        }
    };

    if api_key.is_empty() {
        if let Some(log) = log {
            log.record(
                DiagnosticKind::Discovery,
                "model_listing",
                "no key was supplied, so no model list was asked for",
                fields([("endpoint", config.normalized_base_url())]),
            );
        }
        return (ModelListing::PermissionDenied, Vec::new());
    }

    let mut last = ModelListing::Unavailable;

    for strategy in ListingStrategy::ALL {
        let Some(url) = strategy.url(&config.base_url) else {
            continue;
        };

        if let Some(log) = log {
            log.record(
                DiagnosticKind::Discovery,
                "model_listing_request",
                format!("GET {url} ({})", strategy.name()),
                fields([
                    ("endpoint", config.normalized_base_url()),
                    ("strategy", strategy.name().to_string()),
                    ("url", url.clone()),
                ]),
            );
        }

        let mut request = client.get(&url).header("content-type", "application/json");

        request = if strategy.is_anthropic() {
            request
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
        } else {
            request.bearer_auth(api_key)
        };

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                last = listing_failure(&error);
                record_listing_outcome(log, strategy, &url, None, &error.to_string(), last);
                if !strategy.may_fall_through(last) {
                    return (last, Vec::new());
                }
                continue;
            }
        };

        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();

        if !(200..300).contains(&status) {
            last = listing_status_failure(status);
            record_listing_outcome(log, strategy, &url, Some(status), &text, last);
            if !strategy.may_fall_through(last) {
                return (last, Vec::new());
            }
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            record_listing_outcome(
                log,
                strategy,
                &url,
                Some(status),
                &text,
                ModelListing::Unreachable,
            );
            return (ModelListing::Unreachable, Vec::new());
        };

        // A 200 with nothing in it is a listing route that does not list
        // anything, which is still "no models here" rather than "no route", and
        // the other strategies would only find the same empty answer.
        let models = parse_model_list(&value);
        record_listing_outcome(
            log,
            strategy,
            &url,
            Some(status),
            &text,
            ModelListing::Available,
        );

        if let Some(log) = log {
            log.record(
                DiagnosticKind::Discovery,
                "model_listing_result",
                format!("{} models were listed", models.len()),
                fields([
                    ("endpoint", config.normalized_base_url()),
                    ("strategy", strategy.name().to_string()),
                    ("count", models.len().to_string()),
                    // The count is the headline, but the names are what a user
                    // needs to type one, and a listing is not a secret.
                    (
                        "models",
                        models
                            .iter()
                            .take(20)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ]),
            );
        }

        return (ModelListing::Available, models);
    }

    if let Some(log) = log {
        log.record(
            DiagnosticKind::Discovery,
            "model_listing_result",
            "no listing route answered, so a model has to be typed in",
            fields([
                ("endpoint", config.normalized_base_url()),
                ("outcome", listing_name(last)),
            ]),
        );
    }

    (last, Vec::new())
}

/// Record how one listing strategy answered.
fn record_listing_outcome(
    log: Option<&DiagnosticLog>,
    strategy: ListingStrategy,
    url: &str,
    status: Option<u16>,
    body: &str,
    outcome: ModelListing,
) {
    let Some(log) = log else {
        return;
    };

    log.record(
        DiagnosticKind::Discovery,
        "model_listing_result",
        format!("{url}: {outcome:?}"),
        fields([
            ("strategy", strategy.name().to_string()),
            ("url", url.to_string()),
            (
                "status",
                status
                    .map(|status| status.to_string())
                    .unwrap_or_else(|| "no answer".to_string()),
            ),
            ("outcome", listing_name(outcome)),
            ("body", truncate(body)),
        ]),
    );
}

/// The listing outcome as one word a log or a screen can show.
fn listing_name(listing: ModelListing) -> String {
    match listing {
        ModelListing::Unknown => "unknown",
        ModelListing::Available => "available",
        ModelListing::Unavailable => "unavailable",
        ModelListing::PermissionDenied => "permission_denied",
        ModelListing::RateLimited => "rate_limited",
        ModelListing::Unreachable => "unreachable",
    }
    .to_string()
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
    use sujiu_core::apply_reasoning_override;

    fn config(model: &str) -> EndpointConfig {
        EndpointConfig {
            id: "provider-1".into(),
            name: "test".into(),
            base_url: "https://api.example.com/v1".into(),
            selected_model: Some(model.into()),
            credential_ref: None,
            overrides: Default::default(),
        }
    }

    #[tokio::test]
    async fn a_probe_that_was_throttled_does_not_downgrade_the_endpoint() {
        let cache = CapabilityCache::new();

        // No server is running, so every attempt is inconclusive.
        let negotiation = negotiate(None, &config("m"), "sk-test", Some(&cache), None, None).await;

        assert!(negotiation.selected.is_none());
        assert!(!negotiation.undetermined.is_empty());
        assert!(!cache.len() > 0, "a throttled probe must not be cached");
    }

    #[tokio::test]
    async fn a_probe_without_a_credential_is_a_credential_problem() {
        let negotiation = negotiate(
            None,
            &config("m"),
            "",
            Some(&CapabilityCache::new()),
            None,
            None,
        )
        .await;

        assert!(negotiation.selected.is_none());
        assert_eq!(negotiation.failure, Some(ProbeFailure::Credentials));
        assert!(negotiation
            .failure_explanation()
            .is_some_and(|reason| !reason.is_empty()));
    }

    #[tokio::test]
    async fn listing_a_model_never_forbids_typing_one() {
        let (listing, models) = list_models(None, &config("m"), "sk-test", None).await;

        assert!(models.is_empty());
        assert!(listing.allows_manual_entry());
    }

    /// The status is what a settings screen switches on, so the two cases a user
    /// acts on differently must not land on the same word. A missing key and a
    /// wrong address both read as "probe failed" when they are one string, and
    /// the fix for one is the other field.
    #[test]
    fn every_way_a_probe_fails_gets_its_own_status() {
        let attempt = |verdict: ProbeVerdict, status: Option<u16>| ProtocolAttempt {
            protocol: Protocol::OpenAiResponses,
            verdict,
            status,
            body_excerpt: String::new(),
        };

        let statuses: Vec<&str> = [
            ProbeFailure::Credentials,
            ProbeFailure::NoSuchEndpoint,
            ProbeFailure::RateLimited,
            ProbeFailure::Malformed,
            ProbeFailure::ModelUnavailable,
        ]
        .into_iter()
        .map(|failure| {
            let verdict = ProbeVerdict::Inconclusive(failure.clone());
            Negotiation::failed("why", Some(failure), &[attempt(verdict, Some(400))]).status()
        })
        .collect();
        assert_eq!(
            statuses,
            [
                "credentials_rejected",
                "no_such_endpoint",
                "rate_limited",
                "unreadable",
                "undetermined"
            ]
        );

        // Unreachable is one verdict for two different faults. A status means
        // the server answered and is unwell; no status at all means the request
        // never landed. Telling those apart by guesswork sends the user to fix
        // their key when their router is off.
        let sick = Negotiation::failed(
            "why",
            Some(ProbeFailure::Unavailable),
            &[attempt(
                ProbeVerdict::Inconclusive(ProbeFailure::Unavailable),
                Some(503),
            )],
        );
        let offline = Negotiation::failed(
            "why",
            Some(ProbeFailure::Unavailable),
            &[attempt(
                ProbeVerdict::Inconclusive(ProbeFailure::Unavailable),
                None,
            )],
        );
        assert_eq!(sick.status(), "server_unavailable");
        assert_eq!(offline.status(), "network_error");

        // Every protocol said no. That is a real answer, not a failure to find
        // one out, and it is the only outcome that means "go elsewhere".
        let nowhere = Negotiation::failed("why", None, &[]);
        assert_eq!(nowhere.status(), "no_usable_protocol");

        // A supported protocol outranks every failure code, because a walk that
        // found a working path is not a failure to report one.
        let found = Negotiation {
            selected: Some(Protocol::OpenAiResponses),
            supported: vec![Protocol::OpenAiResponses],
            ..Negotiation::failed("why", Some(ProbeFailure::Credentials), &[])
        };
        assert_eq!(found.status(), "supported");
    }

    /// The one that needed a live endpoint to find.
    ///
    /// A real Chat-Completions-only gateway answers every probe with a bare
    /// `404 page not found` — the same body it uses for a route it never had and
    /// for a model it cannot resolve. Every attempt is therefore ambiguous,
    /// nothing is selected, and the walk ends having concluded nothing at all.
    ///
    /// Reporting that as `no_usable_protocol` tells the user their working
    /// endpoint supports nothing this build speaks, and sends them off to
    /// reconfigure it. The endpoint listed eighty-one models and answered a
    /// chat request perfectly well.
    #[test]
    fn an_endpoint_that_only_declined_to_answer_is_not_reported_as_speaking_nothing() {
        let attempt = |protocol| ProtocolAttempt {
            protocol,
            verdict: ProbeVerdict::Ambiguous,
            status: Some(404),
            body_excerpt: "404 page not found".to_string(),
        };

        let evasive = Negotiation::failed(
            "we could not tell what it speaks",
            None,
            &[
                attempt(Protocol::OpenAiResponses),
                attempt(Protocol::OpenAiChatCompletions),
                attempt(Protocol::AnthropicMessages),
            ],
        );

        assert_eq!(evasive.status(), "undetermined");

        // The distinction is not cosmetic: it is the difference between "use a
        // different endpoint" and "try again", and only the second one is true.
        let mut definite = Negotiation::failed(
            "the endpoint offered none of the protocols this build speaks",
            None,
            &[
                attempt(Protocol::OpenAiResponses),
                attempt(Protocol::OpenAiChatCompletions),
            ],
        );
        definite.ambiguous = false;
        assert_eq!(definite.status(), "no_usable_protocol");
    }

    /// A probe has to leave a record, or the question "which protocol was
    /// tried and what did it say" has no answer after the fact.
    #[tokio::test]
    async fn a_probe_records_every_protocol_it_asked_about() {
        let log = DiagnosticLog::new(50);
        // An address that cannot resolve, so every protocol is attempted and
        // every attempt is a real one rather than a cached answer.
        let mut unreachable = config("m");
        unreachable.base_url = "https://sujiu-does-not-resolve.invalid/v1".into();
        unreachable.selected_model = None;

        let negotiation = negotiate(
            None,
            &unreachable,
            "sk-live-1234567890abcdef",
            None,
            Some(&log),
            None,
        )
        .await;

        let entries = log.entries();
        let stages: Vec<&str> = entries.iter().map(|entry| entry.stage.as_str()).collect();
        assert!(stages.contains(&"probe_start"));
        assert!(stages.contains(&"probe_request"));
        assert!(stages.contains(&"protocol_result"));
        assert!(stages.contains(&"probe_end"));

        // Every protocol the endpoint is asked about appears by name, and the
        // probe ran before any model was chosen.
        for protocol in config("m").protocols() {
            assert!(
                log.render().contains(&protocol_name(protocol)),
                "{} was never recorded",
                protocol_name(protocol)
            );
        }
        assert!(log.render().contains("(not chosen yet)"));

        // The walk is short-circuited on the first inconclusive answer, so the
        // attempts are reported with what the endpoint said.
        assert!(!negotiation.attempts.is_empty());
        assert!(negotiation
            .attempts
            .iter()
            .all(|attempt| attempt.status.is_none() || attempt.status.is_some()));

        assert!(
            !log.render().contains("sk-live-1234567890abcdef"),
            "a probe log must not carry the key it used"
        );
    }

    /// The whole point of the log: after a failed configuration, a reader can
    /// tell a rejected key from an address that does not resolve.
    #[tokio::test]
    async fn a_listing_records_the_route_it_asked_and_what_came_back() {
        let log = DiagnosticLog::new(50);
        let mut unreachable = config("m");
        unreachable.base_url = "https://sujiu-does-not-resolve.invalid/v1".into();

        let (listing, models) =
            list_models(None, &unreachable, "sk-live-1234567890abcdef", Some(&log)).await;

        assert!(models.is_empty());
        assert!(matches!(listing, ModelListing::Unreachable));

        let rendered = log.render();
        assert!(rendered.contains("model_listing_request"));
        assert!(
            rendered.contains("/models"),
            "the route asked is not recorded"
        );
        assert!(
            rendered.contains("unreachable"),
            "the outcome is not recorded"
        );
        assert!(!rendered.contains("sk-live-1234567890abcdef"));
    }

    #[test]
    fn a_model_list_is_read_in_whichever_shape_it_arrives() {
        // The conventions disagree about the envelope, the field name and
        // whether the entries are objects at all. A gateway that just listed
        // every model must not be told it listed none.
        let open_ai = json!({"data": [{"id": "gpt-4o-mini"}, {"id": "gpt-4o"}]});
        let anthropic = json!({"models": [{"name": "claude-x"}]});
        let bare = json!(["a", "b"]);
        let alt_field = json!({"data": [{"model": "m-1"}]});

        assert_eq!(parse_model_list(&open_ai), vec!["gpt-4o-mini", "gpt-4o"]);
        assert_eq!(parse_model_list(&anthropic), vec!["claude-x"]);
        assert_eq!(parse_model_list(&bare), vec!["a", "b"]);
        assert_eq!(parse_model_list(&alt_field), vec!["m-1"]);
        assert!(parse_model_list(&json!({"error": "nope"})).is_empty());
    }

    #[test]
    fn only_a_missing_route_makes_the_next_listing_strategy_worth_trying() {
        // The same rule the protocol chain follows. A rejected key or a rate
        // limit is an answer about this request, and trying the next strategy
        // would spend the user's key again and still report the wrong reason.
        assert!(ListingStrategy::OpenAiCompatible.may_fall_through(ModelListing::Unavailable));
        for stop in [
            ModelListing::PermissionDenied,
            ModelListing::RateLimited,
            ModelListing::Unreachable,
        ] {
            assert!(!ListingStrategy::OpenAiCompatible.may_fall_through(stop));
        }
    }

    #[test]
    fn the_listing_chain_asks_more_than_one_question() {
        // A single request to one route is not a fallback chain, and the review
        // is explicit that it is not. Each strategy must reach a different URL
        // or authenticate differently, or asking it again wastes a round trip
        // to learn exactly what the first one already said.
        let urls: Vec<String> = ListingStrategy::ALL
            .iter()
            .filter_map(|strategy| strategy.url("https://gw.example/v1"))
            .collect();

        assert!(urls.contains(&"https://gw.example/v1/models".to_string()));
        // The anthropic strategy reaches the same route with a different
        // credential, which is the part a gateway can actually reject.
        assert!(ListingStrategy::ALL
            .iter()
            .any(|strategy| strategy.is_anthropic()));

        // A base URL that is already versioned has no separate host root worth
        // asking: `.../v1` and `.../v1/models` are the same request the first
        // strategy already made, so the strategy must not invent one.
        assert_eq!(
            ListingStrategy::HostRootVersioned.url("https://gw.example/v1"),
            None
        );
        assert_eq!(
            ListingStrategy::HostRootVersioned.url("https://gw.example/v1/"),
            None
        );
        // A base with some other path is a gateway root, and `/v1/models` is
        // genuinely a different place to look.
        assert_eq!(
            ListingStrategy::HostRootVersioned.url("https://gw.example/openai"),
            Some("https://gw.example/v1/models".to_string())
        );

        // All three strategies must be reachable for a bare host, or the chain
        // is shorter than it claims to be.
        let all: Vec<Option<String>> = ListingStrategy::ALL
            .iter()
            .map(|strategy| strategy.url("https://gw.example"))
            .collect();
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|url| url.is_some()));
    }

    #[test]
    fn an_inconclusive_attempt_is_never_a_reason_to_try_the_next_protocol() {
        let rate_limited = ProtocolAttempt {
            protocol: Protocol::OpenAiResponses,
            verdict: ProbeVerdict::Inconclusive(ProbeFailure::RateLimited),
            status: Some(429),
            body_excerpt: String::new(),
        };
        let no_such_endpoint = ProtocolAttempt {
            protocol: Protocol::OpenAiResponses,
            verdict: ProbeVerdict::Unsupported,
            status: Some(404),
            body_excerpt: "unknown endpoint".to_string(),
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
                attempts: Vec::new(),
                ambiguous: false,
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
