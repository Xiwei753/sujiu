use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::WorldBook;

/// The version a `Character` document written by this build writes.
pub const CHARACTER_SCHEMA_VERSION: u32 = 2;

/// A character card.
///
/// A character is a *participant*, not the root of a chat. It used to own a
/// whole `WorldBook`, which tied a world's lifetime to one card's storage and
/// made "the same lore, used by a different card" impossible to express; world
/// books are independent resources now, and a card names the ones it defaults
/// to.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Character {
    /// Absent in documents written before the schema was versioned.
    #[serde(default = "character_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub first_message: String,
    #[serde(default)]
    pub alternate_greetings: Vec<String>,
    #[serde(default)]
    pub example_dialogue: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub post_history_instructions: String,
    /// World books this character brings to a conversation by default.
    ///
    /// A default, not a binding: what the conversation actually uses is the
    /// conversation's own list, plus these.
    #[serde(default)]
    pub worldbook_ids: Vec<String>,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            schema_version: CHARACTER_SCHEMA_VERSION,
            id: String::new(),
            name: String::new(),
            description: String::new(),
            personality: String::new(),
            scenario: String::new(),
            first_message: String::new(),
            alternate_greetings: Vec::new(),
            example_dialogue: String::new(),
            system_prompt: String::new(),
            post_history_instructions: String::new(),
            worldbook_ids: Vec::new(),
            extensions: Map::new(),
        }
    }
}

impl Character {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }
}

fn character_schema_version() -> u32 {
    CHARACTER_SCHEMA_VERSION
}

/// Pull a world book out of a stored character that still carries one.
///
/// Migration only. Version 1 cards embedded their `WorldBook` inline, so a
/// character read out of an old store parses without complaint and would then
/// silently lose its lore the first time it was written back — the embedded
/// field is not part of `Character` any more, and serde ignores what it does not
/// know. This takes the field out of the stored value, gives the book an id of
/// its own so it can live as an independent resource, and leaves the value
/// without it.
pub fn split_embedded_world_book(stored: &mut Value, owner_id: &str) -> Option<WorldBook> {
    let object = stored.as_object_mut()?;
    let embedded = object
        .remove("world_book")
        .or_else(|| object.remove("worldBook"))?;

    let mut book: WorldBook = serde_json::from_value(embedded).ok()?;
    if book.id.trim().is_empty() {
        let owner = if owner_id.trim().is_empty() {
            "world"
        } else {
            owner_id.trim()
        };
        book.id = format!("{owner}-world-book");
    }

    Some(book)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The whole point of the split: a card that carried its world book must
    /// lose nothing when it is read out of an old store.
    #[test]
    fn an_embedded_world_book_leaves_the_card_with_its_entries_intact() {
        let mut stored = json!({
            "id": "card",
            "name": "Aerin",
            "world_book": {
                "name": "The World",
                "entries": [{
                    "id": "tower",
                    "content": "The tower is north.",
                    "keys": ["tower"]
                }]
            }
        });

        let book = split_embedded_world_book(&mut stored, "card").expect("a world book");

        assert_eq!(book.name, "The World");
        assert_eq!(book.entries.len(), 1);
        assert_eq!(book.entries[0].content, "The tower is north.");
        // It needs an identity of its own before it can be stored separately.
        assert_eq!(book.id, "card-world-book");
        // And the value it was taken from no longer carries it, so writing it
        // back cannot produce a card with two copies of the same lore.
        assert!(stored.get("world_book").is_none());

        let character: Character = serde_json::from_value(stored).expect("the card still parses");
        assert_eq!(character.worldbook_ids, Vec::<String>::new());
    }

    /// The camelCase spelling is read too, because that is how the field reached
    /// disk.
    #[test]
    fn an_embedded_world_book_is_read_under_either_spelling() {
        let mut stored = json!({
            "id": "card",

            "worldBook": { "name": "The World", "entries": [] }
        });

        let book = split_embedded_world_book(&mut stored, "card").expect("a world book");

        assert_eq!(book.name, "The World");
        assert!(stored.get("worldBook").is_none());
    }

    /// A card with no world book is the ordinary case and must not fail here.
    #[test]
    fn a_card_without_an_embedded_world_book_is_left_alone() {
        let mut stored = json!({ "id": "card", "name": "Aerin" });

        assert!(split_embedded_world_book(&mut stored, "card").is_none());
        assert_eq!(stored["name"], "Aerin");
    }

    /// An id that was already there is kept: the migration invents one only when
    /// there is none, and overwriting a real one would break the reference a
    /// conversation already made.
    #[test]
    fn a_world_book_that_already_has_an_id_keeps_it() {
        let mut stored = json!({
            "id": "card",
            "world_book": {
                "id": "shared-lore",
                "name": "Lore",
                "entries": [{ "id": "one", "content": "x", "keys": [] }]
            }
        });

        let book = split_embedded_world_book(&mut stored, "card").expect("a world book");

        assert_eq!(book.id, "shared-lore");
        assert_eq!(book.entries[0].id, "one");
    }
}
