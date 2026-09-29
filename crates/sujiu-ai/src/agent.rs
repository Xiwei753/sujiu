use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde_json::{json, Value};
use sujiu_core::PromptPlan;
use thiserror::Error;

use crate::{
    provider::{AiProvider, NullStreamSink, ProviderError, StreamSink},
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

    #[error("turn cancelled")]
    Cancelled,
}

/// Cooperative cancellation for an in-flight turn.
///
/// A turn is cancelled from another thread, so cancellation is observed at
/// safe points: between provider rounds, between tool calls and between
/// streaming deltas. It never interrupts a tool or a request in progress.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug)]
pub struct AgentOutcome {
    pub final_text: String,
    pub rounds: usize,
    pub transcript: Vec<ModelMessage>,
    pub tool_results: Vec<ToolResult>,
}

pub struct AgentRuntime<'a> {
    provider: &'a dyn AiProvider,
    tools: &'a ToolRegistry,
    config: AgentConfig,
}

impl<'a> AgentRuntime<'a> {
    pub fn new(provider: &'a dyn AiProvider, tools: &'a ToolRegistry, config: AgentConfig) -> Self {
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
        messages: Vec<ModelMessage>,
        discovery_query: &str,
    ) -> Result<AgentOutcome, AgentError> {
        self.run_streaming(messages, discovery_query, &mut NullStreamSink)
            .await
    }

    /// Run one conversation turn, reporting progress to `sink`.
    ///
    /// This is the single agent loop. Non-streaming callers get the same
    /// semantics through `run`, which simply discards the events.
    pub async fn run_streaming(
        &self,
        mut messages: Vec<ModelMessage>,
        discovery_query: &str,
        sink: &mut dyn StreamSink,
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

            // A provider reports a cancelled turn as an error rather than
            // completing it, so a stop the user asked for never looks like a
            // finished answer.
            let turn = self
                .provider
                .stream(
                    ProviderRequest {
                        messages: messages.clone(),
                        tools: definitions,
                    },
                    sink,
                )
                .await
                .map_err(|error| match error {
                    ProviderError::Cancelled => AgentError::Cancelled,
                    other => AgentError::Provider(other),
                })?;

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
                if !sink.should_continue() {
                    return Err(AgentError::Cancelled);
                }

                sink.on_tool_call_started(&call);

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
                sink.on_tool_call_finished(&call, &result);
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

        let matches = self
            .tools
            .search(query, self.tools.len())
            .into_iter()
            .filter(|definition| !active.contains(&definition.name))
            .take(requested_limit)
            .collect::<Vec<_>>();

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

    /// A provider that only knows how to complete. The default `stream` body
    /// has to report cancellation on its own, otherwise stopping a
    /// non-streaming provider would look like a finished turn.
    struct CompleteOnlyProvider {
        text: String,
    }

    #[async_trait]
    impl AiProvider for CompleteOnlyProvider {
        async fn complete(
            &self,
            _request: ProviderRequest,
        ) -> Result<AssistantTurn, ProviderError> {
            Ok(AssistantTurn {
                text: Some(self.text.clone()),
                tool_calls: Vec::new(),
                ..AssistantTurn::default()
            })
        }
    }

    #[tokio::test]
    async fn a_non_streaming_provider_reports_cancellation() {
        let provider = CompleteOnlyProvider {
            text: "the console is awake".to_string(),
        };
        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());

        // A sink that has already been told to stop.
        let mut sink = RecordingSink {
            stop_after_deltas: Some(0),
            ..RecordingSink::default()
        };

        let error = runtime
            .run_streaming(user_message("hi"), "hello", &mut sink)
            .await
            .expect_err("a stopped sink must not complete the turn");
        assert!(matches!(error, AgentError::Cancelled), "got {error:?}");
        assert_eq!(sink.deltas.len(), 1, "the answer is still reported once");
    }

    /// Emits deltas for the whole turn text, then the completed turn.
    struct StreamingMockProvider {
        turns: Mutex<VecDeque<(Vec<String>, AssistantTurn)>>,
    }

    #[async_trait]
    impl AiProvider for StreamingMockProvider {
        async fn complete(&self, request: ProviderRequest) -> Result<AssistantTurn, ProviderError> {
            self.stream(request, &mut NullStreamSink).await
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
            sink: &mut dyn crate::provider::StreamSink,
        ) -> Result<AssistantTurn, ProviderError> {
            let (chunks, turn) = self
                .turns
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::InvalidResponse("mock exhausted".into()))?;

            for chunk in chunks {
                if !sink.should_continue() {
                    break;
                }
                sink.on_text_delta(&chunk);
            }

            Ok(turn)
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        deltas: Vec<String>,
        reasoning: Vec<String>,
        requested: Vec<String>,
        started: Vec<String>,
        finished: Vec<(String, bool)>,
        stop_after_deltas: Option<usize>,
    }

    impl StreamSink for RecordingSink {
        fn on_text_delta(&mut self, delta: &str) {
            self.deltas.push(delta.to_owned());
        }

        fn on_reasoning_delta(&mut self, delta: &str) {
            self.reasoning.push(delta.to_owned());
        }

        fn on_tool_call_requested(&mut self, call: &ToolCall) {
            self.requested.push(call.name.clone());
        }

