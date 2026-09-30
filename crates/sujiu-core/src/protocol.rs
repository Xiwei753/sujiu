//! What an endpoint speaks, and how we found out.
//!
//! Protocol capability belongs to an endpoint, not to a vendor's name or a
//! model's name. A model called `deepseek-reasoner` behind a gateway that
//! strips thinking, and the same model name served by a thinking endpoint, are
//! two different conversations; a name cannot tell them apart, and it goes
//! stale the moment a vendor renames anything. So the runtime asks the
//! endpoint, and the answer is a [`Protocol`] plus a set of
//! [`EndpointCapabilities`].
//!
//! The order below is the order we prefer, and it is deliberate:
//!
//! ```text
//! OpenAI Responses -> Chat Completions -> Anthropic Messages
//! ```
//!
//! Responses comes first because an endpoint that speaks it carries native
//! continuation, reasoning and tool state, and degrading it to Chat
//! Completions means bolting sidecar fields onto a protocol that has no place
//! for them. Chat Completions is the common denominator: most third-party
//! gateways speak some version of it and nothing else. Anthropic is last
//! because almost no endpoint offers Anthropic Messages *without* an
//! OpenAI-compatible interface, so building the abstraction around it would
//! mean designing for the rarest case.
//!
//! Model listing is deliberately not here. Where a provider lists its models
//! says nothing about which protocol its chat endpoint speaks, and a listing
//! failure must never be read as one. It has its own
//! [`ModelListing`] result so the two concerns cannot be confused.

use serde::{Deserialize, Serialize};

/// A wire protocol the runtime can talk to an endpoint with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// The OpenAI Responses API, with its own continuation and item model.
    OpenAiResponses,
    /// OpenAI-compatible Chat Completions. The common denominator.
    OpenAiChatCompletions,
    /// Anthropic Messages.
    AnthropicMessages,
}

impl Protocol {
    /// The order we prefer protocols in.
    pub const PRIORITY: [Protocol; 3] = [
        Protocol::OpenAiResponses,
        Protocol::OpenAiChatCompletions,
        Protocol::AnthropicMessages,
    ];

    /// Whether this protocol can resume a conversation from provider state.
    ///
    /// Only Responses has a first-class handle. A Chat Completions completion
    /// id is a label, not something you can continue from.
    pub fn native_continuation(&self) -> bool {
        matches!(self, Protocol::OpenAiResponses)
    }

    /// Whether a caller can hand the endpoint's own protocol a path.
    pub fn path(&self) -> &'static str {
        match self {
            Protocol::OpenAiResponses => "/responses",
            Protocol::OpenAiChatCompletions => "/chat/completions",
            Protocol::AnthropicMessages => "/messages",
        }
    }

    /// The first protocol in priority order that `supported` says we may use.
    pub fn negotiate(supported: &[Protocol]) -> Option<Protocol> {
        Self::PRIORITY
            .into_iter()
            .find(|protocol| supported.contains(protocol))
    }
}

/// How a probe of one protocol ended.
///
/// The distinction that matters is [`ProbeVerdict::Unsupported`] versus
/// everything else. Falling through to the next protocol is only ever correct
/// when the endpoint said it does not have this one. A wrong key, a rate
/// limit and an outage are all answers about *this moment*, and treating them
/// as "this endpoint does not speak that protocol" permanently downgrades a
/// working provider because of a transient condition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum ProbeVerdict {
    /// The endpoint answered as this protocol.
    Supported,
    /// The endpoint does not have this protocol. Only this may fall through.
    Unsupported,
    /// We could not tell. The reason is kept so it is not mistaken for a
    /// capability, and so a later probe can be worth retrying.
    Inconclusive(ProbeFailure),
}

impl ProbeVerdict {
    pub fn is_supported(&self) -> bool {
        matches!(self, ProbeVerdict::Supported)
    }

    /// Whether a negotiation may move on to the next protocol.
    pub fn may_fall_through(&self) -> bool {
        matches!(self, ProbeVerdict::Unsupported)
    }
}

/// Why a probe could not answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeFailure {
    /// The endpoint answered, and the answer means this protocol is not here.
    /// 404, or an explicit "unknown endpoint" of the kind services use for a
    /// path they do not implement.
    NoSuchEndpoint,
    /// 401/403. The key is missing, wrong, or not allowed to use this.
    /// Says nothing at all about which protocols exist.
    Credentials,
    /// 429. We are being throttled. The protocol is not the problem.
    RateLimited,
    /// 5xx, or the connection failed. A service problem, not a capability.
    Unavailable,
    /// The endpoint answered with something we could not interpret.
    Malformed,
}

