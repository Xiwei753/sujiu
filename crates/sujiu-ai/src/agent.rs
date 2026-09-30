use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde_json::{json, Value};
use sujiu_core::{
    AssistantStep, ModelMessage, ModelRole, PromptPlan, ProviderContinuation, ProviderRequest,
    ToolCall, ToolCallRecord, ToolCallState, ToolDefinition, ToolOutput, ToolResult,
    ToolResultRecord, Turn, TurnState,
};

use crate::{
    provider::{AiProvider, NullStreamSink, ProviderError, StreamSink},
    tool::ToolRegistry,
    types::{messages_from_prompt_plan, ToolAnnotations, ToolDiscovery},
};

const SEARCH_TOOLS_NAME: &str = "sujiu_search_tools";

#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub max_rounds: usize,
    pub initial_tool_limit: usize,
    pub tool_search_limit: usize,
    pub enable_tool_search: bool,
    /// The participant every step of this turn speaks for.
    ///
    /// A conversation can hold several characters, so "assistant" alone does not
    /// say who answered. The caller sets this when there is exactly one
    /// attribution it can justify, and leaves it `None` when it cannot: which
    /// of several characters replies next is a speaking-order policy, and the
    /// loop does not invent one.
    ///
    /// It is set here, on the step as it is born, rather than afterwards on the
    /// stored turn. A step attributed after it was already sent would change the
    /// wire shape of history the provider has seen, which breaks the prompt
    /// cache on the next request for a fact that was never in doubt.
    pub speaker: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_rounds: 8,
            initial_tool_limit: 8,
            tool_search_limit: 8,
            enable_tool_search: true,
            speaker: None,
        }
    }
}

/// Why a turn stopped.
///
/// Stopping is not the same as failing. A cancelled turn still produced steps
/// the model saw, and those steps belong in the transcript, so a stop is
/// reported as a state rather than as an error that discards the work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentStop {
    /// The assistant produced a final answer.
    Completed,
    /// The turn was cancelled while a tool call was still open.
    Cancelled,
    /// The tool loop hit its round limit.
    MaxRounds(usize),
    /// A round could not be completed: the transport failed, the provider
    /// returned an error status, or the response could not be parsed.
    ///
    /// The message is reported to the platform separately. It is not a reason
    /// to throw away the rounds that already succeeded, because those rounds
    /// contain tool calls whose results the next request has to repeat.
    Failed(String),
}

impl AgentStop {
    pub fn turn_state(&self) -> TurnState {
        match self {
            Self::Completed => TurnState::Completed,
            Self::Cancelled => TurnState::Cancelled,
            Self::MaxRounds(_) | Self::Failed(_) => TurnState::Failed,
        }
    }

    /// Whether a platform should treat the stop as a failure to report.
    pub fn is_error(&self) -> bool {
        !matches!(self, Self::Completed)
    }
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

/// What one turn produced.
///
/// `turn` holds every step the model took, not only the last one, so a caller
/// that stores it keeps the tool calls and their results that a later request
/// has to repeat.
#[derive(Clone, Debug)]
pub struct AgentOutcome {
    pub final_text: String,
    pub rounds: usize,
    /// The steps this turn produced. The id is left to the session that stores
    /// it, because only that knows the session's own numbering.
    pub turn: Turn,
    pub tool_results: Vec<ToolResult>,
    pub stop: AgentStop,
}

impl AgentOutcome {
    /// Exactly the messages this turn added, in wire order.
    pub fn messages(&self) -> Vec<ModelMessage> {
        self.turn.model_messages()
    }
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

    pub async fn run_prompt(
        &self,
        plan: &PromptPlan,
        continuation: Option<ProviderContinuation>,
    ) -> AgentOutcome {
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

        self.run(messages, continuation, &discovery_query).await
    }

    pub async fn run(
        &self,
        messages: Vec<ModelMessage>,
        continuation: Option<ProviderContinuation>,
        discovery_query: &str,
    ) -> AgentOutcome {
        self.run_streaming(messages, continuation, discovery_query, &mut NullStreamSink)
            .await
    }

