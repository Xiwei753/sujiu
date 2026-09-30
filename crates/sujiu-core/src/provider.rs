use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::protocol::{EndpointCapabilities, Protocol};

/// What a user has to tell us about an endpoint.
///
/// There is deliberately no vendor here. A user gives a base URL and a key; the
/// runtime asks the endpoint what it speaks and how well. A `ProviderKind` used
/// to sit in this struct, which was two mistakes at once: it asked the user to
/// know something the runtime can find out, and it reordered protocol
/// negotiation so the common case tried Chat Completions before Responses —
/// the opposite of the priority we settled on. A name cannot tell a gateway
/// from the vendor behind it, a model called `deepseek-reasoner` on an endpoint
/// that strips thinking from a real thinking endpoint serving the same name, and
/// both go stale the moment anything is renamed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EndpointConfig {
    pub id: String,
    /// A display name only. It is never read back to make a decision.
    #[serde(default)]
    pub name: String,
    pub base_url: String,

    /// The model this endpoint will be used with.
    ///
    /// Optional because discovery runs before a model is known: the endpoint is
    /// asked what it can do with a URL and a key alone, and the model is chosen
    /// afterwards from what it offered or typed by hand.
    #[serde(default)]
    pub selected_model: Option<String>,

    /// Reference to a secret held by the platform secure-storage layer.
    /// API keys are intentionally not part of the shared core config.
    #[serde(default)]
    pub credential_ref: Option<String>,

    /// Advanced compatibility overrides. Never the normal path.
    #[serde(default)]
    pub overrides: Map<String, Value>,
}

impl EndpointConfig {
    /// The base URL in the form a capability answer is cached under.
    ///
    /// Case and a trailing slash vary between how a user types an endpoint and
    /// how a service documents it, and both name the same endpoint. Left
    /// unnormalised they would be cached as two different endpoints, so a
    /// probe would be repeated and a stale answer could survive a correction.
    /// The path is kept: `/v1` and `/v2` on one host are different APIs.
    pub fn normalized_base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);

        // Only the scheme and host are case-insensitive. A path may legitimately
        // differ in case, so it is left alone.
        match trimmed.split_once("://") {
            Some((scheme, rest)) => {
                let (host, path) = match rest.find('/') {
                    Some(at) => rest.split_at(at),
                    None => (rest, ""),
                };

                format!(
                    "{}://{}{}",
                    scheme.to_ascii_lowercase(),
                    host.to_ascii_lowercase(),
                    path
                )
                .trim_end_matches('/')
                .to_string()
            }
            None => trimmed.to_string(),
        }
    }

    /// Where this endpoint's negotiated capabilities are cached.
    pub fn capability_key(&self) -> String {
        EndpointCapabilities::cache_key(&self.id, &self.normalized_base_url())
    }

    /// The order protocols are tried in, which is the same for every endpoint.
    ///
    /// There is nothing to configure and nothing to derive from a name. An
    /// endpoint that only speaks Anthropic Messages is found by Responses and
    /// Chat Completions both answering that they are not there, which is a
    /// question only the endpoint can answer.
    pub fn protocols(&self) -> Vec<Protocol> {
        Protocol::PRIORITY.to_vec()
    }

    /// An advanced compatibility override, or the negotiation's own answer.
    ///
    /// The override exists for a gateway the probe cannot reason about, and it
    /// is deliberately not the normal path: a capability that only a
    /// hand-written field can set is a feature a real user can never reach.
    pub fn forced_reasoning_replay(&self) -> Option<bool> {
        self.overrides
            .get(REPLAYS_ASSISTANT_REASONING_KEY)
            .and_then(Value::as_bool)
    }

    /// A friendly name for the endpoint, for a settings screen.
    ///
    /// This is the one place a hostname is allowed to say anything, and it says
    /// something only a human reads. It decides no protocol, no capability, no
    /// continuation and no tool behaviour, because a display label that
    /// changed how a conversation was spoken would be the same mistake wearing
    /// a different hat. An endpoint we do not recognise is named after its own
    /// host, which is also what a self-hosted gateway should be called.
    pub fn display_label(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.trim().to_string();
        }

        match known_vendor_label(host_of(&self.base_url)) {
            Some(label) => label.to_string(),
            None => host_of(&self.base_url)
                .unwrap_or_else(|| self.base_url.trim())
                .to_string(),
        }
    }
}

/// A friendly name for a host we recognise, for display only.
///
/// This table is here so a settings screen can say "DeepSeek" instead of
/// `api.deepseek.com`. It is a list of words to show, and it is not consulted
/// anywhere a decision is made. Adding an entry here cannot make a single
/// request behave differently.
fn known_vendor_label(host: Option<&str>) -> Option<&'static str> {
    let host = host?.to_ascii_lowercase();

    match host.as_str() {
        "api.openai.com" => Some("OpenAI"),
        "api.deepseek.com" | "api.deepseek.com.cn" => Some("DeepSeek"),
        "openrouter.ai" => Some("OpenRouter"),
        "api.anthropic.com" => Some("Anthropic"),
        "generativelanguage.googleapis.com" => Some("Google"),
        _ => None,
    }
}

