use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sujiu_core::{ContextKind, ContextRecord, ContextScope, ContextSource, WorldBook};
use thiserror::Error;

use crate::{
    tool::{Tool, ToolError, ToolRegistry},
    types::{ToolAnnotations, ToolContent, ToolDefinition, ToolDiscovery, ToolOutput},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ContextSearchQuery {
    pub query: String,
    #[serde(default)]
    pub kinds: Vec<ContextKind>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_ms: Option<i64>,
}

impl Default for ContextSearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            kinds: Vec::new(),
            source_ids: Vec::new(),
            limit: default_search_limit(),
            after_ms: None,
            before_ms: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ContextHit {
    pub uri: String,
    pub source_id: String,
    pub kind: ContextKind,
    pub title: String,
    pub snippet: String,
    pub score: i32,
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ContextStoreError {
    #[error("context store failed: {0}")]
    Store(String),
}

#[async_trait]
pub trait ContextStore: Send + Sync {
    async fn list_sources(
        &self,
        kinds: &[ContextKind],
    ) -> Result<Vec<ContextSource>, ContextStoreError>;

    async fn search(
        &self,
        query: &ContextSearchQuery,
    ) -> Result<Vec<ContextHit>, ContextStoreError>;

    async fn read(&self, uris: &[String]) -> Result<Vec<ContextRecord>, ContextStoreError>;
}

#[derive(Default)]
pub struct InMemoryContextStore {
    sources: RwLock<BTreeMap<String, ContextSource>>,
    records: RwLock<BTreeMap<String, ContextRecord>>,
}

impl InMemoryContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_source(&self, source: ContextSource) {
        self.sources
            .write()
            .expect("context source lock poisoned")
            .insert(source.id.clone(), source);
    }

    pub fn add_record(&self, record: ContextRecord) {
        let source_id = record.source_id.clone();

        self.records
            .write()
            .expect("context record lock poisoned")
            .insert(record.uri.clone(), record);

        if let Some(source) = self
            .sources
            .write()
            .expect("context source lock poisoned")
            .get_mut(&source_id)
        {
            source.record_count = self
                .records
                .read()
                .expect("context record lock poisoned")
                .values()
                .filter(|record| record.source_id == source_id)
                .count();
        }
    }

    pub fn add_world_book(
        &self,
        source_id: impl Into<String>,
        name: impl Into<String>,
        book: &WorldBook,
        scope: ContextScope,
    ) {
        let source_id = source_id.into();
        let source_name = name.into();

        self.add_source(ContextSource {
            id: source_id.clone(),
            kind: ContextKind::WorldLore,
            name: source_name,
            description: "Character/world lore imported from a world book.".into(),
            scope: scope.clone(),
            mutable: false,
            record_count: 0,
            metadata: Default::default(),
        });

        for entry in book.entries.iter().filter(|entry| entry.enabled) {
            self.add_record(ContextRecord {
                uri: format!("sujiu://context/{}/{}", source_id, entry.id),
                source_id: source_id.clone(),
                kind: ContextKind::WorldLore,
                title: entry.name.clone(),
                content: entry.content.clone(),
                keywords: entry.keys.clone(),
                tags: Vec::new(),
                priority: entry.priority,
                timestamp_ms: None,
                scope: scope.clone(),
                metadata: entry.extensions.clone(),
            });
        }
    }
}

#[async_trait]
impl ContextStore for InMemoryContextStore {
    async fn list_sources(
        &self,
        kinds: &[ContextKind],
    ) -> Result<Vec<ContextSource>, ContextStoreError> {
        let mut sources = self
            .sources
            .read()
            .map_err(|error| ContextStoreError::Store(error.to_string()))?
            .values()
            .filter(|source| kinds.is_empty() || kinds.contains(&source.kind))
            .cloned()
            .collect::<Vec<_>>();

        sources.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(sources)
    }

    async fn search(
        &self,
        request: &ContextSearchQuery,
    ) -> Result<Vec<ContextHit>, ContextStoreError> {
        let query = request.query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let terms = query
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();

        let mut scored = self
            .records
            .read()
            .map_err(|error| ContextStoreError::Store(error.to_string()))?
            .values()
            .filter(|record| {
                (request.kinds.is_empty() || request.kinds.contains(&record.kind))
                    && (request.source_ids.is_empty()
                        || request.source_ids.contains(&record.source_id))
                    && request
                        .after_ms
                        .is_none_or(|after| record.timestamp_ms.is_some_and(|ts| ts >= after))
                    && request
                        .before_ms
                        .is_none_or(|before| record.timestamp_ms.is_some_and(|ts| ts <= before))
            })
            .filter_map(|record| {
                let title = record.title.to_lowercase();
                let content = record.content.to_lowercase();
                let keywords = record
                    .keywords
                    .iter()
                    .map(|keyword| keyword.to_lowercase())
                    .collect::<Vec<_>>();
                let tags = record
                    .tags
                    .iter()
                    .map(|tag| tag.to_lowercase())
                    .collect::<Vec<_>>();

                let mut score = record.priority;

                if !title.is_empty() && (title.contains(&query) || query.contains(&title)) {
                    score += 100;
                }
                if content.contains(&query) {
                    score += 40;
                }
                if keywords
                    .iter()
                    .any(|keyword| query.contains(keyword) || keyword.contains(&query))
                {
                    score += 120;
                }
                if tags
                    .iter()
                    .any(|tag| query.contains(tag) || tag.contains(&query))
                {
                    score += 70;
                }

                for term in &terms {
                    if title.contains(*term) {
                        score += 25;
                    }
                    if content.contains(*term) {
                        score += 5;
                    }
                    if keywords.iter().any(|keyword| keyword.contains(*term)) {
                        score += 35;
                    }
                    if tags.iter().any(|tag| tag.contains(*term)) {
                        score += 20;
                    }
                }

                if score <= record.priority {
                    None
                } else {
                    Some(ContextHit {
                        uri: record.uri.clone(),
                        source_id: record.source_id.clone(),
                        kind: record.kind,
                        title: record.title.clone(),
                        snippet: make_snippet(&record.content, 280),
                        score,
                        priority: record.priority,
                        timestamp_ms: record.timestamp_ms,
                        tags: record.tags.clone(),
                    })
                }
            })
            .collect::<Vec<_>>();

        scored.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.priority.cmp(&left.priority))
                .then_with(|| right.timestamp_ms.cmp(&left.timestamp_ms))
                .then_with(|| left.uri.cmp(&right.uri))
        });

        scored.truncate(request.limit.clamp(1, 20));
        Ok(scored)
    }

    async fn read(&self, uris: &[String]) -> Result<Vec<ContextRecord>, ContextStoreError> {
        let records = self
            .records
            .read()
            .map_err(|error| ContextStoreError::Store(error.to_string()))?;

        Ok(uris
            .iter()
            .filter_map(|uri| records.get(uri).cloned())
            .collect())
    }
}