impl ProbeFailure {
    /// Whether this failure is worth retrying later.
    pub fn is_transient(&self) -> bool {
        matches!(self, ProbeFailure::RateLimited | ProbeFailure::Unavailable)
    }

    /// A human-readable reason, for a settings screen that has to say why.
    pub fn describe(&self) -> &'static str {
        match self {
            ProbeFailure::NoSuchEndpoint => "this endpoint does not offer that protocol",
            ProbeFailure::Credentials => {
                "the key was rejected, so the endpoint's protocols could not be checked"
            }
            ProbeFailure::RateLimited => {
                "the endpoint is rate limiting us, so the check did not run"
            }
            ProbeFailure::Unavailable => "the endpoint did not answer, so nothing can be concluded",
            ProbeFailure::Malformed => "the endpoint answered in a way we could not read",
        }
    }
}

/// Classify a probe outcome. Returns the verdict for one protocol attempt.
pub fn classify(status: Option<u16>, body: &str) -> ProbeVerdict {
    let Some(status) = status else {
        return ProbeVerdict::Inconclusive(ProbeFailure::Unavailable);
    };

    match status {
        200..=299 => ProbeVerdict::Supported,
        // A path the service does not implement. This is the one answer that
        // legitimately means "try the next protocol".
        404 | 405 | 501 => ProbeVerdict::Unsupported,
        401 | 403 => ProbeVerdict::Inconclusive(ProbeFailure::Credentials),
        429 => ProbeVerdict::Inconclusive(ProbeFailure::RateLimited),
        400 | 422 => {
            // A rejected request is only evidence about the *request*, not
            // about which protocols exist. A gateway that answers every path
            // with 400 would otherwise look like it supports nothing, and a
            // real endpoint answering 400 to a probe shape is a question about
            // our probe, not about its capabilities.
            if says_endpoint_is_unknown(body) {
                ProbeVerdict::Unsupported
            } else {
                ProbeVerdict::Inconclusive(ProbeFailure::Malformed)
            }
        }
        500..=599 => ProbeVerdict::Inconclusive(ProbeFailure::Unavailable),
        _ => ProbeVerdict::Inconclusive(ProbeFailure::Malformed),
    }
}

/// Whether a rejection body clearly says the endpoint is not implemented.
///
/// Some gateways answer an unknown path with 400 rather than 404. That is only
/// evidence when the message is about the path.
///
/// The phrases therefore all name an endpoint, route or path, and a bare "not
/// found" is deliberately not among them. "model not found" and "no such
/// model" are the most common 400 a usable endpoint returns, and matching
/// either would downgrade a perfectly good endpoint on the first typo.
fn says_endpoint_is_unknown(body: &str) -> bool {
    let body = body.to_ascii_lowercase();

    [
        "unknown endpoint",
        "unknown path",
        "no such endpoint",
        "unsupported endpoint",
        "endpoint not found",
        "route not found",
        "not implemented",
        "no route",
    ]
    .iter()
    .any(|phrase| body.contains(phrase))
}

/// Whether an endpoint lists its models, and if not, why.
///
/// Separate from [`Protocol`] on purpose. A provider with no listing endpoint
/// may still serve every protocol it claims, and a provider whose listing
/// needs a permission the key lacks is still perfectly usable for chat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelListing {
    /// Not asked yet.
    #[default]
    Unknown,
    /// The endpoint listed models.
    Available,
    /// There is no listing here, and that is fine. Manual entry still works.
    Unavailable,
    /// The key may not list models. Manual entry still works.
    PermissionDenied,
    /// Transient. Worth trying again later.
    RateLimited,
    /// The service was unreachable. Worth trying again later.
    Unreachable,
}

impl ModelListing {
    /// Whether a caller may still type a model by hand.
    ///
    /// Always true: discovery exists to save typing, never to gate it.
    pub fn allows_manual_entry(&self) -> bool {
        true
    }

    /// Whether it is worth asking again later.
    pub fn is_transient(&self) -> bool {
        matches!(self, ModelListing::RateLimited | ModelListing::Unreachable)
    }

    /// Why a listing did not happen, for a screen that has to explain itself.
    pub fn describe(&self) -> &'static str {
        match self {
            ModelListing::Unknown => "models have not been looked up yet",
            ModelListing::Available => "the endpoint lists its models",
            ModelListing::Unavailable => {
                "this endpoint has no model list, so a model has to be typed in"
            }
            ModelListing::PermissionDenied => {
                "this key may not list models, so a model has to be typed in"
            }
            ModelListing::RateLimited => {
                "the endpoint is rate limiting the lookup, so a model has to be typed in for now"
            }
            ModelListing::Unreachable => {
                "the endpoint could not be reached, so a model has to be typed in for now"
            }
        }
    }
}