/// The host part of a base URL, if it has one.
fn host_of(base_url: &str) -> Option<&str> {
    let rest = base_url.trim().split_once("://")?.1;
    let host = match rest.find('/') {
        Some(at) => &rest[..at],
        None => rest,
    };
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);

    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// The one advanced override key the runtime reads.
pub const REPLAYS_ASSISTANT_REASONING_KEY: &str = "replaysAssistantReasoning";

/// Combine what an endpoint was negotiated to be with an advanced override.
pub fn apply_reasoning_override(
    mut capabilities: EndpointCapabilities,
    forced: Option<bool>,
) -> EndpointCapabilities {
    if let Some(forced) = forced {
        capabilities.replays_assistant_reasoning = forced;
    }

    capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(base_url: &str) -> EndpointConfig {
        EndpointConfig {
            id: "e1".into(),
            name: String::new(),
            base_url: base_url.into(),
            selected_model: Some("some-model".into()),
            credential_ref: None,
            overrides: Map::new(),
        }
    }

    #[test]
    fn a_base_url_is_cached_under_one_spelling() {
        let a = endpoint("https://API.Example.com/v1");
        let b = endpoint("https://api.example.com/v1/");

        assert_eq!(a.normalized_base_url(), "https://api.example.com/v1");
        assert_eq!(a.capability_key(), b.capability_key());
    }

    #[test]
    fn a_different_path_is_a_different_endpoint() {
        // `/v1` and `/v2` on one host are different APIs, so folding them
        // together would serve a stale answer for the wrong one.
        assert_ne!(
            endpoint("https://api.example.com/v1").capability_key(),
            endpoint("https://api.example.com/v2").capability_key()
        );
    }

    #[test]
    fn every_endpoint_is_negotiated_in_the_same_order() {
        // The order is the one we settled on and nothing can change it. A
        // vendor name, a model name or a display label does not get a vote, so
        // there is nothing here to vary with.
        for base_url in [
            "https://api.deepseek.com/v1",
            "https://api.anthropic.com/v1",
            "https://gateway.example.com/openai/v1",
        ] {
            assert_eq!(
                endpoint(base_url).protocols(),
                Protocol::PRIORITY.to_vec(),
                "the order must not depend on the host"
            );
        }
    }

    #[test]
    fn nothing_about_a_vendor_or_a_model_name_decides_the_protocol() {
        for model in ["deepseek-reasoner", "r1", "thinking-v2", "gpt-4o-mini"] {
            let mut candidate = endpoint("https://api.deepseek.com/v1");
            candidate.selected_model = Some(model.into());

            assert_eq!(candidate.protocols(), Protocol::PRIORITY.to_vec());
            assert_eq!(candidate.forced_reasoning_replay(), None);
        }
    }

    #[test]
    fn a_host_may_name_itself_and_may_not_be_asked_anything() {
        // A friendly name, for a human to read.
        assert_eq!(
            endpoint("https://api.deepseek.com/v1").display_label(),
            "DeepSeek"
        );
        assert_eq!(
            endpoint("https://openrouter.ai/api/v1").display_label(),
            "OpenRouter"
        );

        // A gateway nobody has heard of is named after itself.
        assert_eq!(
            endpoint("https://llm.internal.example:8443/v1").display_label(),
            "llm.internal.example:8443"
        );

        // A name the user typed wins, so a self-hosted endpoint can be called
        // whatever the person running it calls it.
        let mut named = endpoint("https://api.deepseek.com/v1");
        named.name = "  The Night Shift  ".into();
        assert_eq!(named.display_label(), "The Night Shift");
    }

    #[test]
    fn a_label_is_the_only_thing_a_host_can_change() {
        // Two endpoints whose hosts are both recognised differ in what a
        // settings screen shows and in nothing else that matters. If this ever
        // starts failing because a host leaked into a decision, the label has
        // been promoted into a branch.
        let deepseek = endpoint("https://api.deepseek.com/v1");
        let openai = endpoint("https://api.openai.com/v1");

        assert_ne!(deepseek.display_label(), openai.display_label());
        assert_eq!(deepseek.protocols(), openai.protocols());
        assert_eq!(
            deepseek.forced_reasoning_replay(),
            openai.forced_reasoning_replay()
        );
        assert_eq!(
            deepseek.capability_key().split('|').nth(1),
            Some(deepseek.normalized_base_url().as_str())
        );
    }

    #[test]
    fn an_endpoint_may_be_known_before_a_model_is_chosen() {
        // Discovery runs on a URL and a key alone, so a model is not required
        // to ask the endpoint anything.
        let mut bare = endpoint("https://api.example.com/v1");
        bare.selected_model = None;

        assert!(bare.protocols().len() == Protocol::PRIORITY.len());
        assert!(!bare.capability_key().is_empty());
    }

    #[test]
    fn an_override_still_wins_when_someone_states_one() {
        let negotiated = EndpointCapabilities::negotiate(&[Protocol::OpenAiChatCompletions])
            .expect("one protocol supported");

        assert!(!apply_reasoning_override(negotiated.clone(), None).replays_assistant_reasoning);
        assert!(
            apply_reasoning_override(negotiated.clone(), Some(true)).replays_assistant_reasoning
        );
        assert!(!apply_reasoning_override(negotiated, Some(false)).replays_assistant_reasoning);
    }
}
