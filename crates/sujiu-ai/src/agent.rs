use std::collections::BTreeSet;

use serde_json::{json, Value};
use sujiu_core::PromptPlan;
use thiserror::Error;

use crate::{
    provider::{AiProvider, ProviderError},
    tool::ToolRegistry,
    types::{
        messages_from_prompt_plan, ModelMessage, ModelRole, ProviderRequest, ToolAnnotations,
        ToolCall, ToolDefinition, ToolDiscovery, ToolOutput, ToolResult,
    },
};

const SEARCH_TOOLS_NAME: &str = "sujiu_search_tools";

#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub max_rounds: usize,
    pub initial_tool_limit: usize,
    pub tool_search_limit: usize,
    pub enable_tool_search: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_rounds: 8,
            initial_tool_limit: 8,
            tool_search_limit: 8,
            enable_tool_search: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error(transparent)]
    Provider(#[from] ProviderError),

    #[error("tool loop exceeded {0} rounds")]
    MaxRounds(usize),
}

#[derive(Clone, Debug)]
pub struct AgentOutcome {
    pub final_text: String,
    pub rounds: usize,
    pub transcript: Vec<ModelMessage>,
    pub tool_results: Vec<ToolResult>,
}

pub struct AgentRuntime<'a, P: AiProvider> {
    provider: &'a P,
    tools: &'a ToolRegistry,
    config: AgentConfig,
}

impl<'a, P: AiProvider> AgentRuntime<'a, P> {
    pub fn new(provider: &'a P, tools: &'a ToolRegistry, config: AgentConfig) -> Self {
        Self {
            provider,
            tools,
            config,
        }
    }

    pub async fn run_prompt(&self, plan: &PromptPlan) -> Result<AgentOutcome, AgentError> {
        let messages = messages_from_prompt_plan(plan);
        let discovery_query = messages
            .iter()
            .rev()
            .find_map(|message| match message {
                ModelMessage::Text {
                    role: ModelRole::User,
                    content,
                } => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_default();

        self.run(messages, &discovery_query).await
    }

    pub async fn run(
        &self,
        mut messages: Vec<ModelMessage>,
        discovery_query: &str,
    ) -> Result<AgentOutcome, AgentError> {
        let mut active = self.tools.always_available_names();

        for definition in self
            .tools
            .search(discovery_query, self.config.initial_tool_limit)
        {
            active.insert(definition.name);
        }

        let mut all_results = Vec::new();

        for round in 1..=self.config.max_rounds {
            let definitions = self.request_tool_definitions(&active);

            let turn = self
                .provider
                .complete(ProviderRequest {
                    messages: messages.clone(),
                    tools: definitions,
                })
                .await?;

            if turn.tool_calls.is_empty() {
                let final_text = turn.text.unwrap_or_default();
                if !final_text.is_empty() {
                    messages.push(ModelMessage::Text {
                        role: ModelRole::Assistant,
                        content: final_text.clone(),
                    });
                }

                return Ok(AgentOutcome {
                    final_text,
                    rounds: round,
                    transcript: messages,
                    tool_results: all_results,
                });
            }

            let calls = turn.tool_calls;
            messages.push(ModelMessage::AssistantToolCalls {
                content: turn.text,
                calls: calls.clone(),
            });

            for call in calls {
                let result = if call.name == SEARCH_TOOLS_NAME {
                    self.execute_tool_search(&call, &mut active)
                } else if active.contains(&call.name) {
                    self.tools
                        .execute(call.id.clone(), call.name.clone(), call.arguments.clone())
                        .await
                } else {
                    ToolResult {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        output: ToolOutput::error(
                            "tool_not_exposed: this tool was not loaded for the current turn",
                        ),
                    }
                };

                messages.push(ModelMessage::ToolResult {
                    call_id: result.call_id.clone(),
                    name: result.name.clone(),
                    content: result.output.model_text(),
                    structured_content: result.output.structured_content.clone(),
                    is_error: result.output.is_error,
                });
                all_results.push(result);
            }
        }

        Err(AgentError::MaxRounds(self.config.max_rounds))
    }

    fn request_tool_definitions(&self, active: &BTreeSet<String>) -> Vec<ToolDefinition> {
        let mut definitions = self.tools.definitions_for(active.iter());

        if self.config.enable_tool_search && active.len() < self.tools.len() {
            definitions.push(search_tools_definition());
        }

        definitions
    }

    fn execute_tool_search(&self, call: &ToolCall, active: &mut BTreeSet<String>) -> ToolResult {
        let query = call
            .arguments
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let requested_limit = call
            .arguments
            .get("limit")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(self.config.tool_search_limit)
            .min(self.config.tool_search_limit);

        if query.trim().is_empty() {
            return ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                output: ToolOutput::error("query_required"),
            };
        }

        let matches = self.tools.search(query, requested_limit);
        for definition in &matches {
            active.insert(definition.name.clone());
        }

        ToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            output: ToolOutput::structured(json!({
                "loaded": matches
                    .iter()
                    .map(|definition| json!({
                        "name": definition.name,
                        "title": definition.title,
                        "description": definition.description,
                        "category": definition.discovery.category,
                    }))
                    .collect::<Vec<_>>()
            })),
        }
    }
}

