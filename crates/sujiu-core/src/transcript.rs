//! The canonical conversation transcript.
//!
//! A session stores this, not a text projection of itself. The transcript is
//! the ordered record of what the model was actually given and produced:
//! user messages, assistant steps, tool calls and tool results, plus the
//! provider continuation state needed to resume exactly.
//!
//! The rules this type exists to enforce:
//!
//! - one turn contains many steps; the final answer is the last assistant
//!   step, not the only one
//! - a tool call and its result are one atomic pair, so a call can never be
//!   stored without a result and a result can never exist without its call
//! - history is append-only apart from explicit compaction
//! - provider continuation state is stored, and is only reused when the
//!   provider kind and model still match
//!
//! A UI projection may fold these steps into one bubble per turn. That is a
//! separate concern and lives in [`Transcript::ui_messages`].

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    model::{ModelMessage, ToolCall, ToolCallState, ToolContent, ToolOutput},
    ChatRole,
};

/// Why a turn stopped.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    /// The assistant produced a final answer.
    #[default]
    Completed,
    /// The turn stopped while a tool call was still open.
    Interrupted,
    /// The turn was cancelled by the user.
    Cancelled,
    /// The turn stopped because the tool loop hit its round limit.
    Failed,
}

impl TurnState {
    /// Whether the turn produced a final assistant answer.
    pub fn is_complete(self) -> bool {
        matches!(self, Self::Completed)
    }
}

/// Reads a step's continuation event, accepting the older shape.
///
/// State written before the event was a persistable thing stored only the
/// handle a round produced, so a stored `null` meant "this round said nothing"
/// and a stored object meant "this round produced this". Both are still
/// readable; anything genuinely new is read as an event.
fn de_continuation_event<'de, D>(
    deserializer: D,
) -> Result<crate::model::ContinuationUpdate, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let stored = Option::<Value>::deserialize(deserializer)?;

    let Some(stored) = stored else {
        return Ok(crate::model::ContinuationUpdate::Unchanged);
    };

    if let Ok(event) = serde_json::from_value::<crate::model::ContinuationUpdate>(stored.clone()) {
        return Ok(event);
    }

    // A document written before continuation events were named stored the bare
    // handle. Only a value that is recognisably a handle is read that way: every
    // field of `ProviderContinuation` is optional, so asking it to read an
    // unrecognised shape yields an empty handle rather than an error, and an
    // empty handle is a different answer to "what should happen to the handle"
    // than the one the provider gave.
    let reads_as_a_handle = stored
        .as_object()
        .map(|fields| {
            ["identity", "support", "response_id", "state"]
                .iter()
                .any(|field| fields.contains_key(*field))
        })
        .unwrap_or(false);

    if reads_as_a_handle {
        return serde_json::from_value::<crate::model::ProviderContinuation>(stored)
            .map(crate::model::ContinuationUpdate::Replace)
            .map_err(D::Error::custom);
    }

    Err(D::Error::custom(format!(
        "unrecognised continuation event: {stored}"
    )))
}

/// Read a reasoning sidecar, including the shape an earlier build wrote.
///
/// Reasoning used to be a bare string, because the transcript did not yet
/// remember which provider produced it. That string is still readable, and it
/// is read as a sidecar with an empty identity, which makes it
/// non-replayable: a real identity always has at least a model, so old data
/// keeps its text for the record and is never sent to a provider as if we knew
/// where it came from. Losing that is the safe direction to be wrong in.
///
/// The alternative — refusing to parse — would make a whole session
/// unreadable over a field that only ever cost us a replay.
fn de_reasoning_sidecar<'de, D>(
    deserializer: D,
) -> Result<Option<crate::model::ReasoningSidecar>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let stored = Option::<Value>::deserialize(deserializer)?;

    let Some(stored) = stored else {
        return Ok(None);
    };

    if let Ok(sidecar) = serde_json::from_value::<crate::model::ReasoningSidecar>(stored.clone()) {
        return Ok(Some(sidecar));
    }

    // A string is the pre-identity shape.
    if let Ok(content) = serde_json::from_value::<String>(stored.clone()) {
        return Ok(Some(crate::model::ReasoningSidecar::new(
            content,
            crate::model::ProviderIdentity::default(),
        )));
    }

    Err(D::Error::custom(format!(
        "unrecognised reasoning sidecar: {stored}"
    )))
}

/// One tool call and its result.
///
/// The result lives inside the call on purpose: that makes an unpaired call
/// unrepresentable rather than something validation has to catch later.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCallRecord {
    /// The provider's call id, stored exactly as issued so a later request
    /// can pair the result with the same call.
    pub id: String,
    pub name: String,
    /// The tool's own display title, passed through from its definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub arguments: Value,
    pub result: ToolResultRecord,
}

/// What a tool produced, or why it produced nothing.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolResultRecord {
    #[serde(default)]
    pub content: Vec<ToolContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(default)]
    pub state: ToolCallState,
}

impl ToolResultRecord {
    pub fn completed(output: ToolOutput) -> Self {
        Self {
            state: if output.is_error {
                ToolCallState::Failed
            } else {
                ToolCallState::Completed
            },
            content: output.content,
            structured_content: output.structured_content,
        }
    }

    /// A call that never ran, recorded so the provider still sees a result.
    pub fn unfinished(state: ToolCallState, name: &str) -> Self {
        debug_assert!(state.is_error(), "only an unrun call is unfinished");
        Self {
            content: vec![ToolContent::Text {
                text: ToolCallState::interrupted_text(name, state),
            }],
            structured_content: None,
            state,
        }
    }

    pub fn is_error(&self) -> bool {
        self.state.is_error()
    }

    /// The text the model sees for this result.
    pub fn model_text(&self) -> String {
        ToolOutput {
            content: self.content.clone(),
            structured_content: self.structured_content.clone(),
            is_error: self.is_error(),
        }
        .model_text()
    }
}