pub fn register_standard_context_tools(registry: &mut ToolRegistry, store: Arc<dyn ContextStore>) {
    registry.register(ListContextSourcesTool::new(store.clone()));
    registry.register(SearchContextTool::new(store.clone()));
    registry.register(ReadContextTool::new(store));
}

pub struct ListContextSourcesTool {
    store: Arc<dyn ContextStore>,
}

impl ListContextSourcesTool {
    pub fn new(store: Arc<dyn ContextStore>) -> Self {
        Self { store }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ListSourcesArgs {
    #[serde(default)]
    kinds: Vec<ContextKind>,
}

#[async_trait]
impl Tool for ListContextSourcesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_context_sources".into(),
            title: Some("List context sources".into()),
            description: "List the lore, story-history, long-term-memory, chat-history, persona, note, and other context sources currently available to this conversation. Use this to discover where information can be searched; it returns metadata only, not the full source contents.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kinds": {
                        "type": "array",
                        "items": {"type":"string","enum": context_kind_names()},
                        "description": "Optional source kinds to include."
                    }
                },
                "additionalProperties": false
            }),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "sources": {
                        "type":"array",
                        "items": {
                            "type":"object",
                            "properties": {
                                "id":{"type":"string"},
                                "kind":{"type":"string"},
                                "name":{"type":"string"},
                                "description":{"type":"string"},
                                "mutable":{"type":"boolean"},
                                "record_count":{"type":"integer"}
                            },
                            "required":["id","kind","name","description","mutable","record_count"]
                        }
                    }
                },
                "required":["sources"]
            })),
            annotations: read_only_annotations("List context sources"),
            discovery: context_discovery(vec!["sources", "available context", "what can i search"]),
        }
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput, ToolError> {
        let arguments: ListSourcesArgs = parse_arguments(arguments)?;
        let sources = self
            .store
            .list_sources(&arguments.kinds)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;

        Ok(ToolOutput::structured(json!({"sources": sources})))
    }
}

pub struct SearchContextTool {
    store: Arc<dyn ContextStore>,
}