fn search_tools_definition() -> ToolDefinition {
    ToolDefinition {
        name: SEARCH_TOOLS_NAME.into(),
        title: Some("Search tools".into()),
        description: "Search Sujiu's deferred local tool catalog. Use this only when the currently visible tools do not cover a capability you need. Matching tool schemas become available on the next model turn.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Describe the capability or information source you need."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 8,
                    "default": 5
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
        output_schema: Some(json!({
            "type":"object",
            "properties":{
                "loaded":{
                    "type":"array",
                    "items":{
                        "type":"object",
                        "properties":{
                            "name":{"type":"string"},
                            "title":{"type":["string","null"]},
                            "description":{"type":"string"},
                            "category":{"type":"string"}
                        },
                        "required":["name","description","category"]
                    }
                }
            },
            "required":["loaded"]
        })),
        annotations: ToolAnnotations {
            title: Some("Search tools".into()),
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        },
        discovery: ToolDiscovery {
            category: "system".into(),
            keywords: Vec::new(),
            always_available: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;

    use super::*;
    use crate::{
        provider::AiProvider,
        tool::{Tool, ToolError},
        types::AssistantTurn,
    };

    struct LoreTool;

    #[async_trait]
    impl Tool for LoreTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "search_lore".into(),
                title: Some("Search lore".into()),
                description: "Search detailed world lore.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"query":{"type":"string"}},
                    "required": ["query"]
                }),
                output_schema: Some(json!({"type":"object"})),
                annotations: ToolAnnotations {
                    title: Some("Search lore".into()),
                    read_only_hint: true,
                    destructive_hint: false,
                    idempotent_hint: true,
                    open_world_hint: false,
                },
                discovery: ToolDiscovery {
                    category: "context".into(),
                    keywords: vec!["lore".into(), "history".into(), "kingdom".into()],
                    always_available: false,
                },
            }
        }

        async fn execute(&self, arguments: Value) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::structured(json!({
                "query": arguments.get("query"),
                "result": "The old king vanished beneath the western tower."
            })))
        }
    }

    struct MockProvider {
        turns: Mutex<VecDeque<AssistantTurn>>,
        requests: Arc<Mutex<Vec<ProviderRequest>>>,
    }

    #[async_trait]
    impl AiProvider for MockProvider {
        async fn complete(&self, request: ProviderRequest) -> Result<AssistantTurn, ProviderError> {
            self.requests.lock().unwrap().push(request);
            self.turns
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::InvalidResponse("mock exhausted".into()))
        }
    }

    #[tokio::test]
    async fn model_can_discover_a_deferred_tool_then_call_it() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let provider = MockProvider {
            requests: requests.clone(),
            turns: Mutex::new(VecDeque::from([
                AssistantTurn {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "search-1".into(),
                        name: SEARCH_TOOLS_NAME.into(),
                        arguments: json!({"query":"kingdom history"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    response_id: None,
                },
                AssistantTurn {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "lore-1".into(),
                        name: "search_lore".into(),
                        arguments: json!({"query":"old king"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    response_id: None,
                },
                AssistantTurn {
                    text: Some("The old king vanished beneath the western tower.".into()),
                    tool_calls: Vec::new(),
                    finish_reason: Some("stop".into()),
                    response_id: None,
                },
            ])),
        };

        let mut tools = ToolRegistry::new();
        tools.register(LoreTool);

        let runtime = AgentRuntime::new(
            &provider,
            &tools,
            AgentConfig {
                initial_tool_limit: 0,
                ..AgentConfig::default()
            },
        );

        let outcome = runtime
            .run(
                vec![ModelMessage::Text {
                    role: ModelRole::User,
                    content: "What happened to the old king?".into(),
                }],
                "unrelated initial selector text",
            )
            .await
            .unwrap();

        assert_eq!(
            outcome.final_text,
            "The old king vanished beneath the western tower."
        );

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == SEARCH_TOOLS_NAME));
        assert!(!requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "search_lore"));
        assert!(requests[1]
            .tools
            .iter()
            .any(|tool| tool.name == "search_lore"));
    }
}
