use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    WorldLore,
    StoryEvent,
    CharacterMemory,
    ChatHistory,
    Persona,
    Note,
    Other,
}

/// What a context record is about.
///
/// A record can be scoped to a character, to a conversation, or to a persona.
/// The conversation is the primary scope now: a chat belongs to no single
/// character, so anything recorded *for a chat* records the conversation id.
///
/// `session_id` is kept as a read-only alias of `conversation_id`. Stored
/// documents written before the split carry `sessionId`, and reading them
/// through a flat reader would otherwise drop the scope silently.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    #[serde(default, alias = "session_id", skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ContextSource {
    pub id: String,
    pub kind: ContextKind,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub scope: ContextScope,
    #[serde(default)]
    pub mutable: bool,
    #[serde(default)]
    pub record_count: usize,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ContextRecord {
    /// Stable address used by read_context. The concrete storage backend owns
    /// the URI scheme; built-in stores use sujiu://context/...
    pub uri: String,
    pub source_id: String,
    pub kind: ContextKind,
    #[serde(default)]
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub scope: ContextScope,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}