impl SearchContextTool {
    pub fn new(store: Arc<dyn ContextStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for SearchContextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_context".into(),
            title: Some("Search context".into()),
            description: "Search relevant roleplay context without loading whole databases. Searches the same standard interface across world lore, past plot/story events, character long-term memory, previous chat history, persona facts, notes, and future context-source types. Returns short ranked hits with stable URIs; call read_context only for hits whose full contents are needed.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type":"string",
                        "description":"Natural-language description, names, facts, events, or keywords to retrieve."
                    },
                    "kinds": {
                        "type":"array",
                        "items":{"type":"string","enum":context_kind_names()},
                        "description":"Optional context kinds to search."
                    },
                    "source_ids": {
                        "type":"array",
                        "items":{"type":"string"},
                        "description":"Optional source IDs returned by list_context_sources."
                    },
                    "limit": {
                        "type":"integer",
                        "minimum":1,
                        "maximum":20,
                        "default":5
                    },
                    "after_ms": {
                        "type":"integer",
                        "description":"Optional inclusive lower timestamp bound for historical records."
                    },
                    "before_ms": {
                        "type":"integer",
                        "description":"Optional inclusive upper timestamp bound for historical records."
                    }
                },
                "required":["query"],
                "additionalProperties":false
            }),
            output_schema: Some(json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "hits":{
                        "type":"array",
                        "items":{
                            "type":"object",
                            "properties":{
                                "uri":{"type":"string"},
                                "source_id":{"type":"string"},
                                "kind":{"type":"string"},
                                "title":{"type":"string"},
                                "snippet":{"type":"string"},
                                "score":{"type":"integer"},
                                "priority":{"type":"integer"},
                                "timestamp_ms":{"type":["integer","null"]},
                                "tags":{"type":"array","items":{"type":"string"}}
                            },
                            "required":["uri","source_id","kind","title","snippet","score","priority","tags"]
                        }
                    }
                },
                "required":["query","hits"]
            })),
            annotations: read_only_annotations("Search context"),
            discovery: context_discovery(vec![
                "lore",
                "world",
                "setting",
                "plot",
                "story",
                "event",
                "history",
                "memory",
                "chat",
                "persona",
                "note",
                "世界",
                "设定",
                "剧情",
                "事件",
                "历史",
                "记忆",
                "聊天",
                "人物",
            ]),
        }
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput, ToolError> {
        let mut request: ContextSearchQuery = parse_arguments(arguments)?;
        request.query = request.query.trim().to_owned();

        if request.query.is_empty() {
            return Err(ToolError::InvalidArguments("query is required".into()));
        }

        request.limit = request.limit.clamp(1, 20);

        let hits = self
            .store
            .search(&request)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;

        Ok(ToolOutput::structured(json!({
            "query": request.query,
            "hits": hits,
        })))
    }
}

pub struct ReadContextTool {
    store: Arc<dyn ContextStore>,
}

impl ReadContextTool {
    pub fn new(store: Arc<dyn ContextStore>) -> Self {
        Self { store }
    }
}

#[derive(Debug, Deserialize)]
struct ReadContextArgs {
    uris: Vec<String>,
}

#[async_trait]
impl Tool for ReadContextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_context".into(),
            title: Some("Read context".into()),
            description: "Read the full contents of specific context records selected from search_context. Prefer search_context first and read only the few relevant URIs; do not use this as a way to dump an entire lorebook or chat history.".into(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "uris":{
                        "type":"array",
                        "minItems":1,
                        "maxItems":8,
                        "items":{"type":"string"},
                        "description":"Stable context URIs returned by search_context."
                    }
                },
                "required":["uris"],
                "additionalProperties":false
            }),
            output_schema: Some(json!({
                "type":"object",
                "properties":{
                    "records":{
                        "type":"array",
                        "items":{
                            "type":"object",
                            "properties":{
                                "uri":{"type":"string"},
                                "source_id":{"type":"string"},
                                "kind":{"type":"string"},
                                "title":{"type":"string"},
                                "content":{"type":"string"},
                                "keywords":{"type":"array","items":{"type":"string"}},
                                "tags":{"type":"array","items":{"type":"string"}},
                                "priority":{"type":"integer"},
                                "timestamp_ms":{"type":["integer","null"]}
                            },
                            "required":["uri","source_id","kind","title","content","keywords","tags","priority"]
                        }
                    }
                },
                "required":["records"]
            })),
            annotations: read_only_annotations("Read context"),
            discovery: context_discovery(vec!["read context", "open result", "full lore", "full memory"]),
        }
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput, ToolError> {
        let arguments: ReadContextArgs = parse_arguments(arguments)?;

        if arguments.uris.is_empty() {
            return Err(ToolError::InvalidArguments("uris must not be empty".into()));
        }
        if arguments.uris.len() > 8 {
            return Err(ToolError::InvalidArguments(
                "at most 8 context records can be read per call".into(),
            ));
        }

        let records = self
            .store
            .read(&arguments.uris)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;

        let structured = json!({"records": records});
        let content = records
            .iter()
            .map(|record| ToolContent::Resource {
                uri: record.uri.clone(),
                mime_type: Some("text/plain".into()),
                text: Some(format!(
                    "{}\n\n{}",
                    if record.title.is_empty() {
                        record.uri.as_str()
                    } else {
                        record.title.as_str()
                    },
                    record.content
                )),
            })
            .collect();

        Ok(ToolOutput {
            content,
            structured_content: Some(structured),
            is_error: false,
        })
    }
}

