//! Normalized turn events crossing the FFI boundary.
//!
//! Provider wire events never leave the provider adapters. Adapters emit
//! deltas through `StreamSink`, the agent loop adds tool lifecycle events, and
//! this module turns the result into the provider-neutral event vocabulary the
//! platforms render.

use serde::Serialize;
use sujiu_ai::{CancelToken, StreamSink, ToolCall, ToolResult};

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
            text: None,
            tool_name: Some(call.name.clone()),
            tool_call_id: Some(call.id.clone()),
            is_error: Some(result.output.is_error),
        });
    }

    fn should_continue(&self) -> bool {
        !self.cancel.is_cancelled()
    }
}
