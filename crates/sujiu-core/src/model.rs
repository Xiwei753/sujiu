//! Provider-neutral model conversation types.
//!
//! These live in the core crate because the persisted transcript is the
//! canonical model history: a session must be able to describe assistant tool
//! calls, tool results, reasoning and provider continuation state without
//! depending on the transport. Adapters in `sujiu-ai` translate these into a
//! provider wire format; nothing here knows about a provider.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{ChatRole, PromptPlan, Protocol};

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
    /// One assistant message: what the user can read, whatever the provider
    /// sent alongside it, and whatever it asked for.
    ///
    /// All three live together because they are properties of the same
    /// message, and splitting them is what let a plain answer lose its
    /// reasoning: a step with no tool call used to become a bare `Text`, and
    /// the sidecar had nowhere to go. An adapter decides the wire form, so a
    /// protocol that has no field for the sidecar simply omits it.
    Assistant {
        /// Visible text. Absent when the assistant only asked for tools.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Which participant this answer spoke for.
        ///
        /// A conversation can hold several characters, so "assistant" is not
        /// an identity: two answers from two participants stored as two
        /// anonymous assistant messages cannot be told apart afterwards. The id
        /// therefore travels with the step instead of being guessed from the
        /// turn, and a stored step is reloaded with the speaker it had.
        ///
        /// `None` means nobody was picked, which is a real answer rather than
        /// a missing field: choosing between participants is a scheduling
        /// policy, and inventing one here would put a guess in the transcript.
        ///
        /// Whether a protocol can carry it is the adapter's call. Chat
        /// Completions has a `name` field, Responses does not, and this is not
        /// a field Sujiu invents for a protocol that has none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speaker: Option<String>,
        /// The reasoning the model produced alongside this message, with the
        /// provider that produced it.
        ///
        /// Several thinking-mode transports reject a request whose previous
        /// assistant message arrives without the reasoning that produced it,
        /// so the reasoning has to travel back. It belongs to the assistant
        /// message rather than to a tool call, and it becomes a wire field
        /// only by an adapter that has been told the transport requires it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<ReasoningSidecar>,
        /// Empty when this is a final answer rather than a request for tools.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
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

    /// A final assistant answer.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant {
            content: Some(content.into()),
            speaker: None,
            reasoning: None,
            calls: Vec::new(),
        }
    }

    /// The same message, attributed to a participant.
    ///
    /// `None` leaves the message unattributed rather than clearing an
    /// attribution it already had, so this can be applied to a message that
    /// arrived named.
    pub fn with_speaker(mut self, speaker: Option<String>) -> Self {
        if let Some(speaker) = speaker {
            if let Self::Assistant { speaker: slot, .. } = &mut self {
                *slot = Some(speaker);
            }
        }
        self
    }

    /// Which participant this answer spoke for, if anyone.
    pub fn speaker(&self) -> Option<&str> {
        match self {
            Self::Assistant { speaker, .. } => speaker.as_deref(),
            _ => None,
        }
    }

    /// Whether this message asks for tools.
    pub fn has_tool_calls(&self) -> bool {
        matches!(self, Self::Assistant { calls, .. } if !calls.is_empty())
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

/// Which endpoint produced something, in enough detail to tell two apart.
///
/// The protocol is what matters most, and it is negotiated rather than
/// configured: the same endpoint answers over Responses on one turn and over
/// Chat Completions on the next, and provider state from one of those is not
/// state the other can accept. Model and endpoint are here too, because two
/// OpenAI-compatible gateways can serve the same model name while being
/// entirely different services, and replaying one endpoint's state to the other
/// is exactly the kind of mistake that is only discovered in production.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderIdentity {
    /// The wire protocol this state belongs to.
    #[serde(default)]
    pub protocol: Protocol,
    /// The configured endpoint id.
    #[serde(default)]
    pub endpoint_id: String,
    /// The endpoint the request is sent to.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
}

/// Reasoning a model produced, together with the provider it came from.
///
/// Ordinary visible assistant text is portable: the next provider can read it
/// and continue. Provider reasoning is not. It is wire metadata whose field
/// name and meaning belong to one protocol, so it carries its origin and is
/// only put back on the wire for that same provider. A bare string would let a
/// switch hand one provider's reasoning to another under a field name the
/// second one may not even accept.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningSidecar {
    /// The reasoning text as the provider sent it.
    pub content: String,
    /// The provider that produced this reasoning.
    #[serde(default)]
    pub identity: ProviderIdentity,
}

impl ReasoningSidecar {
    pub fn new(content: impl Into<String>, identity: ProviderIdentity) -> Self {
        Self {
            content: content.into(),
            identity,
        }
    }

