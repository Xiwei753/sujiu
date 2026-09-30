use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::protocol::{EndpointCapabilities, Protocol};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,

    /// Reference to a secret held by the platform secure-storage layer.
    /// API keys are intentionally not part of the shared core config.
    #[serde(default)]
    pub credential_ref: Option<String>,

    #[serde(default)]
    pub extra: Map<String, Value>,
}

impl ProviderConfig {
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

    /// What this configuration should be negotiated against.
    ///
    /// Everything here comes from the provider layer; there is no vendor or
    /// model name in it. `kind` is a *preference* among endpoints we have not
    /// probed yet, not a claim about what the endpoint speaks, so it only
    /// reorders the list and never decides it.
    pub fn preferred_protocols(&self) -> Vec<Protocol> {
        let preferred = match self.kind {
            ProviderKind::Anthropic => Protocol::AnthropicMessages,
            // Gemini has no adapter yet, so it negotiates like anything else
            // rather than pretending to be a protocol of its own.
            ProviderKind::OpenAiCompatible | ProviderKind::Gemini => {
                Protocol::OpenAiChatCompletions
            }
        };

        let mut ordered = vec![preferred];
        ordered.extend(
            Protocol::PRIORITY
                .into_iter()
                .filter(|protocol| *protocol != preferred),
        );

        ordered
    }

    /// An advanced compatibility override, or the negotiation's own answer.
    ///
    /// The override exists for a gateway the probe cannot reason about, and it
    /// is deliberately not the normal path: a capability that only a
    /// hand-written `extra` can set is a feature a real user can never reach.
    pub fn forced_reasoning_replay(&self) -> Option<bool> {
        self.extra
            .get(REPLAYS_ASSISTANT_REASONING_KEY)
            .and_then(Value::as_bool)
    }
}

/// The one advanced override key the runtime reads out of `extra`.
///
/// It is named in the shared core rather than left to a caller because the
/// platform has to be able to set it too; see `apps/harmony` `ProviderDraft`.
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

    fn config(base_url: &str) -> ProviderConfig {
        ProviderConfig {
            id: "p1".into(),
            name: "test".into(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: base_url.into(),
            model: "some-model".into(),
            credential_ref: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn a_base_url_is_cached_under_one_spelling() {
        let a = config("https://API.Example.com/v1");
        let b = config("https://api.example.com/v1/");

        assert_eq!(a.normalized_base_url(), "https://api.example.com/v1");
        assert_eq!(a.capability_key(), b.capability_key());
    }

    #[test]
    fn a_different_path_is_a_different_endpoint() {
        // `/v1` and `/v2` on one host are different APIs, so folding them
        // together would serve a stale answer for the wrong one.
        assert_ne!(
            config("https://api.example.com/v1").capability_key(),
            config("https://api.example.com/v2").capability_key()
        );
    }

    #[test]
    fn nothing_about_a_vendor_or_a_model_name_decides_the_protocol() {
        // The same endpoint and the same capabilities, whatever the model is
        // called. This is the whole point: a name goes stale and a gateway
        // behind a name is invisible from the name.
        for model in ["deepseek-reasoner", "r1", "thinking-v2", "gpt-4o-mini"] {
            let mut candidate = config("https://api.deepseek.com/v1");
            candidate.model = model.into();

            assert_eq!(
                candidate.preferred_protocols()[0],
                Protocol::OpenAiChatCompletions
            );
            assert_eq!(candidate.forced_reasoning_replay(), None);
        }
    }

    #[test]
    fn a_kind_only_reorders_never_removes_a_protocol() {
        let mut anthropic = config("https://api.example.com/v1");
        anthropic.kind = ProviderKind::Anthropic;

        let ordered = anthropic.preferred_protocols();

        assert_eq!(ordered[0], Protocol::AnthropicMessages);
        assert_eq!(ordered.len(), Protocol::PRIORITY.len());
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
