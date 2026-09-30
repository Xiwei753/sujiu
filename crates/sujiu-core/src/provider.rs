use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

/// What an endpoint requires from the runtime, beyond being able to answer.
///
/// This is answered here, in the provider layer, and not in the settings form.
/// The form's job is to describe which endpoint and model to talk to; whether
/// that endpoint rejects a thinking-mode tool call that arrives without its
/// `reasoning_content` is a fact about the endpoint. A platform toggle for it
/// would be a protocol detail leaking into a form, and a capability stored
/// only in a hidden `extra` would be a feature a real user can never turn on,
/// because no form produces that key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// The endpoint rejects a request whose previous assistant tool call
    /// arrives without the reasoning that produced it, so every previous
    /// assistant `reasoning_content` has to be sent back.
    ///
    /// DeepSeek's thinking mode is the documented case, and it is a 400 rather
    /// than a silently wrong answer, which is why it is stated instead of
    /// discovered at runtime.
    pub replays_assistant_reasoning: bool,
}

impl ProviderConfig {
    /// What this configuration needs from the runtime.
    ///
    /// A profile for the endpoint decides, so a user who only supplies a base
    /// URL and a model gets the right behaviour. An explicit override exists
    /// for a gateway in front of a known endpoint, where the profile cannot
    /// see through to it.
    pub fn capabilities(&self) -> ProviderCapabilities {
        let known = ProviderCapabilities {
            replays_assistant_reasoning: requires_reasoning_replay(&self.base_url, &self.model),
        };

        match self
            .extra
            .get(REPLAYS_ASSISTANT_REASONING_KEY)
            .and_then(Value::as_bool)
        {
            Some(override_) => ProviderCapabilities {
                replays_assistant_reasoning: override_,
            },
            None => known,
        }
    }
}

/// The one override key the runtime reads out of `extra`.
///
/// It is named in the shared core rather than left to a caller because the
/// platform has to be able to set it too; see `apps/harmony` `ProviderDraft`.
pub const REPLAYS_ASSISTANT_REASONING_KEY: &str = "replaysAssistantReasoning";

/// Whether this known endpoint replays assistant reasoning in thinking mode.
///
/// Matching is on the host, not the whole URL, because a deployment may append
/// a version path. A model name is checked as well so a reasoning-capable
/// deployment is not assumed to need this for a model that does not think.
fn requires_reasoning_replay(base_url: &str, model: &str) -> bool {
    if !host_of(base_url).is_some_and(|host| {
        host.eq_ignore_ascii_case("api.deepseek.com")
            || host.eq_ignore_ascii_case("api.deepseek.com.cn")
    }) {
        return false;
    }

    let model = model.to_ascii_lowercase();

    model.contains("reasoner") || model.contains("thinking") || model.contains("r1")
}

/// The host of a base URL, without depending on a URL parser for the common
/// case. Anything that does not look like `scheme://host` yields `None`, which
/// simply means no profile applies and nothing is assumed.
fn host_of(base_url: &str) -> Option<&str> {
    let (_, rest) = base_url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map(|(_, host)| host).unwrap_or(host);

    (!host.is_empty()).then_some(host)
}
