use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

use crate::types::{ToolDefinition, ToolOutput, ToolResult};

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

    async fn execute(&self, arguments: Value) -> Result<ToolOutput, ToolError>;
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
            .filter(|term| is_significant_term(term))
            .collect::<Vec<_>>();

        let mut scored =
            self.tools
                .values()
                .map(|tool| {
                    let definition = tool.definition();
                    let name = definition.name.to_lowercase();
                    let title = definition.title.clone().unwrap_or_default().to_lowercase();
                    let description = definition.description.to_lowercase();
                    let category = definition.discovery.category.to_lowercase();
                    let keywords = definition
                        .discovery
                        .keywords
                        .iter()
                        .map(|keyword| keyword.to_lowercase())
                        .collect::<Vec<_>>();

                    let mut score = 0;

                    if !query.is_empty() {
                        if name.contains(&query) || query.contains(&name) {
                            score += 80;
                        }
                        if !title.is_empty() && (title.contains(&query) || query.contains(&title)) {
                            score += 50;
                        }
                        if description.contains(&query) {
                            score += 20;
                        }
                        if !category.is_empty() && query.contains(&category) {
                            score += 25;
                        }
                        if keywords
                            .iter()
                            .any(|keyword| query.contains(keyword) || keyword.contains(&query))
                        {
                            score += 60;
                        }
                    }

                    for term in &terms {
                        if name.contains(*term) {
                            score += 20;
                        }
                        if title.contains(*term) {
                            score += 10;
                        }
                        if description.contains(*term) {
                            score += 5;
                        }
                        if category.contains(*term) {
                            score += 8;
                        }
                        if keywords.iter().any(|keyword| {
                            keyword.contains(*term) || term.contains(keyword.as_str())
                        }) {
                            score += 30;
                        }
                    }

                    (score, definition)
                })
                .filter(|(score, _)| *score >= 15)
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
            .filter(|definition| definition.discovery.always_available)
            .map(|definition| definition.name)
            .collect()
    }

    pub async fn execute(&self, call_id: String, name: String, arguments: Value) -> ToolResult {
        let output = match self.tools.get(&name) {
            Some(tool) => match tool.execute(arguments).await {
                Ok(output) => output,
                Err(error) => ToolOutput::error(error.to_string()),
            },
            None => ToolOutput::error("unknown_tool"),
        };

        ToolResult {
            call_id,
            name,
            output,
        }
    }
}

fn is_significant_term(term: &str) -> bool {
    let char_count = term.chars().count();
    char_count >= 3 || (char_count >= 2 && term.chars().any(|ch| !ch.is_ascii()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ToolAnnotations, ToolDiscovery};

    struct LoreSearch;

    #[async_trait]
    impl Tool for LoreSearch {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "search_context".into(),
                title: Some("Search context".into()),
                description: "Search world lore, story history and memory.".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: None,
                annotations: ToolAnnotations {
                    read_only_hint: true,
                    idempotent_hint: true,
                    ..ToolAnnotations::default()
                },
                discovery: ToolDiscovery {
                    category: "context".into(),
                    keywords: vec!["lore".into(), "world".into(), "history".into()],
                    always_available: false,
                },
            }
        }

        async fn execute(&self, _arguments: Value) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::structured(serde_json::json!({"hits":[]})))
        }
    }

    struct Calculator;

    #[async_trait]
    impl Tool for Calculator {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "calculator".into(),
                title: None,
                description: "Calculate arithmetic.".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: None,
                annotations: ToolAnnotations {
                    read_only_hint: true,
                    idempotent_hint: true,
                    ..ToolAnnotations::default()
                },
                discovery: ToolDiscovery {
                    category: "utility".into(),
                    keywords: vec!["math".into(), "calculate".into()],
                    always_available: false,
                },
            }
        }

        async fn execute(&self, _arguments: Value) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::structured(serde_json::json!({})))
        }
    }

    #[test]
    fn keyword_search_does_not_expose_every_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(LoreSearch);
        registry.register(Calculator);

        let tools = registry.search("Tell me about the world history", 4);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search_context");
    }
}
