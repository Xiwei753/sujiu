use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

use crate::types::{ToolDefinition, ToolResult};

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("tool execution failed: {0}")]
    Execution(String),
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    async fn execute(&self, arguments: Value) -> Result<Value, ToolError>;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        let definition = tool.definition();
        self.tools.insert(definition.name.clone(), Arc::new(tool));
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn definitions_for<'a, I>(&self, names: I) -> Vec<ToolDefinition>
    where
        I: IntoIterator<Item = &'a String>,
    {
        names
            .into_iter()
            .filter_map(|name| self.tools.get(name))
            .map(|tool| tool.definition())
            .collect()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<ToolDefinition> {
        let query = query.to_lowercase();
        let terms = query
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();

        let mut scored = self
            .tools
            .values()
            .map(|tool| {
                let definition = tool.definition();
                let name = definition.name.to_lowercase();
                let description = definition.description.to_lowercase();
                let keywords = definition
                    .keywords
                    .iter()
                    .map(|keyword| keyword.to_lowercase())
                    .collect::<Vec<_>>();

                let mut score = if definition.always_available { 10_000 } else { 0 };

                for term in &terms {
                    if name.contains(term) {
                        score += 20;
                    }
                    if description.contains(term) {
                        score += 5;
                    }
                    if keywords.iter().any(|keyword| keyword.contains(term) || term.contains(keyword))
                    {
                        score += 30;
                    }
                }

                (score, definition)
            })
            .filter(|(score, _)| *score > 0)
            .collect::<Vec<_>>();

        scored.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| left.name.cmp(&right.name))
        });

        scored
            .into_iter()
            .take(limit)
            .map(|(_, definition)| definition)
            .collect()
    }

    pub fn always_available_names(&self) -> BTreeSet<String> {
        self.tools
            .values()
            .map(|tool| tool.definition())
            .filter(|definition| definition.always_available)
            .map(|definition| definition.name)
            .collect()
    }

    pub async fn execute(&self, call_id: String, name: String, arguments: Value) -> ToolResult {
        let Some(tool) = self.tools.get(&name) else {
            return ToolResult {
                call_id,
                name,
                output: serde_json::json!({
                    "error": "unknown_tool",
                }),
                is_error: true,
            };
        };

        match tool.execute(arguments).await {
            Ok(output) => ToolResult {
                call_id,
                name,
                output,
                is_error: false,
            },
            Err(error) => ToolResult {
                call_id,
                name,
                output: serde_json::json!({
                    "error": error.to_string(),
                }),
                is_error: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LoreSearch;

    #[async_trait]
    impl Tool for LoreSearch {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "search_lore".into(),
                description: "Search character and world lore.".into(),
                parameters: serde_json::json!({"type":"object"}),
                keywords: vec!["lore".into(), "world".into(), "setting".into()],
                always_available: false,
            }
        }

        async fn execute(&self, _arguments: Value) -> Result<Value, ToolError> {
            Ok(serde_json::json!([]))
        }
    }

    struct Calculator;

    #[async_trait]
    impl Tool for Calculator {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "calculator".into(),
                description: "Calculate arithmetic.".into(),
                parameters: serde_json::json!({"type":"object"}),
                keywords: vec!["math".into(), "calculate".into()],
                always_available: false,
            }
        }

        async fn execute(&self, _arguments: Value) -> Result<Value, ToolError> {
            Ok(serde_json::json!({}))
        }
    }

    #[test]
    fn keyword_search_does_not_expose_every_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(LoreSearch);
        registry.register(Calculator);

        let tools = registry.search("Tell me about the world setting", 4);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search_lore");
    }
}
