use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::Transcript;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    Developer,
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

/// A conversation and the canonical record of what the model was shown.
///
/// The session holds the transcript itself. A list of flattened chat messages
/// is a projection of it, and storing that projection instead would lose every
/// intermediate assistant step, tool call and tool result as soon as a turn
/// used a tool.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: String,
    #[serde(default)]
    pub character_id: Option<String>,
    /// The model transcript. Append-only apart from explicit compaction.
    #[serde(default)]
    pub transcript: Transcript,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl Session {
    /// The folded conversation a platform renders.
    pub fn ui_messages(&self) -> Vec<crate::UiMessage> {
        self.transcript.ui_messages()
    }

    /// The text a world-book keyword scan reads.
    pub fn scan_text(&self) -> String {
        self.transcript.scan_text()
    }
}