/// One assistant step: what the model said, and the tools it asked for.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AssistantStep {
    /// Visible assistant text. Empty for a step that only requested tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The participant this step spoke for.
    ///
    /// A conversation can hold several characters, and this is the only place
    /// that says which one produced which line. It is stored per step rather
    /// than per turn because a turn may answer as one character in one round
    /// and as another in the next, and a turn-level label could only be right
    /// for the first of them.
    ///
    /// A stored step keeps the speaker it had, so a group chat does not need a
    /// transcript migration the first time two participants both speak.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// Reasoning content, kept apart from visible text because it is not part
    /// of the conversation the user reads. It remembers which provider
    /// produced it, because that decides whether it may be replayed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_reasoning_sidecar"
    )]
    pub reasoning: Option<crate::model::ReasoningSidecar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,
    /// What this round said should happen to the continuation handle.
    ///
    /// The whole event is stored, not just the handle it produced. A provider
    /// that drops the handle has told us something: storing only the handle
    /// would turn that answer into silence, and the next turn would search
    /// backwards and find the dead handle this step retired.
    #[serde(default, deserialize_with = "de_continuation_event")]
    pub continuation: crate::model::ContinuationUpdate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::model::TokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

impl AssistantStep {
    pub fn text_only(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            text: (!text.is_empty()).then_some(text),
            ..Self::default()
        }
    }

    /// Whether this step asked for at least one tool.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    /// This step as provider-neutral model messages, in wire order.
    ///
    /// A step that asked for tools becomes an assistant message followed by one
    /// `ToolResult` per call, in call order. A call that never finished still
    /// gets a result, because a provider rejects a call with no result at all.
    ///
    /// A step that only answered also becomes an assistant message, and it
    /// carries its reasoning. The reasoning used to be attached only to the
    /// tool-calling shape, so a thinking model that reasoned and then answered
    /// without a tool lost that reasoning on the next turn — the sidecar had
    /// nowhere to go. What the wire form is, and whether the reasoning is sent
    /// at all, is the adapter's call.
    pub fn model_messages(&self) -> Vec<ModelMessage> {
        let mut messages = Vec::new();

        let content = self.text.clone().filter(|text| !text.trim().is_empty());

        // A step that only asked for tools still needs a message; the call is
        // the content in that case. A step with neither text nor calls is not
        // a message at all, and emitting an empty one would put a blank turn
        // on the wire.
        if content.is_some() || self.has_tool_calls() {
            messages.push(ModelMessage::Assistant {
                content,
                speaker: self.speaker.clone(),
                calls: self
                    .tool_calls
                    .iter()
                    .map(|call| ToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    })
                    .collect(),
                reasoning: self.reasoning.clone(),
            });

            messages.extend(self.tool_calls.iter().map(|call| ModelMessage::ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: call.result.model_text(),
                structured_content: call.result.structured_content.clone(),
                is_error: call.result.is_error(),
            }));
        }

        messages
    }
}

/// A user message and every step the assistant took to answer it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Turn {
    pub id: String,
    /// The user text that started this turn.
    pub user: String,
    #[serde(default)]
    pub steps: Vec<AssistantStep>,
    #[serde(default)]
    pub state: TurnState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<i64>,
}

impl Turn {
    pub fn new(id: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            user: user.into(),
            steps: Vec::new(),
            state: TurnState::Completed,
            created_at_ms: None,
        }
    }

    /// The last step that produced visible text.
    ///
    /// The final answer is only the last assistant step. An earlier step's text
    /// was still sent to the model and still belongs to the transcript.
    pub fn final_text(&self) -> Option<&str> {
        self.steps
            .iter()
            .rev()
            .find_map(|step| step.text.as_deref())
            .filter(|text| !text.trim().is_empty())
    }

    /// The participant that produced the answer the user reads.
    ///
    /// The answer is the last step with visible text, so the speaker is that
    /// step's speaker and not the first one. A step that named nobody returns
    /// `None`, which is not the same as "the first participant": reading a
    /// guess off the conversation would put a character in the transcript that
    /// the turn never claimed.
    pub fn final_speaker(&self) -> Option<&str> {
        let step = self.steps.iter().rev().find(|step| {
            step.text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
        })?;

        step.speaker.as_deref()
    }

    /// Every distinct participant that spoke in this turn, in the order they
    /// first spoke.
    ///
    /// A turn that is answered as two different characters is a real thing a
    /// conversation has to be able to show, and a caller that only reads
    /// `final_speaker` would hide the first of them.
    pub fn speakers(&self) -> Vec<&str> {
        let mut speakers: Vec<&str> = Vec::new();
        for step in &self.steps {
            if let Some(speaker) = step.speaker.as_deref() {
                if !speakers.contains(&speaker) {
                    speakers.push(speaker);
                }
            }
        }
        speakers
    }

    /// Every tool call in this turn, in call order.
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCallRecord> {
        self.steps.iter().flat_map(|step| step.tool_calls.iter())
    }

    /// What this turn cost, as the provider reported it.
    ///
    /// A turn is several provider requests, and the number that matters to the
    /// bill is the sum over them: a tool-using turn re-sends the conversation on
    /// every round. Reading only the last step reports the cheap round and hides
    /// the expensive ones.
    pub fn usage(&self) -> TurnUsage {
        let mut usage = TurnUsage {
            rounds: self.steps.len(),
            tool_rounds: 0,
            ..TurnUsage::default()
        };

        for step in &self.steps {
            if !step.tool_calls.is_empty() {
                usage.tool_rounds += 1;
            }
            if let Some(reported) = step.usage.as_ref() {
                usage.absorb(reported);
            }
        }

        usage
    }

    /// The last thing any step in this turn said about continuation.
    ///
    /// A step that said nothing is not an answer, so the search keeps going
    /// backwards. A step that said "clear" is an answer, and it is returned
    /// rather than skipped, because a retired handle must not be rediscovered.
    pub fn continuation_event(&self) -> Option<&crate::model::ContinuationUpdate> {
        self.steps
            .iter()
            .rev()
            .find_map(|step| match step.continuation {
                crate::model::ContinuationUpdate::Unchanged => None,
                _ => Some(&step.continuation),
            })
    }

    /// The assistant bubbles this turn folds into.
    ///
    /// A run of consecutive steps that agree on the speaker becomes one bubble
    /// carrying that speaker, the text of the last step in the run, and every
    /// tool call the run made. A step with neither text nor a call makes no
    /// bubble, so a turn that did nothing produces nothing — the same thing it
    /// did before speakers existed.
    fn bubbles(&self) -> Vec<UiBubble> {
        let mut bubbles: Vec<UiBubble> = Vec::new();

        for step in &self.steps {
            let same_speaker = bubbles
                .last()
                .is_some_and(|bubble| bubble.speaker == step.speaker);

            let tool_calls = step.tool_calls.iter().map(|call| UiToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                title: call.title.clone().unwrap_or_else(|| call.name.clone()),
                state: call.result.state,
                is_error: call.result.is_error(),
                result_text: call.result.model_text(),
            });

            if same_speaker {
                let bubble = bubbles.last_mut().expect("a bubble was just found");
                if let Some(text) = step.text.as_deref().filter(|t| !t.trim().is_empty()) {
                    bubble.text = text.to_owned();
                }
                bubble.tool_calls.extend(tool_calls);
                continue;
            }

            let text = step
                .text
                .clone()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_default();
            let tool_calls: Vec<UiToolCall> = tool_calls.collect();

            if text.is_empty() && tool_calls.is_empty() {
                // A step that said nothing and called nothing. It is kept in
                // the transcript, but it has no shape in a bubble, and giving
                // it one would put an empty assistant message on the screen.
                // It also does not break the current run: a step that said
                // nothing did not take the floor away from anybody.
                continue;
            }

            bubbles.push(UiBubble {
                index: bubbles.len(),
                text,
                speaker: step.speaker.clone(),
                tool_calls,
            });
        }

        bubbles
    }

    /// This turn as provider-neutral model messages, in wire order.
    ///
    /// The user message comes first and every step follows in the order it
    /// happened, so the turn replays exactly as the model saw it.
    pub fn model_messages(&self) -> Vec<ModelMessage> {
        let mut messages = Vec::new();

        if !self.user.trim().is_empty() {
            messages.push(ModelMessage::user(self.user.clone()));
        }

        messages.extend(self.steps.iter().flat_map(AssistantStep::model_messages));

        messages
    }
}