    /// Whether this reasoning may be replayed to `identity`.
    ///
    /// The same provider config, endpoint and model is the only case treated
    /// as safe. A provider behind a different gateway is a different protocol
    /// as far as this text is concerned, even when both speak the same dialect
    /// of the same API.
    pub fn is_replayable_for(&self, identity: &ProviderIdentity) -> bool {
        !self.content.trim().is_empty() && self.identity == *identity
    }
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

/// What a provider's response says should happen to the continuation the next
/// round will send.
///
/// An earlier shape used `Option<ProviderContinuation>` and read "no state" as
/// "keep what you had", which is a guess: a transport that has just lost the
/// ability to resume would have been carried a handle that no longer works.
/// The three cases are named instead, so a provider has to say which one it
/// means.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationUpdate {
    /// The provider has no opinion: keep carrying whatever was in hand. A
    /// transport that simply replays the whole conversation always reports
    /// this, because it never had a handle to begin with.
    #[default]
    Unchanged,
    /// The handle is no longer usable. The next round continues from the
    /// normalized transcript alone.
    Clear,
    /// Continue from this state.
    Replace(ProviderContinuation),
}

impl ContinuationUpdate {
    /// The state this update produced, if it produced one. `Clear` and
    /// `Unchanged` both produce nothing; they differ in what the next round
    /// sends, not in what is stored.
    pub fn produced(&self) -> Option<ProviderContinuation> {
        match self {
            Self::Replace(state) => Some(state.clone()),
            Self::Unchanged | Self::Clear => None,
        }
    }

    /// Apply this update to the handle the next round will carry.
    pub fn apply(&self, carried: Option<ProviderContinuation>) -> Option<ProviderContinuation> {
        match self {
            Self::Unchanged => carried,
            Self::Clear => None,
            Self::Replace(state) => Some(state.clone()),
        }
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
    /// A handle of the given kind, stamped with the endpoint it came from.
    pub fn new(
        identity: ProviderIdentity,
        support: ContinuationSupport,
        response_id: Option<String>,
    ) -> Self {
        Self {
            identity,
            support,
            response_id,
            state: Map::new(),
        }
    }

    /// Record what this handle accounts for.
    ///
    /// A native handle does not replace the transcript, it lets a request stop
    /// re-sending the part the endpoint still holds. That only works if the
    /// handle says *which* part, so the lineage travels with it. Without it the
    /// safe answer is to send everything, which is what a missing one does.
    pub fn remembering_coverage(mut self, coverage: ContinuationCoverage) -> Self {
        if let Ok(value) = serde_json::to_value(&coverage) {
            self.state.insert(COVERAGE_KEY.to_owned(), value);
        }
        self
    }

    /// What this handle accounts for, when it says so in the current shape.
    ///
    /// The older bare `sentMessages` count is deliberately *not* read back here.
    /// It said how many messages were sent without saying what they were, so it
    /// cannot prove that the same messages are in front of the model now — and
    /// a count alone is exactly what caused input to be re-billed for a prefix
    /// the endpoint already held. A stored count without a digest degrades to
    /// "cannot prove", which sends everything.
    pub fn coverage(&self) -> Option<ContinuationCoverage> {
        let value = self.state.get(COVERAGE_KEY)?;
        let coverage: ContinuationCoverage = serde_json::from_value(value.clone()).ok()?;
        (coverage.digest.len() == DIGEST_CHARS).then_some(coverage)
    }

    /// How many leading request messages this handle provably still accounts
    /// for, given the messages this request would actually send.
    ///
    /// This is the check that makes reuse safe rather than hopeful. The handle
    /// names a digest of the messages it covered; if the request's own prefix
    /// still hashes to that digest, the endpoint is holding exactly those
    /// messages and skipping them is correct. If the prompt moved — a world-book
    /// entry matched, a persona or prompt profile was edited, a summary replaced
    /// history — the digest differs, nothing is skipped, and the whole
    /// transcript goes out again.
    pub fn proven_covers(&self, messages: &[ModelMessage]) -> Option<usize> {
        let coverage = self.coverage()?;
        let sent = coverage.sent.min(messages.len());
        if message_prefix_digest(&messages[..sent]) != coverage.digest {
            return None;
        }
        let covered = sent + coverage.assistant_messages;
        (covered <= messages.len()).then_some(covered)
    }

    /// Whether this state may be replayed to `identity` unchanged.
    pub fn is_reusable_for(&self, identity: &ProviderIdentity) -> bool {
        self.support.is_chainable() && &self.identity == identity
    }
}

/// The state key recording what a handle accounts for.
pub const COVERAGE_KEY: &str = "messageCoverage";

/// The state key recording how much of the request a handle covers.
///
/// Kept for reading documents written before coverage carried a digest. It is
/// deliberately not written any more: see [`ProviderContinuation::coverage`].
pub const SENT_MESSAGES_KEY: &str = "sentMessages";

/// What a native handle has already accounted for.
///
/// ## Why this is a provider-neutral message prefix, not a wire item count
///
/// A Responses handle does not cover "N items". One provider-neutral message
/// becomes several input items — an assistant turn that both spoke and called a
/// tool is a `message` item *and* a `function_call` item — and the response
/// itself contributes more items the next request does not send. A number
/// counted in wire items and a number counted in messages drift apart
/// permanently, and using the wrong one either re-sends a prefix the endpoint
/// holds (billed twice) or skips a prefix it never received (silently losing
/// context).
///
/// So the lineage is counted in the provider-neutral messages that are actually
/// the unit of the transcript, and the wire item count is kept beside it for
/// auditing only.
///
/// ## What the three fields mean
///
/// - `sent`: leading request messages the endpoint holds, counting everything
///   sent so far in this chain — including a prefix an earlier handle covered.
/// - `digest`: a digest of exactly those messages, so reuse can be *proved*
///   against the request being built rather than assumed from a count.
/// - `assistant_messages`: assistant messages this response itself added to the
///   endpoint's copy. They are not in `sent` because they did not exist yet when
///   the request was built, and they are not re-sent, so they have to be counted
///   separately. A tool result is deliberately not counted: the endpoint has
///   never seen it, which is why the next round sends exactly the tool results.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationCoverage {
    /// Leading request messages the endpoint already holds.
    pub sent: usize,
    /// Digest of exactly those messages.
    #[serde(default)]
    pub digest: String,
    /// Assistant messages the response itself added.
    #[serde(default)]
    pub assistant_messages: usize,
    /// How many wire items the covered messages became. Audit only.
    #[serde(default)]
    pub wire_items: usize,
}

