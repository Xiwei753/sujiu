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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WorldBook {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub entries: Vec<WorldBookEntry>,
    #[serde(default)]
    pub extensions: Map<String, Value>,
}

fn default_true() -> bool {
    true
}