/// One message as a UI shows it.
///
/// This is a projection, not the history. A turn with three tool calls becomes
/// one user message and one assistant message here, while the transcript keeps
/// all four steps.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct UiMessage {
    pub id: String,
    pub role: ChatRole,
    pub text: String,
    /// The participant this message speaks for, when one is known.
    ///
    /// Folding a turn into bubbles must not fold the identity away, so the
    /// speaker is carried on the projection too. A user message has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<UiToolCall>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct UiToolCall {
    pub id: String,
    pub name: String,
    pub title: String,
    pub state: ToolCallState,
    pub is_error: bool,
    pub result_text: String,
}

/// Turns that were replaced by a summary but are still retrievable.
///
/// Compaction moves history out of the prompt, it does not throw it away. The
/// exact text stays here so an older detail can still be found on request, which
/// is the same rule the long-conversation split in `docs/TOOLS.md` relies on.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CompactedTurns {
    #[serde(default)]
    pub turns: Vec<Turn>,
    /// The summary that stands in for them in the prompt.
    #[serde(default)]
    pub summary: String,
}

/// What a caller must fold into a new summary when it compacts a transcript.
///
/// A summary produced from only the turns being archived *now* would drop
/// everything older out of the model's view, so the input always carries the
/// previous summary next to them. The already-archived raw turns are
/// deliberately absent: they are what `previous_summary` was written from, and
/// including them again would make the summarizer request — and its bill —
/// grow with the entire conversation on every compaction, until the summarizer
/// is the thing that runs out of context.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompactionInput {
    /// The summary that already stands in for the archived turns, empty before
    /// the first compaction.
    pub previous_summary: String,
    /// The turns this compaction is moving, and the only raw turns a new summary
    /// has to read.
    pub turns: Vec<Turn>,
}

impl CompactionInput {
    /// A plain-text rendering for a summarizer prompt, with the previous summary
    /// first so the model extends it instead of starting over.
    pub fn to_prompt_text(&self) -> String {
        let mut text = String::new();

        if !self.previous_summary.trim().is_empty() {
            text.push_str("Summary of the conversation so far:\n");
            text.push_str(self.previous_summary.trim());
            text.push_str("\n\n");
        }

        text.push_str("Turns to fold into that summary:\n");
        for turn in &self.turns {
            text.push_str("\nUSER: ");
            text.push_str(turn.user.as_str());
            for step in turn.steps.iter().filter_map(|step| step.text.as_deref()) {
                if step.trim().is_empty() {
                    continue;
                }
                text.push_str("\nASSISTANT: ");
                text.push_str(step.trim());
            }
        }

        text
    }
}

/// What one turn cost, as the provider accounted for it.
///
/// Provider-neutral on purpose. The field names are the four numbers every
/// protocol reports differently — `prompt_tokens` versus `input_tokens`,
/// `cached_tokens` nested in a detail object, `cache_creation_input_tokens` —
/// and an adapter's job is to have already mapped them onto these. A log that
/// carried the wire names would need a reader per protocol; a log that carried a
/// converted currency figure would need a price list the kernel cannot verify.
///
/// No price is computed here, and none should be added. What a token costs is
/// an account's business: it changes, it is per endpoint, and it is sometimes
/// negotiated. Multiplying by a remembered price would make an auditable number
/// quietly wrong.
///
/// A `None` field means the endpoint did not report it, which is not the same as
/// zero — a provider with no cache billing reports no cache-write number, and
/// reading that as "nothing was written" would be an assumption. Totals are sums
/// of what was reported, so a turn where only some rounds reported usage has a
/// partial total; [`TurnUsage::is_complete`] is what distinguishes the two.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    /// Provider requests this turn made, including the final answer.
    pub rounds: usize,
    /// How many of those rounds asked for tools.
    ///
    /// Separate from `rounds` because a round that calls a tool re-sends the
    /// whole conversation: the round count says how many times the prompt was
    /// sent, and this says how many of those were tool rounds.
    pub tool_rounds: usize,
    /// Rounds that reported token usage at all.
    ///
    /// The comparison `rounds_reported < rounds` is what tells a reader that a
    /// total is a sum over part of the turn rather than over all of it.
    pub rounds_reported: usize,
    /// Prompt tokens billed across the turn, cached ones included.
    pub input_tokens: Option<u64>,
    /// Of those, the ones the provider served from its own prompt cache.
    pub cached_input_tokens: Option<u64>,
    /// Prompt tokens the provider had to write into its cache.
    pub cache_write_tokens: Option<u64>,
    /// Completion tokens billed across the turn.
    pub output_tokens: Option<u64>,
}

impl TurnUsage {
    /// Fold one round's reported usage into the total.
    fn absorb(&mut self, usage: &crate::model::TokenUsage) {
        self.rounds_reported += 1;

        let total = |mine: &mut Option<u64>, theirs: Option<u64>| {
            if let Some(value) = theirs {
                *mine = Some(mine.unwrap_or(0) + value);
            }
        };
        total(&mut self.input_tokens, usage.input_tokens);
        total(&mut self.cached_input_tokens, usage.cached_input_tokens);
        total(&mut self.cache_write_tokens, usage.cache_write_tokens);
        total(&mut self.output_tokens, usage.output_tokens);
    }

