//! Normalized turn events crossing the FFI boundary.
//!
//! Provider wire events never leave the provider adapters. Adapters emit
//! deltas through `StreamSink`, the agent loop adds tool lifecycle events, and
//! this module turns the result into the provider-neutral event vocabulary the
//! platforms render.

use serde::Serialize;
use sujiu_ai::{CancelToken, StreamSink, ToolCall, ToolContent, ToolResult};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnEventKind {
    TurnStarted,
    TextDelta,
    ThinkingDelta,
    ToolCallRequested,
    ToolCallStarted,
    ToolCallFinished,
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
}

/// One normalized event. Absent fields stay absent so platforms can switch on
/// `kind` and read only what that event carries.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnEvent {
    pub kind: TurnEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl TurnEvent {
    pub fn simple(kind: TurnEventKind) -> Self {
        Self {
            kind,
            text: None,
            tool_name: None,
            tool_call_id: None,
            is_error: None,
        }
    }

    pub fn text(kind: TurnEventKind, text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::simple(kind)
        }
    }
}

/// Receives normalized turn events in order.
pub trait TurnEventReporter: Send {
    fn report(&mut self, event: TurnEvent);
}

/// Bridges the agent loop and the provider adapters into normalized events.
pub struct TurnEventSink<'a> {
    reporter: &'a mut dyn TurnEventReporter,
    cancel: CancelToken,
}

impl<'a> TurnEventSink<'a> {
    pub fn new(reporter: &'a mut dyn TurnEventReporter, cancel: CancelToken) -> Self {
        Self { reporter, cancel }
    }

    pub fn turn_started(&mut self) {
        self.reporter
            .report(TurnEvent::simple(TurnEventKind::TurnStarted));
    }

    pub fn turn_completed(&mut self, text: &str) {
        self.reporter
            .report(TurnEvent::text(TurnEventKind::TurnCompleted, text));
    }

    pub fn turn_failed(&mut self, message: &str) {
        self.reporter
            .report(TurnEvent::text(TurnEventKind::TurnFailed, message));
    }

    pub fn turn_cancelled(&mut self) {
        self.reporter
            .report(TurnEvent::simple(TurnEventKind::TurnCancelled));
    }
}

impl StreamSink for TurnEventSink<'_> {
    fn on_text_delta(&mut self, delta: &str) {
        self.reporter
            .report(TurnEvent::text(TurnEventKind::TextDelta, delta));
    }

    fn on_reasoning_delta(&mut self, delta: &str) {
        self.reporter
            .report(TurnEvent::text(TurnEventKind::ThinkingDelta, delta));
    }

    fn on_tool_call_requested(&mut self, call: &ToolCall) {
        self.reporter.report(TurnEvent {
            kind: TurnEventKind::ToolCallRequested,
            text: None,
            tool_name: Some(call.name.clone()),
            tool_call_id: Some(call.id.clone()),
            is_error: None,
        });
    }

    fn on_tool_call_started(&mut self, call: &ToolCall) {
        self.reporter.report(TurnEvent {
            kind: TurnEventKind::ToolCallStarted,
            text: None,
            tool_name: Some(call.name.clone()),
            tool_call_id: Some(call.id.clone()),
            is_error: None,
        });
    }

    fn on_tool_call_finished(&mut self, call: &ToolCall, result: &ToolResult) {
        self.reporter.report(TurnEvent {
            kind: TurnEventKind::ToolCallFinished,
            text: Some(summary_of(result)),
            tool_name: Some(call.name.clone()),
            tool_call_id: Some(call.id.clone()),
            is_error: Some(result.output.is_error),
        });
    }

    fn should_continue(&self) -> bool {
        !self.cancel.is_cancelled()
    }
}

/// A short, single-line result summary for the platform to show under a tool
/// call. Tool results can be large, so this is a preview and not the payload:
/// the model already received the full result.
fn summary_of(result: &ToolResult) -> String {
    const LIMIT: usize = 120;
    let mut text = String::new();
    for block in &result.output.content {
        let piece = match block {
            ToolContent::Text { text } => text.clone(),
            ToolContent::Resource { uri, .. } => uri.clone(),
            _ => continue,
        };
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(piece.trim());
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= LIMIT {
        return collapsed;
    }
    let mut truncated: String = collapsed.chars().take(LIMIT).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::{TurnEvent, TurnEventKind};

    /// Every kind crosses the FFI boundary as one of these strings, and each
    /// platform switches on them by value. Renaming a variant or changing the
    /// rename rule silently breaks every frontend at runtime, because a platform
    /// that does not recognize a kind cannot tell a new event from a broken
    /// one. So the wire spelling is pinned here.
    const WIRE_KINDS: &[(TurnEventKind, &str)] = &[
        (TurnEventKind::TurnStarted, "turn_started"),
        (TurnEventKind::TextDelta, "text_delta"),
        (TurnEventKind::ThinkingDelta, "thinking_delta"),
        (TurnEventKind::ToolCallRequested, "tool_call_requested"),
        (TurnEventKind::ToolCallStarted, "tool_call_started"),
        (TurnEventKind::ToolCallFinished, "tool_call_finished"),
        (TurnEventKind::TurnCompleted, "turn_completed"),
        (TurnEventKind::TurnFailed, "turn_failed"),
        (TurnEventKind::TurnCancelled, "turn_cancelled"),
    ];

    #[test]
    fn event_kinds_serialize_to_the_documented_wire_strings() {
        for (kind, expected) in WIRE_KINDS {
            let json = serde_json::to_string(&TurnEvent::simple(*kind)).expect("serializable");
            assert_eq!(
                json,
                format!("{{\"kind\":\"{expected}\"}}"),
                "a platform switches on this exact string"
            );
        }
    }

    #[test]
    fn an_absent_field_stays_absent_from_the_payload() {
        // A start event carries no text, so a platform must not have to read an
        // empty field to know the turn has only begun.
        let json = serde_json::to_string(&TurnEvent::simple(TurnEventKind::TurnStarted))
            .expect("serializable");
        assert!(!json.contains("text"), "{json}");
    }

    #[test]
    fn a_failed_event_carries_the_reason_as_text() {
        let json = serde_json::to_string(&TurnEvent::text(
            TurnEventKind::TurnFailed,
            "missing_credential",
        ))
        .expect("serializable");
        assert_eq!(
            json,
            "{\"kind\":\"turn_failed\",\"text\":\"missing_credential\"}"
        );
    }
}
