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

    /// The text injected into the prompt, or an empty string when there is
    /// nothing to say. Both parts are included because a persona is the one
    /// place a persona description and its instruction belong together.
    pub fn prompt_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.name.trim().is_empty() {
            parts.push(format!("Persona: {}", self.name.trim()));
        }
        if !self.description.trim().is_empty() {
            parts.push(format!("About the user:\n{}", self.description.trim()));
        }
        if !self.user_prompt.trim().is_empty() {
            parts.push(self.user_prompt.trim().to_owned());
        }
        parts.join("\n\n")
    }
}
fn persona_schema_version() -> u32 {
    PERSONA_SCHEMA_VERSION
}