    /// Run one conversation turn, reporting progress to `sink`.
    ///
    /// This is the single agent loop. Non-streaming callers get the same
    /// semantics through `run`, which simply discards the events.
    ///
    /// A turn that stops early still returns what it produced. Dropping the
    /// partial steps would leave a tool call that the model asked for with no
    /// result, which the next request cannot repair without rewriting history
    /// the provider has already seen. A transport failure is therefore a stop
    /// with a message, not an error: the completed rounds are committed and the
    /// reason is reported alongside them.
    pub async fn run_streaming(
        &self,
        mut messages: Vec<ModelMessage>,
        continuation: Option<ProviderContinuation>,
        discovery_query: &str,
        sink: &mut dyn StreamSink,
    ) -> AgentOutcome {
        let mut active = self.tools.always_available_names();

        for definition in self
            .tools
            .search(discovery_query, self.config.initial_tool_limit)
        {
            active.insert(definition.name);
        }

        let mut all_results = Vec::new();
        let mut turn = Turn::new("", last_user_text(&messages));

        // The continuation this turn starts from, and the one the most recent
        // round produced. They are the same on the first round.
        //
        // What happens in between is the provider's call, not a default:
        // `ContinuationUpdate` names whether a round replaced the handle, said
        // nothing, or dropped it, so no round has to guess on its behalf.
        let mut carried = continuation;

        for round in 1..=self.config.max_rounds {
            let definitions = self.request_tool_definitions(&active);

            let mut observed = ObservedSink::new(sink);
            let produced = match self
                .provider
                .stream(
                    ProviderRequest {
                        messages: messages.clone(),
                        tools: definitions,
                        continuation: carried.clone(),
                    },
                    &mut observed,
                )
                .await
            {
                Ok(produced) => produced,
                // The round never finished, so its partial text is not a step.
                // Everything that already completed stays in the transcript,
                // because those rounds contain tool calls whose results the
                // next request has to repeat.
                Err(ProviderError::Cancelled) => {
                    return outcome(turn, all_results, round, AgentStop::Cancelled)
                }
                Err(error) => {
                    return outcome(
                        turn,
                        all_results,
                        round,
                        AgentStop::Failed(error.to_string()),
                    )
                }
            };

            // The next round continues from whatever this round said, which is
            // either a new handle, a dropped one, or no opinion at all. The
            // three cases are named rather than inferred from an empty value.
            carried = produced.continuation.apply(carried);

            let step = AssistantStep {
                text: produced.text,
                // The speaker is decided before the step exists, so the message
                // this round sends and the message the next turn replays are the
                // same bytes.
                speaker: self.config.speaker.clone(),
                reasoning: produced.reasoning,
                // The event, not the handle it produced. A provider that
                // dropped the handle has said so, and that answer has to
                // survive the session boundary or the next turn will find the
                // retired handle again.
                continuation: produced.continuation,
                usage: produced.usage,
                finish_reason: produced.finish_reason,
                ..AssistantStep::default()
            };

            if produced.tool_calls.is_empty() {
                messages.extend(step.model_messages());
                turn.steps.push(step);

                // The answer arrived, but a user who stopped the turn still asked
                // to stop. The step is kept, because the model really did say
                // it, while the stop stays visible to the platform.
                let stop = if sink.should_continue() {
                    AgentStop::Completed
                } else {
                    AgentStop::Cancelled
                };

                return outcome(turn, all_results, round, stop);
            }

            let mut cancelled_from = false;
            let mut records = Vec::with_capacity(produced.tool_calls.len());

            for call in &produced.tool_calls {
                if cancelled_from || !sink.should_continue() {
                    // This call, and every call after it in the same step, never
                    // ran. Each one is stored with an explicit result so the next
                    // request still pairs it with the call the model asked for,
                    // and so a platform can show it as stopped.
                    cancelled_from = true;
                    records.push(self.tool_record(
                        call,
                        ToolResultRecord::unfinished(ToolCallState::Cancelled, &call.name),
                    ));
                    continue;
                }

                sink.on_tool_call_started(call);
                let result = self.execute_call(call, &mut active).await;
                records.push(
                    self.tool_record(call, ToolResultRecord::completed(result.output.clone())),
                );
                sink.on_tool_call_finished(call, &result);
                all_results.push(result);
            }

            let step = AssistantStep {
                tool_calls: records,
                ..step
            };
            messages.extend(step.model_messages());
            turn.steps.push(step);

            if cancelled_from {
                return outcome(turn, all_results, round, AgentStop::Cancelled);
            }
        }

        outcome(
            turn,
            all_results,
            self.config.max_rounds,
            AgentStop::MaxRounds(self.config.max_rounds),
        )
    }

