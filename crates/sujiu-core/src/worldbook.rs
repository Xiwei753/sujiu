use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorldBookPosition {
    BeforeCharacter,
    #[default]
    AfterCharacter,
    NearHistory,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldBookEntry {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub constant: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub position: WorldBookPosition,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

impl WorldBookEntry {
    pub fn is_active_for(&self, text: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if self.constant {
            return true;
        }

        let haystack = text.to_lowercase();
        self.keys.iter().any(|key| {
            let key = key.trim();
            !key.is_empty() && haystack.contains(&key.to_lowercase())
        })
    }
}

/// The version a `WorldBook` document written by this build writes.
pub const WORLD_BOOK_SCHEMA_VERSION: u32 = 2;

/// An independent world book.
///
/// A world book is a resource, not a character field. It can be bound to a
/// conversation, be a character's default, a persona's default, or global, and
/// more than one can apply at once. Its lifetime has nothing to do with any
/// character card, which is what stops "this lore belongs to a card" from
/// deciding where the lore is stored.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldBook {
    /// Absent in documents written before the schema was versioned. It reads as
    /// the version this build writes rather than as zero, because a zero is a
    /// version nobody wrote and would only invite a migration to guess.
    #[serde(default = "world_book_schema_version")]
    pub schema_version: u32,
    /// Empty only for a book that has not been given an identity yet, such as
    /// one just decoded from a character card. A stored book always has one.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub entries: Vec<WorldBookEntry>,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

impl Default for WorldBook {
    fn default() -> Self {
        Self {
            schema_version: WORLD_BOOK_SCHEMA_VERSION,
            id: String::new(),
            name: String::new(),
            entries: Vec::new(),
            extensions: Map::new(),
        }
    }
}

impl WorldBook {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }
}

fn default_true() -> bool {
    true
}

fn world_book_schema_version() -> u32 {
    WORLD_BOOK_SCHEMA_VERSION
}
