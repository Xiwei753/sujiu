use serde::{Deserialize, Serialize};

/// Who a message is from.
///
/// The role a model speaks in is not a UI concept: it decides which wire field a
/// provider adapter writes, so it belongs to the shared runtime.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    /// The default because a fixed instruction that names no side is addressed
    /// to the model, and the model is not the one who has to act on it.
    #[default]
    System,
    Developer,
    User,
    Assistant,
}

/// A flat message.
///
/// Only read, never written: a conversation stores a `Transcript` with its turns,
/// steps, tool calls and tool results, and this flat shape exists because version
/// 1 of the store kept conversations this way. Reading one is a migration, and
/// the steps that shape had nowhere to record are already lost — but the text
/// still is not, so it is kept rather than dropped.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}