    /// Whether every round of this turn reported its usage.
    ///
    /// False means the totals cover only the rounds that did, so the numbers are
    /// a floor rather than the whole bill.
    pub fn is_complete(&self) -> bool {
        self.rounds_reported == self.rounds
    }
}

/// When a long session has to stop growing.
///
/// Compaction belongs to the runtime kernel, not to a platform. A frontend that
/// decided it was time to compact would produce different transcripts depending
/// on which screen the user was on, and a conversation that survives on one
/// platform and is destroyed on another is not a portable conversation.
///
/// The budget is expressed in characters rather than tokens because the kernel
/// cannot know the endpoint's tokenizer: it can count what it sends, and an
/// estimate is enough to decide when a request has stopped being reasonable. The
/// provider's own accounting is the real number, and that is what the turn usage
/// ledger records.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionPolicy {
    /// Roughly how much prompt text one turn may send before older turns are
    /// folded into a summary.
    pub max_input_chars: usize,
    /// How many recent turns stay verbatim no matter how long the session gets.
    ///
    /// Not a nicety: the most recent exchanges are what the model needs in full
    /// to answer, and a summary of the last thing the user said is a worse
    /// prompt than the thing itself.
    pub keep_recent_turns: usize,
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            max_input_chars: DEFAULT_MAX_INPUT_CHARS,
            keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
        }
    }
}

impl CompactionPolicy {
    /// A policy that never compacts on its own.
    ///
    /// Used when a caller drives compaction explicitly, so an explicit
    /// `compact_session` is not followed by the kernel deciding to compact again
    /// on the same history.
    pub fn manual() -> Self {
        Self {
            max_input_chars: usize::MAX,
            keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
        }
    }

    /// How many turns to keep verbatim, or `None` when the prompt is within
    /// budget.
    ///
    /// The decision is made from what this turn would actually send, not from a
    /// turn count, because those come apart quickly: a few turns of long tool
    /// results cost more than a dozen short ones, and only the first number is
    /// what a provider is billed for.
    ///
    /// The cut is always on a turn boundary, so it cannot separate a tool call
    /// from the result that answers it.
    pub fn keep_recent_for(&self, transcript: &Transcript, input_chars: usize) -> Option<usize> {
        if input_chars <= self.max_input_chars {
            return None;
        }

        // Oldest first, and always keep at least the recent window and one turn
        // to archive. A transcript with nothing to archive is not over budget in
        // any way this can fix, and claiming otherwise would compact the whole
        // conversation into a summary of itself.
        let archivable = transcript.len().saturating_sub(self.keep_recent_turns);
        (archivable > 0).then_some(self.keep_recent_turns)
    }
}

/// Roughly a quarter of a million characters of prompt text.
///
/// Deliberately generous: this is a safety bound against a session that would
/// otherwise never stop growing, not a recommendation. A provider that rejects
/// the request for being too long is the authority on its own limit; this only
/// stops the transcript from growing without one.
pub const DEFAULT_MAX_INPUT_CHARS: usize = 240_000;

/// Turns that stay verbatim when compaction triggers.
pub const DEFAULT_KEEP_RECENT_TURNS: usize = 8;

/// The ordered transcript of a session.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Transcript {
    #[serde(default)]
    pub turns: Vec<Turn>,
    /// History replaced by a summary, kept out of the prompt but not deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted: Option<CompactedTurns>,
}

impl Transcript {
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    pub fn len(&self) -> usize {
        self.turns.len()
    }

    /// Append a turn. History is only ever added at the tail.
    pub fn push(&mut self, turn: Turn) {
        self.turns.push(turn);
    }

    /// Replace the oldest turns with a summary, keeping the last `keep_recent`.
    ///
    /// Compaction is the one operation allowed to rewrite history, and it is
    /// still bound by the pairing rule. A tool call and its result live in the
    /// same turn, so cutting on a turn boundary cannot leave a call without the
    /// result that answers it.
    ///
    /// The compacted turns move to [`Transcript::compacted`] instead of being
    /// deleted, so an exact older detail stays retrievable. Their continuation
    /// state leaves the prompt with them: a provider must not be asked to resume
    /// from turns the model can no longer see.
    ///
    /// The summarizer is handed the summary that already stands for the archived
    /// history plus the turns being archived *now*, and nothing else. The
    /// already-archived raw turns stay in [`Transcript::compacted`] for the UI
    /// and for exact retrieval, but they do not go back into the summarizer
    /// request: `previous_summary` is already what they were folded into, so
    /// resending them would charge for the whole conversation again on every
    /// compaction, forever.
    ///
    /// Returns `false` when there is nothing old enough to compact, which keeps a
    /// caller from rewriting a history that did not need rewriting.
    /// What a caller has to summarize in order to compact this transcript.
    ///
    /// Exposing it matters. An upper layer that only ever sees the turns being
    /// archived *now* cannot know what the summary it is replacing already said,
    /// so it cannot extend it; and one that only ever sees `previous_summary`
    /// cannot fold the new turns in. The input carries exactly the two pieces
    /// that together make a cumulative summary: what is already known, and what
    /// is newly arriving.
    pub fn compaction_input(&self, keep_recent: usize) -> Option<CompactionInput> {
        let keep_from = self.turns.len().saturating_sub(keep_recent);
        if keep_from == 0 {
            return None;
        }

        Some(CompactionInput {
            previous_summary: self
                .compacted
                .as_ref()
                .map(|archived| archived.summary.clone())
                .unwrap_or_default(),
            turns: self.turns[..keep_from].to_vec(),
        })
    }

    pub fn compact(
        &mut self,
        keep_recent: usize,
        summarize: impl FnOnce(&[Turn], &str) -> String,
    ) -> bool {
        let keep_from = self.turns.len().saturating_sub(keep_recent);
        if keep_from == 0 {
            return false;
        }

        let moved: Vec<Turn> = self.turns.drain(..keep_from).collect();
        let previous_summary = self
            .compacted
            .as_ref()
            .map(|archived| archived.summary.clone())
            .unwrap_or_default();
        // Only the turns leaving the prompt, and only against the summary they
        // extend. The turns already in `compacted` are represented by that
        // summary, and handing them back would make every compaction re-bill the
        // entire history.
        let summary = summarize(&moved, &previous_summary);

        let archived = self.compacted.get_or_insert_with(CompactedTurns::default);
        archived.turns.extend(moved);
        archived.summary = summary;

        true
    }

