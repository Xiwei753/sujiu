use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::WorldBook;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Character {
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
    #[serde(default)]
    pub world_book: Option<WorldBook>,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            schema_version: 1,
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
            world_book: None,
            extensions: Map::new(),
        }
    }
}
