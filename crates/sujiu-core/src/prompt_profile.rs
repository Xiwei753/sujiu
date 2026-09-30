use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ChatRole;

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
    /// Fixed user-side instruction.
    ///
    /// Sent as a real user-role message, not folded into the system prompt.
    /// That is the whole point of keeping it a field: an instruction the user
    /// gave is on the user's side of the conversation, and a system prompt that
    /// claims to be the user is a prompt that lies about who is speaking.
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
///
/// Role and position are stored rather than assumed, because "a fixed prompt
/// segment" is not one thing. A format rule is a system instruction, a refusal
/// rule belongs after the transcript where it can still see the turn it applies
/// to, and a standing user request is the user's own message. Hard-coding all
/// three as system text at the top is what made `user_prompt` a dead field: the
/// content existed and nothing could carry it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PromptProfileSegment {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub content: String,
    /// Which side of the conversation this segment speaks from.
    ///
    /// Defaults to the system prompt, which is what a segment was before this
    /// field existed. `Assistant` is accepted by the wire format and has no
    /// meaning here: a fixed segment is not something the model already said.
    #[serde(default)]
    pub role: ChatRole,
    /// Whether it leads the prompt block or follows the transcript.
    #[serde(default)]
    pub position: PromptPosition,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

/// Where a fixed prompt segment belongs relative to the transcript.
///
/// Deliberately coarser than a world-book position: this is about which side of
/// the conversation the text is on, not where in a lore block it belongs. A
/// segment either leads the prompt block, or it is re-sent after the transcript
/// every turn so it applies to the turn about to happen.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptPosition {
    /// Before the transcript, in the cacheable prompt block.
    #[default]
    Prefix,
    /// After the transcript, next to the post-history instructions.
    PostHistory,
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