    fn tool_record(&self, call: &ToolCall, result: ToolResultRecord) -> ToolCallRecord {
        ToolCallRecord {
            id: call.id.clone(),
            name: call.name.clone(),
            title: self.tool_title(&call.name),
            arguments: call.arguments.clone(),
            result,
        }
    }

    /// The tool's own title, passed through. Deriving one from the tool name
    /// would discard the display name the tool published.
    fn tool_title(&self, name: &str) -> Option<String> {
        if name == SEARCH_TOOLS_NAME {
            return search_tools_definition().title;
        }

        self.tools.title_for(name)
    }

    /// Run one tool call, or explain why it cannot run.
    ///
    /// A tool that was not loaded for this turn still produces a result, so the
    /// model learns why instead of seeing a call it cannot pair.
    async fn execute_call(&self, call: &ToolCall, active: &mut BTreeSet<String>) -> ToolResult {
        if call.name == SEARCH_TOOLS_NAME {
            return self.execute_tool_search(call, active);
        }

        if active.contains(&call.name) {
            return self
                .tools
                .execute(call.id.clone(), call.name.clone(), call.arguments.clone())
                .await;
        }

        ToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            output: ToolOutput::error(
                "tool_not_exposed: this tool was not loaded for the current turn",
            ),
        }
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

/// The user text this turn answers.
fn last_user_text(messages: &[ModelMessage]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|message| match message {
            ModelMessage::Text {
                role: ModelRole::User,
                content,
            } => Some(content.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Close out a turn, whatever stopped it.
fn outcome(
    mut turn: Turn,
    tool_results: Vec<ToolResult>,
    rounds: usize,
    stop: AgentStop,
) -> AgentOutcome {
    let final_text = turn.final_text().unwrap_or_default().to_owned();
    turn.state = stop.turn_state();

    AgentOutcome {
        final_text,
        rounds,
        turn,
        tool_results,
        stop,
    }
}

/// Forwards events to the caller's sink.
///
/// Reasoning used to be collected here, because a stream only ever delivers
/// it as deltas. It no longer is: the provider that parsed the deltas is the
/// one that knows which provider they came from, so it assembles the sidecar
/// itself and reports it on the turn. Reconstructing the text here would have
/// thrown that provenance away.
struct ObservedSink<'a> {
    inner: &'a mut dyn StreamSink,
}

impl<'a> ObservedSink<'a> {
    fn new(inner: &'a mut dyn StreamSink) -> Self {
        Self { inner }
    }
}

impl StreamSink for ObservedSink<'_> {
    fn on_text_delta(&mut self, delta: &str) {
        self.inner.on_text_delta(delta);
    }

    fn on_reasoning_delta(&mut self, delta: &str) {
        self.inner.on_reasoning_delta(delta);
    }

    fn on_tool_call_requested(&mut self, call: &ToolCall) {
        self.inner.on_tool_call_requested(call);
    }

    fn on_tool_call_started(&mut self, call: &ToolCall) {
        self.inner.on_tool_call_started(call);
    }

    fn on_tool_call_finished(&mut self, call: &ToolCall, result: &ToolResult) {
        self.inner.on_tool_call_finished(call, result);
    }

    fn should_continue(&self) -> bool {
        self.inner.should_continue()
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
        types::{AssistantTurn, ContinuationSupport, ContinuationUpdate, ProviderIdentity},
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

    /// A provider that answers with a chainable continuation, so a transport
    /// that really does resume from a handle can be modelled.
    struct ChainedProvider {
        turns: Mutex<VecDeque<AssistantTurn>>,
        requests: Arc<Mutex<Vec<ProviderRequest>>>,
    }

    #[async_trait]
    impl AiProvider for ChainedProvider {
        async fn complete(&self, request: ProviderRequest) -> Result<AssistantTurn, ProviderError> {
            self.requests.lock().unwrap().push(request);
            self.turns
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::InvalidResponse("mock exhausted".into()))
        }
    }

    fn chainable(identity: ProviderIdentity, response_id: &str) -> ProviderContinuation {
        ProviderContinuation {
            identity,
            support: ContinuationSupport::ResponseId,
            response_id: Some(response_id.into()),
            state: Default::default(),
        }
    }

    /// The next round of a turn must continue from the round before it, not
    /// from whatever the session handed in before the turn began. Replaying a
    /// stale handle is the same class of bug as replaying stale history.
    #[tokio::test]
    async fn each_round_continues_from_the_previous_round() {
        let identity = ProviderIdentity {
            protocol: sujiu_core::Protocol::OpenAiChatCompletions,
            endpoint_id: "primary".into(),
            base_url: "https://api.example.com/v1".into(),
            model: "example-1".into(),
        };
        let session_start = chainable(identity.clone(), "session-0");

        let provider = ChainedProvider {
            requests: Arc::new(Mutex::new(Vec::new())),
            turns: Mutex::new(VecDeque::from([
                AssistantTurn {
                    tool_calls: vec![ToolCall {
                        id: "search-1".into(),
                        name: "search_context".into(),
                        arguments: json!({"query": "western tower"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    continuation: ContinuationUpdate::Replace(chainable(
                        identity.clone(),
                        "round-1",
                    )),
                    ..AssistantTurn::default()
                },
                AssistantTurn {
                    text: Some("The old king vanished.".into()),
                    finish_reason: Some("stop".into()),
                    continuation: ContinuationUpdate::Replace(chainable(
                        identity.clone(),
                        "round-2",
                    )),
                    ..AssistantTurn::default()
                },
            ])),
        };

        let store = Arc::new(crate::context::InMemoryContextStore::new());
        let mut tools = ToolRegistry::new();
        crate::context::register_standard_context_tools(&mut tools, store);

        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        runtime
            .run_streaming(
                user_message("what happened?"),
                Some(session_start),
                "what happened?",
                &mut sink,
            )
            .await;

        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);

        // Round 1 continues the session's handle.
        let first = requests[0]
            .continuation
            .as_ref()
            .expect("the first round resumes the session handle");
        assert_eq!(first.response_id.as_deref(), Some("session-0"));

        // Round 2 continues round 1, not the session's original handle.
        let second = requests[1]
            .continuation
            .as_ref()
            .expect("the second round resumes the first round's handle");
        assert_eq!(second.response_id.as_deref(), Some("round-1"));
    }

    /// A provider that produces no continuation must not invalidate the handle
    /// the session already had. `Unchanged` means "I have nothing to chain",
    /// which is a different statement from "forget what you had".
    #[tokio::test]
    async fn a_round_without_new_state_keeps_the_previous_handle() {
        let identity = ProviderIdentity {
            protocol: sujiu_core::Protocol::OpenAiChatCompletions,
            endpoint_id: "primary".into(),
            base_url: "https://api.example.com/v1".into(),
            model: "example-1".into(),
        };

        let provider = ChainedProvider {
            requests: Arc::new(Mutex::new(Vec::new())),
            turns: Mutex::new(VecDeque::from([
                AssistantTurn {
                    tool_calls: vec![ToolCall {
                        id: "search-1".into(),
                        name: "search_context".into(),
                        arguments: json!({"query": "western tower"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    // `Unchanged`, the default: the provider has no opinion.
                    ..AssistantTurn::default()
                },
                AssistantTurn {
                    text: Some("The old king vanished.".into()),
                    finish_reason: Some("stop".into()),
                    ..AssistantTurn::default()
                },
            ])),
        };

        let store = Arc::new(crate::context::InMemoryContextStore::new());
        let mut tools = ToolRegistry::new();
        crate::context::register_standard_context_tools(&mut tools, store);

        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        runtime
            .run_streaming(
                user_message("what happened?"),
                Some(chainable(identity, "session-0")),
                "what happened?",
                &mut sink,
            )
            .await;

        let requests = provider.requests.lock().unwrap();
        let second = requests[1]
            .continuation
            .as_ref()
            .expect("the handle the session already had is still carried");
        assert_eq!(second.response_id.as_deref(), Some("session-0"));
    }

    /// A provider that says its handle died must not have the dead handle
    /// carried into the next round. Under the old `Option` shape this was
    /// indistinguishable from "nothing to say", so a dead handle was carried.
    #[tokio::test]
    async fn a_provider_can_drop_a_handle_it_no_longer_honours() {
        let identity = ProviderIdentity {
            protocol: sujiu_core::Protocol::OpenAiChatCompletions,
            endpoint_id: "primary".into(),
            base_url: "https://api.example.com/v1".into(),
            model: "example-1".into(),
        };

        let provider = ChainedProvider {
            requests: Arc::new(Mutex::new(Vec::new())),
            turns: Mutex::new(VecDeque::from([
                AssistantTurn {
                    tool_calls: vec![ToolCall {
                        id: "search-1".into(),
                        name: "search_context".into(),
                        arguments: json!({"query": "western tower"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    continuation: ContinuationUpdate::Clear,
                    ..AssistantTurn::default()
                },
                AssistantTurn {
                    text: Some("The old king vanished.".into()),
                    finish_reason: Some("stop".into()),
                    ..AssistantTurn::default()
                },
            ])),
        };

        let store = Arc::new(crate::context::InMemoryContextStore::new());
        let mut tools = ToolRegistry::new();
        crate::context::register_standard_context_tools(&mut tools, store);

        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        let outcome = runtime
            .run_streaming(
                user_message("what happened?"),
                Some(chainable(identity, "session-0")),
                "what happened?",
                &mut sink,
            )
            .await;

        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1].continuation.is_none(),
            "a handle the provider dropped must not be replayed: {:?}",
            requests[1].continuation
        );
        drop(requests);

        // The next user turn reads the transcript, not this request list, so
        // the clear has to be written onto the step too.
        assert!(
            matches!(
                outcome.turn.steps[0].continuation,
                ContinuationUpdate::Clear
            ),
            "the clear must be recorded on the step, or the next turn searches \
             past it and finds the handle that was live before it: {:?}",
            outcome.turn.steps[0].continuation
        );
    }

    /// A turn whose first round produced a tool result and whose second round
    /// hit a provider error must still keep those completed rounds. Throwing
    /// the turn away loses the call and its result, and the next request then
    /// either drops the call or sends it without a matching result.
    #[tokio::test]
    async fn a_provider_error_keeps_the_rounds_that_already_finished() {
        let provider = ChainedProvider {
            requests: Arc::new(Mutex::new(Vec::new())),
            turns: Mutex::new(VecDeque::from([AssistantTurn {
                text: Some("looking it up".into()),
                tool_calls: vec![ToolCall {
                    id: "search-1".into(),
                    name: "search_context".into(),
                    arguments: json!({"query": "western tower"}),
                }],
                finish_reason: Some("tool_calls".into()),
                ..AssistantTurn::default()
            }])),
        };

        let store = Arc::new(crate::context::InMemoryContextStore::new());
        let mut tools = ToolRegistry::new();
        crate::context::register_standard_context_tools(&mut tools, store);

        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        // The mock only has one turn, so the second round fails.
        let outcome = runtime
            .run_streaming(
                user_message("what happened?"),
                None,
                "what happened?",
                &mut sink,
            )
            .await;

        match &outcome.stop {
            AgentStop::Failed(reason) => assert!(!reason.is_empty()),
            other => panic!("expected a failed turn, got {other:?}"),
        }
        assert!(outcome.stop.is_error());
        assert_eq!(outcome.turn.state, TurnState::Failed);

        // The completed round and its tool result are still in the transcript.
        assert_eq!(outcome.turn.steps.len(), 1);
        let call = &outcome.turn.steps[0].tool_calls[0];
        assert_eq!(call.id, "search-1");
        assert_eq!(call.result.state, ToolCallState::Completed);

        let replayed = outcome.messages();
        let results = replayed
            .iter()
            .filter_map(|message| match message {
                ModelMessage::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(results, vec!["search-1"]);
        assert_eq!(outcome.tool_results.len(), 1);
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

        let outcome = runtime
            .run_streaming(user_message("hi"), None, "hello", &mut sink)
            .await;

        assert_eq!(outcome.stop, AgentStop::Cancelled);
        assert_eq!(outcome.turn.state, TurnState::Cancelled);
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
        /// Stop the turn once this many tools have finished, which leaves any
        /// later call in the same step unexecuted.
        stop_after_tools: Option<usize>,
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
            if let Some(limit) = self.stop_after_deltas {
                if self.deltas.len() >= limit {
                    return false;
                }
            }

            match self.stop_after_tools {
                Some(limit) => self.finished.len() < limit,
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
                    ..AssistantTurn::default()
                },
            )])),
        };

        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        let outcome = runtime
            .run_streaming(user_message("hi"), None, "hi", &mut sink)
            .await;

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
                    ..AssistantTurn::default()
                },
                AssistantTurn {
                    text: Some("The old king vanished.".into()),
                    tool_calls: Vec::new(),
                    finish_reason: Some("stop".into()),
                    ..AssistantTurn::default()
                },
            ])),
        };

        let store = Arc::new(crate::context::InMemoryContextStore::new());
        let mut tools = ToolRegistry::new();
        crate::context::register_standard_context_tools(&mut tools, store);

        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink::default();

        runtime
            .run_streaming(
                user_message("what happened?"),
                None,
                "what happened?",
                &mut sink,
            )
            .await;

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
                    ..AssistantTurn::default()
                },
            )])),
        };
        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());

        let _ = runtime
            .run_streaming(user_message("hi"), None, "hi", &mut sink)
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
                    ..AssistantTurn::default()
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
            .run_streaming(user_message("hi"), None, "hi", &mut sink)
            .await;

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
                ..AssistantTurn::default()
            }])),
        };

        let tools = ToolRegistry::new();
        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        let mut sink = RecordingSink {
            stop_after_deltas: Some(0),
            ..RecordingSink::default()
        };

        let outcome = runtime
            .run_streaming(user_message("hi"), None, "hi", &mut sink)
            .await;

        assert_eq!(outcome.stop, AgentStop::Cancelled);
        assert!(
            sink.started.is_empty(),
            "no tool may run after cancellation"
        );

        // The call the model made is still stored, paired with an explicit
        // result. Dropping it would leave the next request with a tool call the
        // provider cannot match, which it answers with a 400.
        let call = &outcome.turn.steps[0].tool_calls[0];
        assert_eq!(call.id, "search-1");
        assert_eq!(call.result.state, ToolCallState::Cancelled);

        let messages = outcome.messages();
        let results = messages
            .iter()
            .filter_map(|message| match message {
                ModelMessage::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(results, vec!["search-1"]);
    }

    #[tokio::test]
    async fn a_cancelled_turn_keeps_the_steps_that_already_finished() {
        let provider = MockProvider {
            requests: Arc::new(Mutex::new(Vec::new())),
            turns: Mutex::new(VecDeque::from([
                AssistantTurn {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "search-1".into(),
                        name: "search_context".into(),
                        arguments: json!({"query": "western tower"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    ..AssistantTurn::default()
                },
                // Two more calls in the next step, which the cancellation below
                // must record rather than erase.
                AssistantTurn {
                    text: Some("found the first one".into()),
                    tool_calls: vec![
                        ToolCall {
                            id: "read-1".into(),
                            name: "read_context".into(),
                            arguments: json!({"uri": "lore://western-tower"}),
                        },
                        ToolCall {
                            id: "read-2".into(),
                            name: "read_context".into(),
                            arguments: json!({"uri": "lore://western-tower/2"}),
                        },
                    ],
                    finish_reason: Some("tool_calls".into()),
                    ..AssistantTurn::default()
                },
            ])),
        };

        let store = Arc::new(crate::context::InMemoryContextStore::new());
        let mut tools = ToolRegistry::new();
        crate::context::register_standard_context_tools(&mut tools, store);

        let runtime = AgentRuntime::new(&provider, &tools, AgentConfig::default());
        // Stop once the first tool has finished, so every call in the second
        // step is left unexecuted.
        let mut sink = RecordingSink {
            stop_after_tools: Some(1),
            ..RecordingSink::default()
        };

        let outcome = runtime
            .run_streaming(
                user_message("what happened?"),
                None,
                "what happened?",
                &mut sink,
            )
            .await;

        assert_eq!(outcome.stop, AgentStop::Cancelled);
        assert_eq!(outcome.turn.steps.len(), 2, "both steps stay in the turn");
        assert_eq!(outcome.turn.steps[0].tool_calls[0].id, "search-1");
        assert_eq!(
            outcome.turn.steps[0].tool_calls[0].result.state,
            ToolCallState::Completed
        );

        // Both calls of the second step are recorded as cancelled, so the next
        // request pairs every call with a result.
        let unexecuted = &outcome.turn.steps[1].tool_calls;
        assert_eq!(unexecuted.len(), 2);
        assert_eq!(unexecuted[0].id, "read-1");
        assert_eq!(unexecuted[1].id, "read-2");
        assert!(unexecuted
            .iter()
            .all(|call| call.result.state == ToolCallState::Cancelled));
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
                    ..AssistantTurn::default()
                },
                AssistantTurn {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "lore-1".into(),
                        name: "search_lore".into(),
                        arguments: json!({"query":"old king"}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    ..AssistantTurn::default()
                },
                AssistantTurn {
                    text: Some("The old king vanished beneath the western tower.".into()),
                    tool_calls: Vec::new(),
                    finish_reason: Some("stop".into()),
                    ..AssistantTurn::default()
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
                None,
                "unrelated initial selector text",
            )
            .await;

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