fn parse_arguments<T>(arguments: Value) -> Result<T, ToolError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(arguments)
        .map_err(|error| ToolError::InvalidArguments(error.to_string()))
}

fn read_only_annotations(title: &str) -> ToolAnnotations {
    ToolAnnotations {
        title: Some(title.into()),
        read_only_hint: true,
        destructive_hint: false,
        idempotent_hint: true,
        open_world_hint: false,
    }
}

fn context_discovery(keywords: Vec<&str>) -> ToolDiscovery {
    ToolDiscovery {
        category: "context".into(),
        keywords: keywords.into_iter().map(str::to_owned).collect(),
        always_available: true,
    }
}

fn context_kind_names() -> Vec<&'static str> {
    vec![
        "world_lore",
        "story_event",
        "character_memory",
        "chat_history",
        "persona",
        "note",
        "other",
    ]
}

fn default_search_limit() -> usize {
    5
}

fn make_snippet(content: &str, max_chars: usize) -> String {
    let mut chars = content.chars();
    let snippet = chars.by_ref().take(max_chars).collect::<String>();

    if chars.next().is_some() {
        format!("{snippet}…")
    } else {
        snippet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sujiu_core::{WorldBookEntry, WorldBookPosition};

    #[tokio::test]
    async fn one_search_protocol_covers_lore_and_story_events() {
        let store = InMemoryContextStore::new();

        store.add_world_book(
            "world",
            "Main lorebook",
            &WorldBook {
                name: "world".into(),
                entries: vec![WorldBookEntry {
                    id: "tower".into(),
                    name: "黑塔".into(),
                    content: "黑塔位于王都北面，普通人无法进入。".into(),
                    keys: vec!["黑塔".into()],
                    enabled: true,
                    constant: false,
                    priority: 10,
                    position: WorldBookPosition::AfterCharacter,
                    extensions: Default::default(),
                }],
                extensions: Default::default(),
            },
            ContextScope::default(),
        );

        store.add_source(ContextSource {
            id: "plot".into(),
            kind: ContextKind::StoryEvent,
            name: "Story timeline".into(),
            description: "Important events from this roleplay.".into(),
            scope: ContextScope {
                session_id: Some("session-1".into()),
                ..ContextScope::default()
            },
            mutable: true,
            record_count: 0,
            metadata: Default::default(),
        });

        store.add_record(ContextRecord {
            uri: "sujiu://context/plot/event-1".into(),
            source_id: "plot".into(),
            kind: ContextKind::StoryEvent,
            title: "旧王失踪".into(),
            content: "旧王最后一次被看见是在黑塔地下入口。".into(),
            keywords: vec!["旧王".into(), "失踪".into(), "黑塔".into()],
            tags: vec!["main_plot".into()],
            priority: 20,
            timestamp_ms: Some(1000),
            scope: ContextScope {
                session_id: Some("session-1".into()),
                ..ContextScope::default()
            },
            metadata: Default::default(),
        });

        let lore = store
            .search(&ContextSearchQuery {
                query: "黑塔在哪里".into(),
                kinds: vec![ContextKind::WorldLore],
                ..ContextSearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(lore.len(), 1);
        assert_eq!(lore[0].kind, ContextKind::WorldLore);

        let plot = store
            .search(&ContextSearchQuery {
                query: "旧王失踪".into(),
                kinds: vec![ContextKind::StoryEvent],
                ..ContextSearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(plot.len(), 1);
        assert_eq!(plot[0].kind, ContextKind::StoryEvent);
    }

    #[tokio::test]
    async fn search_returns_snippets_and_read_returns_full_records() {
        let store = InMemoryContextStore::new();
        store.add_source(ContextSource {
            id: "memory".into(),
            kind: ContextKind::CharacterMemory,
            name: "Character memory".into(),
            description: String::new(),
            scope: ContextScope::default(),
            mutable: true,
            record_count: 0,
            metadata: Default::default(),
        });
        store.add_record(ContextRecord {
            uri: "sujiu://context/memory/m1".into(),
            source_id: "memory".into(),
            kind: ContextKind::CharacterMemory,
            title: "约定".into(),
            content: "她答应雨停之后一起去旧城门。".into(),
            keywords: vec!["约定".into(), "旧城门".into()],
            tags: Vec::new(),
            priority: 0,
            timestamp_ms: None,
            scope: ContextScope::default(),
            metadata: Default::default(),
        });

        let hits = store
            .search(&ContextSearchQuery {
                query: "旧城门".into(),
                ..ContextSearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);

        let records = store.read(&[hits[0].uri.clone()]).await.unwrap();
        assert_eq!(records[0].content, "她答应雨停之后一起去旧城门。");
    }
}