        fn on_tool_call_started(&mut self, call: &ToolCall) {
            self.started.push(call.name.clone());
        }

        fn on_tool_call_finished(&mut self, call: &ToolCall, result: &ToolResult) {
            self.finished
                .push((call.name.clone(), result.output.is_error));
        }

        fn should_continue(&self) -> bool {
            match self.stop_after_deltas {
                Some(limit) => self.deltas.len() < limit,
                None => true,
            }
        }
    }

    fn user_message(content: &str) -> Vec<ModelMessage> {
        vec![ModelMessage::Text {
            role: ModelRole::User,
            content: content.into(),
        }]
    }

    #[tokio::test]
    async fn streaming_reports_text_deltas_in_order() {
        let provider = StreamingMockProvider {
            turns: Mutex::new(VecDeque::from([(
                vec!["Hel".into(), "lo there".into()],
                AssistantTurn {
                    text: Some("Hello there".into()),
                    tool_calls: Vec::new(),
                    finish_reason: Some("stop".into()),
                    response_id: None,
                },
            )])),
        };

        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        let outcome = runtime
            .run_streaming(user_message("hi"), "hi", &mut sink)
            .await
            .unwrap();

        assert_eq!(sink.deltas, vec!["Hel".to_string(), "lo there".to_string()]);
        assert_eq!(outcome.final_text, "Hello there");
    }

    #[tokio::test]
    async fn tool_calls_are_reported_to_the_sink_in_lifecycle_order() {
        let provider = MockProvider {
            requests: Arc::new(Mutex::new(Vec::new())),
            turns: Mutex::new(VecDeque::from([
                AssistantTurn {
                    text: Some("looking it up".into()),
                    tool_calls: vec![ToolCall {
                        id: "search-1".into(),
                        name: "search_context".into(),
                        arguments: json!({"query": "western tower"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    response_id: None,
                },
                AssistantTurn {
                    text: Some("The old king vanished.".into()),
                    tool_calls: Vec::new(),
                    finish_reason: Some("stop".into()),
                    response_id: None,
                },
            ])),
        };

        let store = Arc::new(crate::context::InMemoryContextStore::new());
        let mut tools = ToolRegistry::new();
        crate::context::register_standard_context_tools(&mut tools, store);

        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        runtime
            .run_streaming(user_message("what happened?"), "what happened?", &mut sink)
            .await
            .unwrap();

        assert_eq!(sink.started, vec!["search_context".to_string()]);
        assert_eq!(sink.finished, vec![("search_context".to_string(), false)]);
    }

    #[tokio::test]
    async fn a_text_only_stream_never_announces_a_tool_request() {
        let mut sink = RecordingSink::default();
        let provider = StreamingMockProvider {
            turns: Mutex::new(VecDeque::from([(
                vec!["checking".to_string()],
                AssistantTurn {
                    text: Some("checking".into()),
                    tool_calls: Vec::new(),
                    finish_reason: Some("stop".into()),
                    response_id: None,
                },
            )])),
        };
        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());

        let _ = runtime
            .run_streaming(user_message("hi"), "hi", &mut sink)
            .await;

        // The mock streams text only, so no request is announced and the loop
        // has no tool call to run. The announce path is covered by the
        // openai_compat stream tests.
        assert!(sink.requested.is_empty());
        assert!(sink.started.is_empty());
        assert_eq!(sink.deltas, vec!["checking".to_string()]);
    }

    #[tokio::test]
    async fn a_sink_that_stops_continuing_cancels_the_turn() {
        let provider = StreamingMockProvider {
            turns: Mutex::new(VecDeque::from([(
                vec!["first".into(), "second".into()],
                AssistantTurn {
                    text: Some("firstsecond".into()),
                    tool_calls: Vec::new(),
                    finish_reason: Some("stop".into()),
                    response_id: None,
                },
            )])),
        };

        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink {
            stop_after_deltas: Some(0),
            ..RecordingSink::default()
        };

        let outcome = runtime
            .run_streaming(user_message("hi"), "hi", &mut sink)
            .await
            .unwrap();

        assert!(sink.deltas.is_empty());
        assert_eq!(outcome.final_text, "firstsecond");
    }

    #[tokio::test]
    async fn cancelling_before_a_tool_call_stops_the_turn() {
        let provider = MockProvider {
            requests: Arc::new(Mutex::new(Vec::new())),
            turns: Mutex::new(VecDeque::from([AssistantTurn {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "search-1".into(),
                    name: "search_context".into(),
                    arguments: json!({"query": "anything"}),
                }],
                finish_reason: Some("tool_calls".into()),
                response_id: None,
            }])),
        };

        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink {
            stop_after_deltas: Some(0),
            ..RecordingSink::default()
        };

        let error = runtime
            .run_streaming(user_message("hi"), "hi", &mut sink)
            .await
            .expect_err("turn must stop before executing tools");

        assert!(matches!(error, AgentError::Cancelled));
        assert!(
            sink.started.is_empty(),
            "no tool may run after cancellation"
        );
    }

    #[tokio::test]
    async fn cancel_token_is_observable_across_threads() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());

        let worker = token.clone();
        std::thread::spawn(move || worker.cancel()).join().unwrap();

        assert!(token.is_cancelled());
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