    /// The text a keyword scan should see: everything the model was shown.
    ///
    /// The compacted summary counts, because the model is shown it. Leaving it
    /// out would make a world-book entry stop matching at the exact moment the
    /// conversation got long enough to compact, which is a change in what the
    /// model is told triggered by something that changed nothing about the
    /// world. Archived raw turns do not count: they are no longer in the
    /// prompt, and the summary is what stands in for them.
    pub fn scan_text(&self) -> String {
        let summary = self
            .compacted
            .as_ref()
            .map(|compacted| compacted.summary.trim())
            .filter(|summary| !summary.is_empty());

        self.turns
            .iter()
            .flat_map(|turn| {
                std::iter::once(turn.user.as_str()).chain(
                    turn.steps
                        .iter()
                        .filter_map(|step| step.text.as_deref())
                        .filter(|text| !text.trim().is_empty()),
                )
            })
            .chain(summary)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Continuation state that may be replayed to `identity` as is.
    ///
    /// Returns `None` after a provider, endpoint or model switch, and `None`
    /// for a transport that cannot resume from provider state at all. Either
    /// way the runtime falls back to the normalized transcript instead of
    /// sending state the provider never asked for.
    ///
    /// The search stops at the first event a provider ever gave, whatever that
    /// event was. A handle it replaced is superseded, and a handle it cleared
    /// is dead, so neither is a reason to keep looking for an older one.
    pub fn continuation_for(
        &self,
        identity: &crate::model::ProviderIdentity,
    ) -> Option<&crate::model::ProviderContinuation> {
        self.continuation_event()
            .and_then(|event| match event {
                crate::model::ContinuationUpdate::Replace(continuation) => Some(continuation),
                crate::model::ContinuationUpdate::Clear
                | crate::model::ContinuationUpdate::Unchanged => None,
            })
            .filter(|continuation| continuation.is_reusable_for(identity))
    }

    /// The most recent continuation event, whatever provider produced it.
    ///
    /// This is the state lookup that does not need an identity, for callers that
    /// have not negotiated yet and therefore cannot say which provider they are
    /// talking to. Asking for an identity before negotiation is what made an
    /// unknown protocol mean "Responses": the placeholder matched a real
    /// Responses handle and replayed it. Deciding reuse afterwards, against the
    /// identity the endpoint actually settled on, cannot do that.
    pub fn continuation_event(&self) -> Option<&crate::model::ContinuationUpdate> {
        self.turns.iter().rev().find_map(Turn::continuation_event)
    }

    /// Every turn id in the session, including compacted ones.
    ///
    /// An id has to stay unique across the whole session, not just across the
    /// turns that are still in the prompt, or a new turn could be handed an id
    /// that an archived turn already owns.
    pub fn turn_ids(&self) -> impl Iterator<Item = &str> {
        self.turns.iter().map(|turn| turn.id.as_str()).chain(
            self.compacted
                .iter()
                .flat_map(|archived| archived.turns.iter())
                .map(|turn| turn.id.as_str()),
        )
    }

    /// Every tool call id in the transcript, in order.
    pub fn tool_call_ids(&self) -> Vec<&str> {
        self.turns
            .iter()
            .flat_map(Turn::tool_calls)
            .map(|call| call.id.as_str())
            .collect()
    }

    /// The folded conversation a UI renders.
    ///
    /// Archived turns are included. Compaction moves turns out of the *prompt*
    /// and out of nothing else: a user who has to scroll back through a long
    /// conversation has to find it whole, and a kernel that compacts on its own
    /// would otherwise shorten the scrollback every time the budget ran out. The
    /// archived turns come first because they are older, so the order a platform
    /// renders is the order the conversation happened in.
    pub fn ui_messages(&self) -> Vec<UiMessage> {
        let mut messages = Vec::new();

        if let Some(archived) = self.compacted.as_ref() {
            for turn in &archived.turns {
                messages.extend(project_turn(turn));
            }
        }

        for turn in &self.turns {
            messages.extend(project_turn(turn));
        }

        messages
    }
}

/// One turn as UI messages: the user's message, then one bubble per speaker.
fn project_turn(turn: &Turn) -> Vec<UiMessage> {
    let mut messages = Vec::new();

    messages.push(UiMessage {
        id: format!("{}-user", turn.id),
        role: ChatRole::User,
        text: turn.user.clone(),
        speaker: None,
        tool_calls: Vec::new(),
    });

    // The tool steps are folded into the assistant bubble. A platform may hide
    // or collapse them; the transcript above still holds every one of them.
    //
    // A turn is one bubble per speaker, not one per turn. Folding a
    // two-character answer into a single anonymous bubble would lose the one
    // thing that tells the lines apart, and the folding rule stays the same when
    // there is only one speaker: consecutive steps that agree on the speaker are
    // one bubble, so a turn with nobody named still folds exactly as it did
    // before.
    let bubbles = turn.bubbles();

    for bubble in &bubbles {
        // A lone bubble keeps the id it has always had, because ids a platform
        // may already have stored are not renamed to fit a case that did not
        // exist before.
        let id = if bubbles.len() == 1 {
            format!("{}-assistant", turn.id)
        } else {
            format!("{}-assistant-{}", turn.id, bubble.index)
        };

        messages.push(UiMessage {
            id,
            role: ChatRole::Assistant,
            text: bubble.text.clone(),
            speaker: bubble.speaker.clone(),
            tool_calls: bubble.tool_calls.clone(),
        });
    }

    messages
}

/// One assistant bubble in the UI projection.
///
/// It is a run of consecutive steps that agree on who is speaking, with the
/// text of the last step in the run and every tool call the run made.
struct UiBubble {
    /// Where this bubble sits in its turn, so two bubbles get distinct ids.
    index: usize,
    text: String,
    speaker: Option<String>,
    tool_calls: Vec<UiToolCall>,
}

/// The transcript as provider-neutral model messages, in wire order.
///
/// Turns are concatenated in order and nothing is rewritten, which is what keeps
/// a request's prefix identical to the previous request's prefix.
pub fn model_messages_from_transcript(transcript: &Transcript) -> Vec<ModelMessage> {
    transcript
        .turns
        .iter()
        .flat_map(Turn::model_messages)
        .collect()
}

/// Check that every stored call can be paired with a result.
///
/// [`ToolCallRecord`] nests its result, so an unpaired call cannot be built;
/// what this catches is a transcript that arrived from an import, a migration
/// or a hand-edited file.
pub fn validate_pairing(transcript: &Transcript) -> Result<(), PairingError> {
    let mut seen = std::collections::BTreeSet::new();

    for turn in &transcript.turns {
        for call in turn.tool_calls() {
            if !seen.insert(call.id.as_str()) {
                return Err(PairingError::DuplicateCallId(call.id.clone()));
            }

            if call.result.content.is_empty() && call.result.structured_content.is_none() {
                return Err(PairingError::UnpairedCall(call.id.clone()));
            }
        }
    }

    Ok(())
}

/// Why a transcript cannot be turned into provider messages.
///
/// Every case here is a protocol violation a provider would reject, so they
/// are reported instead of being silently repaired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingError {
    /// A call id appears more than once, so a result cannot be matched.
    DuplicateCallId(String),
    /// A call was stored without any result, which a provider rejects.
    UnpairedCall(String),
}

