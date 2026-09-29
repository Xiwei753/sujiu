//! Provider-neutral model conversation types.
//!
//! These live in the core crate because the persisted transcript is the
//! canonical model history: a session must be able to describe assistant tool
//! calls, tool results, reasoning and provider continuation state without
//! depending on the transport. Adapters in `sujiu-ai` translate these into a
//! provider wire format; nothing here knows about a provider.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{ChatRole, PromptPlan, ProviderKind};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    System,
    Developer,
    User,
    Assistant,
}

impl From<ChatRole> for ModelRole {
    fn from(value: ChatRole) -> Self {
        match value {
            ChatRole::System => Self::System,
            ChatRole::Developer => Self::Developer,
            ChatRole::User => Self::User,
            ChatRole::Assistant => Self::Assistant,
        }
    }
}

impl From<ModelRole> for ChatRole {
    fn from(value: ModelRole) -> Self {
        match value {
            ModelRole::System => Self::System,
            ModelRole::Developer => Self::Developer,
            ModelRole::User => Self::User,
            ModelRole::Assistant => Self::Assistant,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelMessage {
    Text {
        role: ModelRole,
        content: String,
    },
    AssistantToolCalls {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        calls: Vec<ToolCall>,
    },
    ToolResult {
        call_id: String,
        name: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        structured_content: Option<Value>,
        is_error: bool,
    },
}

impl ModelMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::Text {
            role: ModelRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::Text {
            role: ModelRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Text {
            role: ModelRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub read_only_hint: bool,
    #[serde(default)]
    pub destructive_hint: bool,
    #[serde(default)]
    pub idempotent_hint: bool,
    #[serde(default)]
    pub open_world_hint: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDiscovery {
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub always_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default)]
    pub annotations: ToolAnnotations,

    /// Local-only discovery metadata. Provider adapters must not treat this as
    /// model-visible weighting.
    #[serde(default)]
    pub discovery: ToolDiscovery,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text {
        text: String,
    },
    Resource {
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Image {
        data: String,
        mime_type: String,
    },
    Audio {
        data: String,
        mime_type: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    #[serde(default)]
    pub content: Vec<ToolContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(default)]
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            structured_content: None,
            is_error: false,
        }
    }

    pub fn structured(value: Value) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: value.to_string(),
            }],
            structured_content: Some(value),
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            content: vec![ToolContent::Text {
                text: message.clone(),
            }],
            structured_content: Some(serde_json::json!({"error": message})),
            is_error: true,
        }
    }

    pub fn model_text(&self) -> String {
        let text = self
            .content
            .iter()
            .filter_map(|content| match content {
                ToolContent::Text { text } => Some(text.as_str()),
                ToolContent::Resource {
                    text: Some(text), ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !text.is_empty() {
            text
        } else if let Some(structured) = &self.structured_content {
            structured.to_string()
        } else {
            String::new()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub name: String,
    #[serde(flatten)]
    pub output: ToolOutput,
}

/// How a tool call ended, recorded so an interrupted turn can be resumed
/// without leaving a call that the provider will reject.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallState {
    #[default]
    Completed,
    /// The tool ran and reported a failure.
    Failed,
    /// The turn stopped while this call was still open.
    Interrupted,
    /// The user cancelled the turn while this call was still open.
    Cancelled,
}

impl ToolCallState {
    /// Whether the provider should see this as an error result.
    ///
    /// An interrupted or cancelled call is reported as a failure because it
    /// did not produce its answer, and because a provider rejects a tool call
    /// that is not followed by any result at all.
    pub fn is_error(self) -> bool {
        !matches!(self, Self::Completed)
    }

    /// The model-visible text for a call that never produced a result.
    pub fn interrupted_text(name: &str, state: Self) -> String {
        match state {
            Self::Interrupted => {
                format!("{name} was interrupted before it finished. No result is available.")
            }
            Self::Cancelled => {
                format!("{name} was cancelled before it finished. No result is available.")
            }
            // Reached only when a caller has no result text; a completed call
            // always carries the tool's own output.
            Self::Completed | Self::Failed => format!("{name} produced no result."),
        }
    }
}

/// Which provider produced something, in enough detail to tell two endpoints
/// apart.
///
/// Provider kind and model are not enough. Two OpenAI-compatible gateways can
/// serve the same model name while being entirely different providers, and
/// replaying one provider's state to the other is exactly the kind of mistake
/// that is only discovered in production.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderIdentity {
    pub kind: ProviderKind,
    /// The configured provider id.
    #[serde(default)]
    pub provider_id: String,
    /// The endpoint the request is sent to.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
}

/// Whether a wire format can resume from provider state at all.
///
/// A transport that replays the whole conversation every time has nothing to
/// resume from, even though it still reports a response id. Recording that id
/// as a continuation token invites the next adapter to send it somewhere it
/// means nothing, so the capability is stated instead of implied.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationSupport {
    /// The wire format has no continuation token. The normalized transcript is
    /// the only way to continue, and any recorded id is metadata, not state.
    #[default]
    Unsupported,
    /// `response_id` is a real continuation token for this protocol.
    ResponseId,
}

impl ContinuationSupport {
    pub fn is_chainable(self) -> bool {
        matches!(self, Self::ResponseId)
    }
}

/// Provider-specific continuation state for one assistant step.
///
/// This is metadata the provider may need to continue a conversation exactly,
/// such as a response item id or an encrypted reasoning block. It is only
/// meaningful to the provider that produced it, so reuse requires an exact
/// [`ProviderIdentity`] match *and* a transport that can actually resume from
/// it; otherwise the runtime falls back to the normalized transcript.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ProviderContinuation {
    /// Defaults to an empty identity, which never matches a real provider, so
    /// state written before identities were recorded degrades to "not
    /// reusable" instead of being replayed to the wrong endpoint.
    #[serde(default)]
    pub identity: ProviderIdentity,
    #[serde(default)]
    pub support: ContinuationSupport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    /// Opaque provider items, kept verbatim so nothing is lost.
    #[serde(default)]
    pub state: Map<String, Value>,
}

impl ProviderContinuation {
    /// Whether this state may be replayed to `identity` unchanged.
    pub fn is_reusable_for(&self, identity: &ProviderIdentity) -> bool {
        self.support.is_chainable() && &self.identity == identity
    }
}

/// Token accounting for one provider response.
///
/// Cached token counts are the only honest way to tell whether a request reused
/// its prompt cache prefix, so they are recorded rather than discarded.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Prompt tokens served from the provider's cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    /// Prompt tokens the provider had to write into its cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ProviderRequest {
    pub messages: Vec<ModelMessage>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    /// Continuation state carried over from a previous step, present only when
    /// the provider and model still match the one that produced it.
    ///
    /// An adapter uses it only when its wire format has a place for it. A
    /// transport without continuation state simply ignores it, because the
    /// normalized messages already carry everything the model needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ProviderContinuation>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AssistantTurn {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    /// What a provider needs to continue this exact response, if anything. It
    /// travels with the transcript instead of being dropped, so a later turn
    /// can replay it to the same provider and model.
    #[serde(default)]
    pub continuation: Option<ProviderContinuation>,
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

pub fn messages_from_prompt_plan(plan: &PromptPlan) -> Vec<ModelMessage> {
    plan.model_messages()
}
