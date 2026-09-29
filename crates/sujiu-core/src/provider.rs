use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
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