impl std::fmt::Display for PairingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateCallId(id) => write!(formatter, "duplicate tool call id: {id}"),
            Self::UnpairedCall(id) => write!(formatter, "tool call without a result: {id}"),
        }
    }
}

impl std::error::Error for PairingError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{ContinuationSupport, ProviderContinuation, ProviderIdentity, ToolOutput},
        Protocol,
    };
    use serde_json::json;

    fn call(id: &str, name: &str) -> ToolCallRecord {
        ToolCallRecord {
            id: id.into(),
            name: name.into(),
            title: Some(format!("Tool {id}")),
            arguments: json!({}),
            result: ToolResultRecord::completed(ToolOutput::text("done")),
        }
    }

    fn tool_turn() -> Turn {
        let mut turn = Turn::new("turn-1", "what is in the lore?");
        turn.steps.push(AssistantStep {
            text: None,
            tool_calls: vec![call("call-1", "search_context")],
            ..AssistantStep::default()
        });
        turn.steps.push(AssistantStep {
            text: Some("A caller held the line open.".into()),
            ..AssistantStep::default()
        });
        turn
    }

    #[test]
    fn a_turn_keeps_every_step_the_model_saw() {
        let turn = tool_turn();

        assert_eq!(turn.steps.len(), 2);
        assert_eq!(turn.final_text(), Some("A caller held the line open."));
        assert_eq!(turn.tool_calls().count(), 1);
    }

    #[test]
    fn the_ui_projection_folds_tool_steps_without_losing_them() {
        let mut transcript = Transcript::default();
        transcript.push(tool_turn());

        let messages = transcript.ui_messages();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, ChatRole::User);
        assert_eq!(messages[1].tool_calls.len(), 1);
        // The folded assistant message is only the last step, which is what a
        // user reads.
        assert_eq!(messages[1].text, "A caller held the line open.");
        // The transcript still holds the call.
        assert_eq!(transcript.tool_call_ids(), vec!["call-1"]);
    }

    #[test]
    fn an_unfinished_call_is_stored_as_an_explicit_result() {
        let record = ToolResultRecord::unfinished(ToolCallState::Interrupted, "search_context");

        assert!(record.is_error());
        assert!(record.model_text().contains("interrupted"));
    }

    #[test]
    fn an_answer_without_a_tool_call_still_carries_its_reasoning() {
        let origin = identity(
            Protocol::OpenAiChatCompletions,
            "p1",
            "https://a.example/v1",
            "m",
        );
        let mut step = AssistantStep::text_only("The light stopped blinking.");
        step.reasoning = Some(crate::model::ReasoningSidecar::new(
            "counting the intervals",
            origin,
        ));

        let messages = step.model_messages();

        assert_eq!(messages.len(), 1, "a plain answer is one message");
        match &messages[0] {
            ModelMessage::Assistant {
                content,
                speaker,
                reasoning,
                calls,
            } => {
                assert_eq!(content.as_deref(), Some("The light stopped blinking."));
                // A step nobody attributed is a real answer, not a missing
                // field.
                assert_eq!(speaker.as_deref(), None);
                // The sidecar used to be dropped here, because only the
                // tool-calling shape had a place for it.
                assert!(reasoning.is_some());
                assert!(calls.is_empty());
            }
            other => panic!("expected an assistant message, got {other:?}"),
        }
    }

    #[test]
    fn reasoning_written_as_a_plain_string_is_read_but_never_replayed() {
        // What an earlier build wrote, before the sidecar remembered where the
        // reasoning came from.
        let stored = json!({
            "id": "turn-1",
            "user": "what do you hear?",
            "state": "completed",
            "steps": [
                { "text": "The sea.", "reasoning": "counting the intervals" }
            ]
        });

        let turn: Turn = serde_json::from_value(stored).expect("old reasoning is readable");

        let sidecar = turn.steps[0]
            .reasoning
            .as_ref()
            .expect("kept for the record");
        assert_eq!(sidecar.content, "counting the intervals");
        // An empty identity is not any real provider, so it can never be
        // replayed. Keeping the text is the point; guessing who wrote it is not.
        assert!(!sidecar.is_replayable_for(&identity(
            Protocol::OpenAiChatCompletions,
            "p1",
            "https://a.example/v1",
            "m"
        )));
    }

    #[test]
    fn a_step_with_neither_text_nor_calls_is_not_a_message() {
        assert!(AssistantStep::default().model_messages().is_empty());
        // A step that only thought is still not a message the model sent.
        let mut thinking = AssistantStep::default();
        thinking.reasoning = Some(crate::model::ReasoningSidecar::new(
            "nothing said",
            identity(
                Protocol::OpenAiChatCompletions,
                "p1",
                "https://a.example/v1",
                "m",
            ),
        ));

        assert!(thinking.model_messages().is_empty());
    }

    fn identity(
        protocol: Protocol,
        endpoint_id: &str,
        base_url: &str,
        model: &str,
    ) -> ProviderIdentity {
        ProviderIdentity {
            protocol,
            endpoint_id: endpoint_id.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
        }
    }

    use crate::model::ContinuationUpdate;

    #[test]
    fn continuation_state_is_only_reusable_for_the_same_provider_endpoint_and_model() {
        let mut turn = tool_turn();
        turn.steps[0].continuation = ContinuationUpdate::Replace(ProviderContinuation {
            identity: identity(
                Protocol::OpenAiChatCompletions,
                "provider-a",
                "https://a.example/v1",
                "model-a",
            ),
            support: ContinuationSupport::ResponseId,
            response_id: Some("resp-1".into()),
            state: Default::default(),
        });

        let mut transcript = Transcript::default();
        transcript.push(turn);

        assert!(transcript
            .continuation_for(&identity(
                Protocol::OpenAiChatCompletions,
                "provider-a",
                "https://a.example/v1",
                "model-a"
            ))
            .is_some());

        // Two OpenAI-compatible gateways serving the same model name are still
        // two different providers.
        assert!(transcript
            .continuation_for(&identity(
                Protocol::OpenAiChatCompletions,
                "provider-b",
                "https://b.example/v1",
                "model-a"
            ))
            .is_none());
        assert!(transcript
            .continuation_for(&identity(
                Protocol::OpenAiChatCompletions,
                "provider-a",
                "https://a.example/v1",
                "model-b"
            ))
            .is_none());
        assert!(transcript
            .continuation_for(&identity(
                Protocol::AnthropicMessages,
                "provider-a",
                "https://a.example/v1",
                "model-a"
            ))
            .is_none());
    }

    #[test]
    fn a_transport_without_native_continuation_never_replays_its_response_id() {
        let mut turn = tool_turn();
        turn.steps[0].continuation = ContinuationUpdate::Replace(ProviderContinuation {
            identity: identity(
                Protocol::OpenAiChatCompletions,
                "provider-a",
                "https://a.example/v1",
                "model-a",
            ),
            // Chat Completions reports an id but cannot resume from it.
            support: ContinuationSupport::Unsupported,
            response_id: Some("chatcmpl-1".into()),
            state: Default::default(),
        });

        let mut transcript = Transcript::default();
        transcript.push(turn);

        assert!(transcript
            .continuation_for(&identity(
                Protocol::OpenAiChatCompletions,
                "provider-a",
                "https://a.example/v1",
                "model-a"
            ))
            .is_none());
    }

    /// A provider that dropped its handle said so, and that answer has to
    /// survive the session boundary. Searching backwards past it would hand the
    /// next turn a handle the provider has already disowned.
    #[test]
    fn a_cleared_handle_is_not_resurrected_from_an_earlier_step() {
        let mut turn = tool_turn();
        turn.steps[0].continuation = ContinuationUpdate::Replace(ProviderContinuation {
            identity: identity(
                Protocol::OpenAiChatCompletions,
                "provider-a",
                "https://a.example/v1",
                "model-a",
            ),
            support: ContinuationSupport::ResponseId,
            response_id: Some("resp-1".into()),
            state: Default::default(),
        });
        // A later round of the same turn.
        turn.steps[1].continuation = ContinuationUpdate::Clear;

        let mut transcript = Transcript::default();
        transcript.push(turn);

        let identity = identity(
            Protocol::OpenAiChatCompletions,
            "provider-a",
            "https://a.example/v1",
            "model-a",
        );
        assert!(
            transcript.continuation_for(&identity).is_none(),
            "a handle the provider dropped must not come back"
        );
    }

    /// The clear is only real if it survives being written to disk and read
    /// back, which is the boundary where the first version of this lost it.
    #[test]
    fn a_cleared_handle_stays_cleared_across_a_snapshot() {
        let mut turn = tool_turn();
        turn.steps[0].continuation = ContinuationUpdate::Replace(ProviderContinuation {
            identity: identity(
                Protocol::OpenAiChatCompletions,
                "provider-a",
                "https://a.example/v1",
                "model-a",
            ),
            support: ContinuationSupport::ResponseId,
            response_id: Some("resp-1".into()),
            state: Default::default(),
        });
        turn.steps[1].continuation = ContinuationUpdate::Clear;

        let mut transcript = Transcript::default();
        transcript.push(turn);

        let restored: Transcript = serde_json::from_value(
            serde_json::to_value(&transcript).expect("transcript serializes"),
        )
        .expect("transcript parses back");
        let identity = identity(
            Protocol::OpenAiChatCompletions,
            "provider-a",
            "https://a.example/v1",
            "model-a",
        );

        assert!(matches!(
            restored.turns[0].continuation_event(),
            Some(ContinuationUpdate::Clear)
        ));
        assert!(restored.continuation_for(&identity).is_none());
    }

    #[test]
    fn continuation_written_before_identities_existed_is_never_replayed() {
        // The shape recorded before identities and support were tracked. It
        // still parses, and it must degrade to "not reusable" rather than being
        // sent to whichever provider happens to be configured.
        let stored: ProviderContinuation = serde_json::from_value(serde_json::json!({
            "provider_kind": "open_ai_compatible",
            "model": "model-a",
            "response_id": "resp-1"
        }))
        .expect("older continuation state still parses");

        let identity = identity(
            Protocol::OpenAiChatCompletions,
            "provider-a",
            "https://a.example/v1",
            "model-a",
        );
        assert!(!stored.is_reusable_for(&identity));
    }

    #[test]
    fn compaction_keeps_every_call_paired_and_stays_retrievable() {
        let mut transcript = Transcript::default();
        transcript.push(tool_turn());
        let mut second = Turn::new("turn-2", "and who is on the line?");
        second.steps.push(AssistantStep::text_only("Nobody yet."));
        transcript.push(second);
        let mut third = Turn::new("turn-3", "still there?");
        third
            .steps
            .push(AssistantStep::text_only("Still listening."));
        transcript.push(third);

        let compacted = transcript.compact(2, |turns, _| format!("{} earlier turns", turns.len()));

        assert!(compacted, "there was enough history to compact");
        assert_eq!(transcript.len(), 2);
        // The call that lived in the compacted turn is gone from the prompt, but
        // its result went with it: cutting on a turn boundary cannot split a pair.
        assert!(transcript.tool_call_ids().is_empty());
        assert!(validate_pairing(&transcript).is_ok());
        // Nothing was deleted, so the exact older turn is still there to retrieve.
        let archived = transcript.compacted.as_ref().expect("archived turns");
        assert_eq!(archived.turns.len(), 1);
        assert_eq!(
            archived.turns[0]
                .tool_calls()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["call-1"]
        );
        assert_eq!(archived.summary, "1 earlier turns");
    }

    #[test]
    fn a_second_compaction_is_given_the_summary_it_extends_and_only_the_new_turns() {
        let mut transcript = Transcript::default();
        for index in 1..=4 {
            let mut turn = Turn::new(&format!("turn-{index}"), &format!("question {index}"));
            turn.steps
                .push(AssistantStep::text_only(&format!("answer {index}")));
            transcript.push(turn);
        }

        assert!(transcript.compact(3, |turns, _| { format!("first: {} turns", turns.len()) }));
        assert!(transcript.compact(2, |turns, previous| {
            format!("{previous} then {} more", turns.len())
        }));

        let archived = transcript.compacted.as_ref().expect("archived turns");
        // Both compactions archived turns, and neither was deleted.
        assert_eq!(archived.turns.len(), 2);
        // The second summarizer was handed one new turn and the summary it
        // extends. Had it also been handed turn-1 again — which the summary
        // already stands for — every compaction would re-bill the whole
        // conversation and grow until the summarizer ran out of context.
        assert_eq!(archived.summary, "first: 1 turns then 1 more");
    }

    #[test]
    fn turn_ids_cover_compacted_turns_too() {
        let mut transcript = Transcript::default();
        for index in 1..=3 {
            let mut turn = Turn::new(&format!("turn-{index}"), "hello");
            turn.steps.push(AssistantStep::text_only("hi"));
            transcript.push(turn);
        }

        assert!(transcript.compact(1, |_, _| "summary".to_string()));
        assert_eq!(transcript.len(), 1);
        // turn-1 and turn-2 left the prompt, but they still own their ids.
        assert_eq!(
            transcript.turn_ids().collect::<Vec<_>>(),
            vec!["turn-3", "turn-1", "turn-2"]
        );
    }

    #[test]
    fn compaction_refuses_to_rewrite_a_history_that_does_not_need_it() {
        let mut transcript = Transcript::default();
        transcript.push(tool_turn());

        assert!(
            !transcript.compact(4, |_, _| "summary".to_string()),
            "nothing old enough to compact means the history stays byte for byte"
        );
        assert_eq!(transcript.len(), 1);
        assert_eq!(transcript.tool_call_ids(), vec!["call-1"]);
    }

    #[test]
    fn a_turn_that_never_answered_projects_no_assistant_bubble() {
        let mut turn = Turn::new("turn-1", "hello");
        turn.state = TurnState::Cancelled;

        let mut transcript = Transcript::default();
        transcript.push(turn);

        let messages = transcript.ui_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, ChatRole::User);
    }

    /// A turn where the model answered as two different characters.
    fn table_turn() -> Turn {
        let mut turn = Turn::new("turn-1", "where is everyone?");
        turn.steps.push(AssistantStep {
            text: Some("The harbour master takes the north pier.".into()),
            speaker: Some("character-lin".into()),
            ..AssistantStep::default()
        });
        turn.steps.push(AssistantStep {
            text: None,
            tool_calls: vec![call("call-1", "search_context")],
            speaker: Some("character-wen".into()),
            ..AssistantStep::default()
        });
        turn.steps.push(AssistantStep {
            text: Some("The south pier is mine.".into()),
            speaker: Some("character-wen".into()),
            ..AssistantStep::default()
        });
        turn
    }

    #[test]
    fn a_speaker_is_kept_through_storage_and_read_back() {
        let stored = serde_json::to_value(table_turn()).expect("a turn is storable");

        let turn: Turn = serde_json::from_value(stored).expect("a speaker survives the round trip");

        assert_eq!(turn.steps[0].speaker.as_deref(), Some("character-lin"));
        assert_eq!(turn.final_speaker(), Some("character-wen"));
        assert_eq!(turn.speakers(), vec!["character-lin", "character-wen"]);
    }

    #[test]
    fn the_ui_projection_keeps_one_bubble_per_voice() {
        let mut transcript = Transcript::default();
        transcript.push(table_turn());

        let messages = transcript.ui_messages();

        assert_eq!(messages.len(), 3, "one user message and two voices");
        let bubbles: Vec<_> = messages
            .iter()
            .filter(|message| message.role == ChatRole::Assistant)
            .collect();
        assert_eq!(bubbles.len(), 2, "two characters are two bubbles");
        assert_eq!(bubbles[0].speaker.as_deref(), Some("character-lin"));
        assert_eq!(bubbles[0].text, "The harbour master takes the north pier.");
        assert_eq!(bubbles[1].speaker.as_deref(), Some("character-wen"));
        // The tool call belongs to the second voice, and folding it into that
        // bubble does not drop it from the transcript.
        assert_eq!(bubbles[1].tool_calls.len(), 1);
        assert_eq!(transcript.tool_call_ids(), vec!["call-1"]);
        // Distinct ids, because two bubbles cannot both be the one message a
        // platform may already have stored.
        assert_ne!(bubbles[0].id, bubbles[1].id);
    }

    #[test]
    fn a_turn_with_one_voice_folds_exactly_as_it_did_before() {
        let mut transcript = Transcript::default();
        transcript.push(tool_turn());

        let messages = transcript.ui_messages();

        let assistant: Vec<_> = messages
            .iter()
            .filter(|message| message.role == ChatRole::Assistant)
            .collect();
        assert_eq!(assistant.len(), 1);
        // The id a platform may already have stored is not renamed just because
        // a speaker field exists.
        assert_eq!(assistant[0].id, "turn-1-assistant");
        assert_eq!(assistant[0].speaker, None);
        assert_eq!(assistant[0].tool_calls.len(), 1);
    }

    #[test]
    fn a_speaker_travels_into_the_model_history() {
        let step = AssistantStep {
            text: Some("The tide is out.".into()),
            speaker: Some("character-lin".into()),
            ..AssistantStep::default()
        };

        let messages = step.model_messages();

        assert_eq!(messages[0].speaker(), Some("character-lin"));
        match &messages[0] {
            ModelMessage::Assistant { speaker, .. } => {
                assert_eq!(speaker.as_deref(), Some("character-lin"));
            }
            other => panic!("expected an assistant message, got {other:?}"),
        }
    }

    #[test]
    fn setting_a_speaker_never_removes_one_the_model_already_sent() {
        let named = ModelMessage::assistant("A line.").with_speaker(Some("character-wen".into()));
        let anonymous = ModelMessage::assistant("Another line.");

        assert_eq!(named.with_speaker(None).speaker(), Some("character-wen"));
        assert_eq!(
            anonymous
                .with_speaker(Some("character-lin".into()))
                .speaker(),
            Some("character-lin")
        );
    }
}