const DIGEST_CHARS: usize = 16;

/// Roughly how much text a request carries.
///
/// An estimate on purpose. The kernel cannot know the endpoint's tokenizer, so
/// this counts characters rather than claiming tokens it did not count; what it
/// is for is deciding whether a request has stopped being reasonable, which a
/// character count answers well enough. The provider's own accounting, recorded
/// as [`TokenUsage`], remains the only real number.
pub fn input_size_chars(messages: &[ModelMessage]) -> usize {
    messages
        .iter()
        .map(|message| match message {
            ModelMessage::Text { content, .. } => content.chars().count(),
            ModelMessage::Assistant {
                content,
                reasoning,
                calls,
                ..
            } => {
                content
                    .as_deref()
                    .map(str::chars)
                    .map(Iterator::count)
                    .unwrap_or(0)
                    + reasoning
                        .as_ref()
                        .map(|sidecar| sidecar.content.chars().count())
                        .unwrap_or(0)
                    + calls
                        .iter()
                        .map(|call| call.arguments.to_string().len())
                        .sum::<usize>()
            }
            ModelMessage::ToolResult {
                content,
                structured_content,
                ..
            } => {
                content.chars().count()
                    + structured_content
                        .as_ref()
                        .map(|value| value.to_string().len())
                        .unwrap_or(0)
            }
        })
        .sum()
}

/// A digest of a prefix of the request messages.
///
/// This is a change detector, not a security primitive: it answers "are these the
/// same messages" for the runtime's own transcript, and FNV-1a is enough for that
/// while keeping the stored handle small and dependency-free. A collision would
/// mean a changed prefix reused a handle, which costs a re-sent prompt, not a
/// wrong answer, so a fast non-cryptographic hash is the right trade.
pub fn message_prefix_digest(messages: &[ModelMessage]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut absorb = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };

    for message in messages {
        absorb(
            serde_json::to_string(message)
                .unwrap_or_default()
                .as_bytes(),
        );
        // A separator so a message boundary cannot be shifted between messages.
        absorb(&[0x1e]);
    }

    format!("{hash:016x}")
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
    /// The reasoning this provider produced, already stamped with the provider
    /// that produced it. The provider fills it in rather than the caller,
    /// because provenance is the one thing the caller cannot know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningSidecar>,
    /// What a provider needs to continue this exact response, if anything. It
    /// travels with the transcript instead of being dropped, so a later turn
    /// can replay it to the same provider and model.
    #[serde(default)]
    pub continuation: ContinuationUpdate,
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

pub fn messages_from_prompt_plan(plan: &PromptPlan) -> Vec<ModelMessage> {
    plan.model_messages()
}
