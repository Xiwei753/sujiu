use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Transcript, UiMessage};

/// The version a `Conversation` document written by this build writes.
pub const CONVERSATION_SCHEMA_VERSION: u32 = 1;

/// Why a character is in a conversation.
///
/// The role is not a speaking-order scheduler. It exists so a table can be
/// recorded truthfully as what it is: one narrator, several characters, and
/// whoever the user is, without inventing a "main character" to hang the others
/// off. Ordering and turn-taking policy stay UI concerns.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantRole {
    #[default]
    /// An ordinary character card taking part in the conversation.
    Character,
    /// Someone outside the conversation playing it: a narrator or game master.
    Narrator,
}

/// One character taking part in a conversation.
///
/// A conversation has zero, one or many of these. Zero is real — a user can
/// start an empty conversation and pick participants later — and one is the
/// ordinary case. Two or more is group chat or a table, and neither required
/// faking a primary character to be expressible.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Participant {
    pub character_id: String,
    #[serde(default)]
    pub role: ParticipantRole,
    /// Overrides the card's own name when a table gives an actor a title
    /// ("the Innkeeper", "Captain Vey") that the card does not carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl Participant {
    pub fn character(character_id: impl Into<String>) -> Self {
        Self {
            character_id: character_id.into(),
            role: ParticipantRole::Character,
            display_name: None,
        }
    }

    pub fn narrator(character_id: impl Into<String>) -> Self {
        Self {
            role: ParticipantRole::Narrator,
            ..Self::character(character_id)
        }
    }
}

/// A conversation and the canonical record of what the model was shown.
///
/// The conversation is the root entity. A character does not own a chat and a
/// chat is not filed under a character: characters are participants, the
/// transcript belongs to the conversation, and the persona, world books and
/// prompt profile are things the conversation binds.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Conversation {
    #[serde(default = "conversation_schema_version")]
    pub schema_version: u32,
    pub id: String,
    /// Who is in it. Empty is valid.
    #[serde(default)]
    pub participants: Vec<Participant>,
    /// The persona the user brings to this conversation, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
    /// World books this conversation uses. A character's own defaults are
    /// added on top of these when the prompt is assembled; they never replace
    /// what the conversation chose.
    #[serde(default)]
    pub worldbook_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_profile_id: Option<String>,
    /// The model transcript. Append-only apart from explicit compaction.
    #[serde(default)]
    pub transcript: Transcript,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            schema_version: CONVERSATION_SCHEMA_VERSION,
            id: String::new(),
            participants: Vec::new(),
            persona_id: None,
            worldbook_ids: Vec::new(),
            prompt_profile_id: None,
            transcript: Transcript::default(),
            metadata: Map::new(),
        }
    }
}

impl Conversation {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }

    /// The folded conversation a platform renders.
    pub fn ui_messages(&self) -> Vec<UiMessage> {
        self.transcript.ui_messages()
    }

    /// The text a world-book keyword scan reads.
    pub fn scan_text(&self) -> String {
        self.transcript.scan_text()
    }

    /// The one character in the conversation, when there is exactly one.
    ///
    /// This is a convenience for single-character callers, not a relationship:
    /// a conversation with two participants has no "primary" one, and asking
    /// for it yields `None` instead of an arbitrary answer.
    pub fn sole_participant(&self) -> Option<&Participant> {
        match self.participants.as_slice() {
            [participant] => Some(participant),
            _ => None,
        }
    }
}
fn conversation_schema_version() -> u32 {
    CONVERSATION_SCHEMA_VERSION
}