/// What we negotiated with an endpoint, and what it turned out to be capable of.
///
/// This is provider-layer state, not a wire type. It is never persisted into a
/// transcript: the transcript stays portable, and this decides only how the
/// portable transcript is spoken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointCapabilities {
    /// The protocol we picked, by priority.
    pub protocol: Protocol,
    /// Every protocol the endpoint answered for, in priority order.
    pub supported: Vec<Protocol>,
    /// Protocols we could not rule in or out on this attempt.
    pub undetermined: Vec<Protocol>,
    pub streaming: bool,
    pub tools: bool,
    /// Whether an assistant's reasoning must travel with its next message.
    ///
    /// A property of the endpoint's protocol, and only meaningful on a
    /// protocol that has a field for it.
    pub replays_assistant_reasoning: bool,
    pub model_listing: ModelListing,
}

impl EndpointCapabilities {
    /// The one protocol to speak, given what the endpoint answered for.
    ///
    /// Returning `None` means the endpoint supports nothing we can talk to.
    pub fn negotiate(supported: &[Protocol]) -> Option<Self> {
        Protocol::negotiate(supported).map(|protocol| {
            let supported: Vec<Protocol> = Protocol::PRIORITY
                .into_iter()
                .filter(|candidate| supported.contains(candidate))
                .collect();

            Self {
                protocol,
                streaming: true,
                tools: true,
                replays_assistant_reasoning: false,
                supported,
                undetermined: Vec::new(),
                model_listing: ModelListing::Unknown,
            }
        })
    }

