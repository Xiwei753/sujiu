use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The version a `PromptProfile` document written by this build writes.
pub const PROMPT_PROFILE_SCHEMA_VERSION: u32 = 1;

/// The fixed prompt a conversation is spoken with.
///
/// Before this existed the same instructions were spread across the character
/// card, a provider override and whatever a platform happened to be holding in
/// UI state, which meant "what is this conversation actually run with?" had no
/// single answer. A profile is that answer: the parts of a prompt that do not
/// change per turn, kept as one entity a conversation binds.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PromptProfile {
    #[serde(default = "prompt_profile_schema_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Replaces the app system prompt for a conversation bound to this profile.
    #[serde(default)]
    pub system_prompt: String,
    /// Fixed user-side instruction, injected with the prompt block.
    #[serde(default)]
    pub user_prompt: String,
    /// Applied after the transcript, where "never speak for the user" belongs.
    #[serde(default)]
    pub post_history_instructions: String,
    /// Output shape rules. Kept out of the system prompt so a caller can tell
    /// what the model is told about the world from what it is told about the
    /// shape of its answer.
    #[serde(default)]
    pub format_rules: String,
    /// Other fixed segments, when a prompt needs more than the named parts.
    #[serde(default)]
    pub segments: Vec<PromptProfileSegment>,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

/// One extra fixed segment of a profile.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PromptProfileSegment {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

fn prompt_profile_schema_version() -> u32 {
    PROMPT_PROFILE_SCHEMA_VERSION
}

impl Default for PromptProfile {
    fn default() -> Self {
        Self {
            schema_version: PROMPT_PROFILE_SCHEMA_VERSION,
            id: String::new(),
            name: String::new(),
            system_prompt: String::new(),
            user_prompt: String::new(),
            post_history_instructions: String::new(),
            format_rules: String::new(),
            segments: Vec::new(),
            extensions: Map::new(),
        }
    }
}

impl PromptProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }
}
