use async_trait::async_trait;
use serde_json::{json, Value};
use sujiu_core::WorldBook;

use crate::{
    tool::{Tool, ToolError},
    types::ToolDefinition,
};

#[derive(Clone)]
pub struct WorldBookSearchTool {
    book: WorldBook,
    max_results: usize,
}

impl WorldBookSearchTool {
    pub fn new(book: WorldBook) -> Self {
        Self {
            book,
            max_results: 8,
        }
    }

    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results.max(1);
        self
    }
}

#[async_trait]
impl Tool for WorldBookSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_world_book".into(),
            description: "Search the current character's world book/lore for details that are not already present in the prompt. Use it for people, places, factions, history, rules, items, relationships, and setting facts.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "A short natural-language description or keyword for the lore to retrieve."
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            keywords: vec![
                "lore".into(),
                "world".into(),
                "setting".into(),
                "character".into(),
                "history".into(),
                "place".into(),
                "faction".into(),
                "世界".into(),
                "设定".into(),
                "角色".into(),
                "人物".into(),
                "地点".into(),
                "历史".into(),
                "背景".into(),
                "势力".into(),
            ],
            always_available: false,
        }
    }

    async fn execute(&self, arguments: Value) -> Result<Value, ToolError> {
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::InvalidArguments("query is required".into()))?;

        let query_lower = query.to_lowercase();
        let query_terms = query_lower
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();

        let mut scored = self
            .book
            .entries
            .iter()
            .filter(|entry| entry.enabled)
            .filter_map(|entry| {
                let name = entry.name.to_lowercase();
                let content = entry.content.to_lowercase();
                let keys = entry
                    .keys
                    .iter()
                    .map(|key| key.to_lowercase())
                    .collect::<Vec<_>>();

                let mut score = 0_i32;

                if !name.is_empty()
                    && (name.contains(&query_lower) || query_lower.contains(&name))
                {
                    score += 80;
                }

                for key in &keys {
                    if !key.is_empty()
                        && (query_lower.contains(key) || key.contains(&query_lower))
                    {
                        score += 100;
                    }
                }

                for term in &query_terms {
                    if name.contains(*term) {
                        score += 20;
                    }
                    if content.contains(*term) {
                        score += 5;
                    }
                    if keys.iter().any(|key| key.contains(*term)) {
                        score += 30;
                    }
                }

                if score == 0 {
                    None
                } else {
                    Some((score, entry))
                }
            })
            .collect::<Vec<_>>();

        scored.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right.priority.cmp(&left.priority))
                .then_with(|| left.id.cmp(&right.id))
        });

        let entries = scored
            .into_iter()
            .take(self.max_results)
            .map(|(score, entry)| {
                json!({
                    "id": entry.id,
                    "name": entry.name,
                    "content": entry.content,
                    "priority": entry.priority,
                    "score": score,
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "query": query,
            "entries": entries,
        }))
    }
}

#[cfg(test)]
mod tests {
    use sujiu_core::{WorldBookEntry, WorldBookPosition};

    use super::*;

    #[tokio::test]
    async fn searches_world_book_without_dumping_every_entry() {
        let book = WorldBook {
            name: "demo".into(),
            entries: vec![
                WorldBookEntry {
                    id: "tower".into(),
                    name: "黑塔".into(),
                    content: "黑塔位于王都北面，禁止普通人进入。".into(),
                    keys: vec!["黑塔".into()],
                    enabled: true,
                    constant: false,
                    priority: 10,
                    position: WorldBookPosition::AfterCharacter,
                    extensions: Default::default(),
                },
                WorldBookEntry {
                    id: "harbor".into(),
                    name: "港口".into(),
                    content: "港口每天清晨开放。".into(),
                    keys: vec!["港口".into()],
                    enabled: true,
                    constant: false,
                    priority: 0,
                    position: WorldBookPosition::AfterCharacter,
                    extensions: Default::default(),
                },
            ],
            extensions: Default::default(),
        };

        let result = WorldBookSearchTool::new(book)
            .execute(json!({"query":"黑塔是什么地方"}))
            .await
            .unwrap();

        let entries = result["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["id"], "tower");
    }
}
