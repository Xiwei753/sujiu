use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The version a `Persona` document written by this build writes.
pub const PERSONA_SCHEMA_VERSION: u32 = 1;

/// Who the user is in a conversation.
///
/// This used to live inside the character, which made it impossible to answer
/// a question the product actually asks: what the model is told about the
/// person on the other side of the chat, and which of several such people a
/// conversation is with. It is its own entity now, and a conversation binds one
/// or none.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Persona {
    #[serde(default = "persona_schema_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// The persona text injected as this conversation's persona.
    #[serde(default)]
    pub description: String,
    /// Extra user-side instruction, kept separate from the description so a
    /// profile can append to it without rewriting the persona itself.
    ///
    /// Sent as a user-role message. Who the user *is* is a system-side fact
    /// about the conversation; what the user *wants asked of the model* is the
    /// user speaking, and collapsing the two into one system paragraph is how a
    /// persona's instruction ends up attributed to somebody else.
    #[serde(default)]
    pub user_prompt: String,
    /// World books this persona brings to a conversation by default.
    ///
    /// A default, not a binding: the conversation's own list is what decides.
    #[serde(default)]
    pub worldbook_ids: Vec<String>,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

impl Default for Persona {
    fn default() -> Self {
        Self {
            schema_version: PERSONA_SCHEMA_VERSION,
            id: String::new(),
            name: String::new(),
            description: String::new(),
            user_prompt: String::new(),
            worldbook_ids: Vec::new(),
            extensions: Map::new(),
        }
    }
}

impl Persona {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }

    /// Who the user is, as the system side of the conversation states it.
    ///
    /// Empty when there is nothing to say, so a caller can pass it straight to a
    /// prompt segment and let that decide whether there is a message at all.
    pub fn system_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.name.trim().is_empty() {
            parts.push(format!("Persona: {}", self.name.trim()));
        }
        if !self.description.trim().is_empty() {
            parts.push(format!("About the user:\n{}", self.description.trim()));
        }
        parts.join("\n\n")
    }

    /// What the user wants asked of the model, in the user's own voice.
    pub fn user_text(&self) -> &str {
        self.user_prompt.trim()
    }

    /// Both halves as one block of text, for callers that can only show prose —
    /// the context projection, mainly.
    ///
    /// The prompt compiler does not use this: it sends the two halves with
    /// their own roles. This exists so the searchable copy of a persona says
    /// everything the prompt does, and the two cannot drift apart.
    pub fn prompt_text(&self) -> String {
        let system = self.system_text();
        let user = self.user_text();
        match (system.is_empty(), user.is_empty()) {
            (true, true) => String::new(),
            (true, false) => user.to_owned(),
            (false, true) => system,
            (false, false) => format!("{system}\n\n{user}"),
        }
    }
}
fn persona_schema_version() -> u32 {
    PERSONA_SCHEMA_VERSION
}
