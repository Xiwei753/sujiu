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

    /// Every tool call in this turn, in call order.
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCallRecord> {
        self.steps.iter().flat_map(|step| step.tool_calls.iter())
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
/// previous summary and every turn archived so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompactionInput {
    /// The summary that already stands in for the archived turns, empty before
    /// the first compaction.
    pub previous_summary: String,
    /// Every turn a new summary has to cover: the already-archived ones first,
    /// then the ones this compaction would move.
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
    /// The summarizer is handed *every* archived turn plus the summary that
    /// already stood in for them, so a second compaction can produce a summary
    /// of the whole compacted history. Overwriting the previous summary with a
    /// summary of only the newly archived turns would silently drop everything
    /// older out of the model's view.
    ///
    /// Returns `false` when there is nothing old enough to compact, which keeps a
    /// caller from rewriting a history that did not need rewriting.
    /// What a caller has to summarize in order to compact this transcript.
    ///
    /// Exposing it matters: an upper layer that only ever sees the turns being
    /// archived *now* cannot write a summary of the whole compacted history, and
    /// would silently overwrite the summary that stood in for the older turns.
    /// The input always carries the previous summary and every already-archived
    /// turn, so a cumulative summary is something the caller can actually build.
    pub fn compaction_input(&self, keep_recent: usize) -> Option<CompactionInput> {
        let keep_from = self.turns.len().saturating_sub(keep_recent);
        if keep_from == 0 {
            return None;
        }

        let mut turns: Vec<Turn> = self
            .compacted
            .as_ref()
            .map(|archived| archived.turns.clone())
            .unwrap_or_default();
        turns.extend(self.turns[..keep_from].iter().cloned());

        Some(CompactionInput {
            previous_summary: self
                .compacted
                .as_ref()
                .map(|archived| archived.summary.clone())
                .unwrap_or_default(),
            turns,
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
        let mut to_summarize: Vec<Turn> = self
            .compacted
            .as_ref()
            .map(|archived| archived.turns.clone())
            .unwrap_or_default();
        to_summarize.extend(moved.iter().cloned());
        let previous_summary = self
            .compacted
            .as_ref()
            .map(|archived| archived.summary.clone())
            .unwrap_or_default();
        let summary = summarize(&to_summarize, &previous_summary);

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
        self.turns
            .iter()
            .rev()
            .find_map(Turn::continuation_event)
            .and_then(|event| match event {
                crate::model::ContinuationUpdate::Replace(continuation) => Some(continuation),
                crate::model::ContinuationUpdate::Clear => None,
                crate::model::ContinuationUpdate::Unchanged => None,
            })
            .filter(|continuation| continuation.is_reusable_for(identity))
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
    pub fn ui_messages(&self) -> Vec<UiMessage> {
        let mut messages = Vec::new();

        for turn in &self.turns {
            messages.push(UiMessage {
                id: format!("{}-user", turn.id),
                role: ChatRole::User,
                text: turn.user.clone(),
                tool_calls: Vec::new(),
            });

            // The tool steps are folded into the assistant bubble. A platform
            // may hide or collapse them; the transcript above still holds
            // every one of them.
            let text = turn.final_text().unwrap_or_default().to_owned();
            let tool_calls: Vec<UiToolCall> = turn
                .tool_calls()
                .map(|call| UiToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    title: call.title.clone().unwrap_or_else(|| call.name.clone()),
                    state: call.result.state,
                    is_error: call.result.is_error(),
                    result_text: call.result.model_text(),
                })
                .collect();

            if text.is_empty() && tool_calls.is_empty() {
                continue;
            }

            messages.push(UiMessage {
                id: format!("{}-assistant", turn.id),
                role: ChatRole::Assistant,
                text,
                tool_calls,
            });
        }

        messages
    }
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
        ProviderKind,
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
            ProviderKind::OpenAiCompatible,
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
                reasoning,
                calls,
            } => {
                assert_eq!(content.as_deref(), Some("The light stopped blinking."));
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
            ProviderKind::OpenAiCompatible,
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
                ProviderKind::OpenAiCompatible,
                "p1",
                "https://a.example/v1",
                "m",
            ),
        ));

        assert!(thinking.model_messages().is_empty());
    }

    fn identity(
        kind: ProviderKind,
        provider_id: &str,
        base_url: &str,
        model: &str,
    ) -> ProviderIdentity {
        ProviderIdentity {
            kind,
            provider_id: provider_id.to_string(),
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
                ProviderKind::OpenAiCompatible,
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
                ProviderKind::OpenAiCompatible,
                "provider-a",
                "https://a.example/v1",
                "model-a"
            ))
            .is_some());

        // Two OpenAI-compatible gateways serving the same model name are still
        // two different providers.
        assert!(transcript
            .continuation_for(&identity(
                ProviderKind::OpenAiCompatible,
                "provider-b",
                "https://b.example/v1",
                "model-a"
            ))
            .is_none());
        assert!(transcript
            .continuation_for(&identity(
                ProviderKind::OpenAiCompatible,
                "provider-a",
                "https://a.example/v1",
                "model-b"
            ))
            .is_none());
        assert!(transcript
            .continuation_for(&identity(
                ProviderKind::Anthropic,
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
                ProviderKind::OpenAiCompatible,
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
                ProviderKind::OpenAiCompatible,
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
                ProviderKind::OpenAiCompatible,
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
            ProviderKind::OpenAiCompatible,
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
                ProviderKind::OpenAiCompatible,
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
            ProviderKind::OpenAiCompatible,
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
            ProviderKind::OpenAiCompatible,
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
    fn a_second_compaction_summarizes_every_archived_turn_not_only_the_new_ones() {
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
        // The summarizer saw the whole compacted history, not just this round's
        // share of it, so overwriting the summary cannot drop older history out
        // of the model's view.
        assert_eq!(archived.summary, "first: 1 turns then 2 more");
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
}