    /// The key this answer is cached under.
    ///
    /// A capability is a fact about an endpoint, so two models on the same
    /// endpoint share it and the same model on two endpoints does not.
    pub fn cache_key(provider_id: &str, normalized_base_url: &str) -> String {
        format!("{provider_id}|{normalized_base_url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_is_preferred_over_the_common_denominator() {
        assert_eq!(
            Protocol::negotiate(&[Protocol::OpenAiChatCompletions, Protocol::OpenAiResponses]),
            Some(Protocol::OpenAiResponses)
        );
    }

    #[test]
    fn chat_completions_is_preferred_over_anthropic() {
        assert_eq!(
            Protocol::negotiate(&[Protocol::AnthropicMessages, Protocol::OpenAiChatCompletions]),
            Some(Protocol::OpenAiChatCompletions)
        );
    }

    #[test]
    fn an_endpoint_supporting_nothing_we_speak_negotiates_to_nothing() {
        assert_eq!(Protocol::negotiate(&[]), None);
        assert!(EndpointCapabilities::negotiate(&[]).is_none());
    }

    #[test]
    fn only_a_missing_endpoint_lets_negotiation_move_on() {
        assert!(classify(Some(404), "").may_fall_through());
        assert!(classify(Some(405), "").may_fall_through());

        // A wrong key, a throttle and an outage are answers about this
        // moment. None of them may permanently downgrade a working endpoint.
        for status in [401, 403, 429, 500, 502, 503, 504] {
            assert!(
                !classify(Some(status), "").may_fall_through(),
                "status {status} must not be read as unsupported"
            );
        }

        assert!(!classify(None, "").may_fall_through());
    }

    #[test]
    fn a_rejected_request_is_only_evidence_when_it_is_about_the_endpoint() {
        // A gateway that answers every path with 400 must not look like it
        // supports nothing.
        assert!(!classify(Some(400), "{\"error\":\"bad request\"}").may_fall_through());
        assert!(!classify(Some(422), "model is required").may_fall_through());

        // But a gateway that says the path is unknown, with its usual 400,
        // is telling us something real.
        assert!(classify(Some(400), "unknown endpoint").may_fall_through());
        assert!(classify(Some(400), "Unknown Path").may_fall_through());
    }

    #[test]
    fn a_failure_keeps_whether_it_is_worth_retrying() {
        assert!(ProbeFailure::RateLimited.is_transient());
        assert!(ProbeFailure::Unavailable.is_transient());
        // A wrong key will still be wrong in a minute, and the endpoint's
        // protocols are not going to change because of it.
        assert!(!ProbeFailure::Credentials.is_transient());
    }

    #[test]
    fn listing_never_gates_typing_a_model() {
        for listing in [
            ModelListing::Unknown,
            ModelListing::Available,
            ModelListing::Unavailable,
            ModelListing::PermissionDenied,
            ModelListing::RateLimited,
            ModelListing::Unavailable,
        ] {
            assert!(listing.allows_manual_entry());
        }
    }

    #[test]
    fn only_responses_can_resume_from_provider_state() {
        assert!(Protocol::OpenAiResponses.native_continuation());
        // A Chat Completions completion id is a label, not a handle. Treating
        // it as one is the bug that put a dead handle back on the wire.
        assert!(!Protocol::OpenAiChatCompletions.native_continuation());
        assert!(!Protocol::AnthropicMessages.native_continuation());
    }

    #[test]
    fn capabilities_are_keyed_by_the_endpoint_not_the_model() {
        assert_eq!(
            EndpointCapabilities::cache_key("p1", "https://api.example/v1"),
            "p1|https://api.example/v1"
        );
    }

    /// The heart of the whole negotiation: a failure that says nothing about
    /// the protocol must never read as "this endpoint does not speak it".
    ///
    /// Getting this wrong is not a slow path, it is a wrong answer. A gateway
    /// that was briefly rate limited would be written down as not speaking Chat
    /// Completions, and every later turn would silently drop to whatever came
    /// next in the priority order.
    #[test]
    fn only_a_plain_404_says_the_protocol_is_not_there() {
        // The one shape that really does mean "no such endpoint here".
        assert_eq!(
            classify(Some(404), ""),
            ProbeVerdict::Unsupported,
            "a 404 on a path this endpoint does not serve is real evidence"
        );
        assert_eq!(classify(Some(405), ""), ProbeVerdict::Unsupported);
        assert_eq!(classify(Some(501), ""), ProbeVerdict::Unsupported);

        // Every one of these is a statement about credentials, load or health,
        // never about which protocols the endpoint speaks.
        for (status, expected) in [
            (401, ProbeFailure::Credentials),
            (403, ProbeFailure::Credentials),
            (429, ProbeFailure::RateLimited),
            (500, ProbeFailure::Unavailable),
            (502, ProbeFailure::Unavailable),
            (503, ProbeFailure::Unavailable),
        ] {
            let verdict = classify(Some(status), "");
            assert_eq!(
                verdict,
                ProbeVerdict::Inconclusive(expected),
                "status {status} must not be read as the protocol being missing"
            );
            assert!(
                !verdict.may_fall_through(),
                "status {status} must not send negotiation to the next protocol"
            );
        }

        // A transport that never answered says nothing at all.
        assert!(!classify(None, "").may_fall_through());
    }

    #[test]
    fn a_400_counts_as_missing_only_when_it_says_so() {
        // A model the endpoint rejected is a wrong request, not a wrong protocol.
        assert!(
            !classify(Some(400), r#"{"error":{"message":"model not found"}}"#).may_fall_through()
        );
        assert!(!classify(
            Some(400),
            r#"{"error":{"message":"context length exceeded"}}"#
        )
        .may_fall_through());

        // A gateway that answers 400 because it does not know the path does
        // mean the protocol is not there.
        assert_eq!(
            classify(
                Some(400),
                r#"{"error":{"message":"Unknown endpoint /v1/responses"}}"#
            ),
            ProbeVerdict::Unsupported
        );
        assert_eq!(
            classify(Some(422), "no such endpoint"),
            ProbeVerdict::Unsupported
        );
    }

    #[test]
    fn a_2xx_is_the_only_positive_evidence() {
        assert_eq!(classify(Some(200), "{}"), ProbeVerdict::Supported);
        assert_eq!(classify(Some(201), "{}"), ProbeVerdict::Supported);
        assert!(classify(Some(204), "").may_fall_through() == false);
    }

    #[test]
    fn a_transient_failure_is_distinguishable_from_a_dead_endpoint() {
        // This is what decides whether a cached answer may be written down: a
        // throttled or briefly broken service must not be remembered as
        // permanently lacking a protocol.
        assert!(ProbeFailure::RateLimited.is_transient());
        assert!(ProbeFailure::Unavailable.is_transient());
        assert!(!ProbeFailure::NoSuchEndpoint.is_transient());
        assert!(!ProbeFailure::Malformed.is_transient());

        // A credential problem is not transient, but it is still not evidence
        // about the protocol, so a different fix applies.
        assert!(!ProbeFailure::Credentials.is_transient());
    }

    #[test]
    fn a_failure_explains_itself_without_guessing_a_protocol() {
        // Each message has to distinguish "your key" from "their service" from
        // "that path", because the fix is different in every case and the user
        // is the one applying it.
        assert!(ProbeFailure::Credentials
            .describe()
            .to_lowercase()
            .contains("key"));
        assert!(ProbeFailure::RateLimited
            .describe()
            .to_lowercase()
            .contains("limit"));
        assert!(ProbeFailure::NoSuchEndpoint
            .describe()
            .to_lowercase()
            .contains("endpoint"));
    }
}
