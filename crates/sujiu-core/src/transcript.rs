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
    ChatRole, ProviderKind,
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
    /// of the conversation the user reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<crate::model::ProviderContinuation>,
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
    /// A step that asked for tools becomes an `AssistantToolCalls` message
    /// followed by one `ToolResult` per call, in call order. A call that never
    /// finished still gets a result, because a provider rejects a call with no
    /// result at all.
    pub fn model_messages(&self) -> Vec<ModelMessage> {
        let mut messages = Vec::new();

        if self.has_tool_calls() {
            messages.push(ModelMessage::AssistantToolCalls {
                content: self.text.clone(),
                calls: self
                    .tool_calls
                    .iter()
                    .map(|call| ToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    })
                    .collect(),
            });

            messages.extend(self.tool_calls.iter().map(|call| ModelMessage::ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: call.result.model_text(),
                structured_content: call.result.structured_content.clone(),
                is_error: call.result.is_error(),
            }));
        } else if let Some(text) = self.text.as_ref().filter(|text| !text.trim().is_empty()) {
            messages.push(ModelMessage::assistant(text.clone()));
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

    /// The continuation state of the last step that produced any.
    pub fn continuation(&self) -> Option<&crate::model::ProviderContinuation> {
        self.steps
            .iter()
            .rev()
            .find_map(|step| step.continuation.as_ref())
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
    /// Returns `false` when there is nothing old enough to compact, which keeps a
    /// caller from rewriting a history that did not need rewriting.
    pub fn compact(&mut self, keep_recent: usize, summary: impl FnOnce(&[Turn]) -> String) -> bool {
        let keep_from = self.turns.len().saturating_sub(keep_recent);
        if keep_from == 0 {
            return false;
        }

        let summary = summary(&self.turns[..keep_from]);
        let moved: Vec<Turn> = self.turns.drain(..keep_from).collect();

        let archived = self.compacted.get_or_insert_with(CompactedTurns::default);
        archived.turns.extend(moved);
        archived.summary = summary;

        true
    }

    /// The text a keyword scan should see: everything the model was shown.
    pub fn scan_text(&self) -> String {
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
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Continuation state that may be replayed to `kind`/`model` as is.
    ///
    /// Returns `None` after a provider or model switch, which is what makes
    /// the runtime fall back to the normalized transcript instead of sending
    /// another provider's state.
    pub fn continuation_for(
        &self,
        kind: ProviderKind,
        model: &str,
    ) -> Option<&crate::model::ProviderContinuation> {
        self.turns
            .iter()
            .rev()
            .find_map(Turn::continuation)
            .filter(|continuation| continuation.is_reusable_for(kind, model))
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
    use crate::model::{ProviderContinuation, ToolOutput};
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
    fn continuation_state_is_only_reusable_for_the_same_provider_and_model() {
        let mut turn = tool_turn();
        turn.steps[0].continuation = Some(ProviderContinuation {
            provider_kind: ProviderKind::OpenAiCompatible,
            model: "model-a".into(),
            response_id: Some("resp-1".into()),
            state: Default::default(),
        });

        let mut transcript = Transcript::default();
        transcript.push(turn);

        assert!(transcript
            .continuation_for(ProviderKind::OpenAiCompatible, "model-a")
            .is_some());
        // A different model, or a different provider, must fall back to the
        // normalized transcript.
        assert!(transcript
            .continuation_for(ProviderKind::OpenAiCompatible, "model-b")
            .is_none());
        assert!(transcript
            .continuation_for(ProviderKind::Anthropic, "model-a")
            .is_none());
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

        let compacted = transcript.compact(2, |turns| format!("{} earlier turns", turns.len()));

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
    fn compaction_refuses_to_rewrite_a_history_that_does_not_need_it() {
        let mut transcript = Transcript::default();
        transcript.push(tool_turn());

        assert!(
            !transcript.compact(4, |_| "summary".to_string()),
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
