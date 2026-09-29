//! napi surface for the shared Rust runtime.
//!
//! This crate is the only place where Rust types are shaped for a JavaScript
//! caller. It stays deliberately thin: the runtime owns conversation
//! semantics, and this layer only translates between the runtime's domain
//! types and the data a platform bridge needs.
//!
//! Turn events cross the boundary as JSON strings so the event vocabulary
//! stays provider neutral and can evolve without breaking the platform side.

use std::sync::Arc;

use napi::bindgen_prelude::{AsyncTask, Result, Task};
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use sujiu_core::ProviderConfig;
use sujiu_ffi::events::{TurnEvent, TurnEventReporter};
use sujiu_ffi::runtime::{
    CharacterSummary, ContextSourceSummary, ConversationSnapshot, ModelSummary, SendTurnRequest,
    SessionSummary, SujiuRuntime, ToolCallSummary,
};

/// A live Rust runtime handed to the platform bridge.
///
/// The platform never sees provider wire formats or tool internals, only
/// domain summaries and normalized turn events.
#[napi]
pub struct SujiuRuntimeBridge {
    runtime: Arc<SujiuRuntime>,
}

#[napi(object)]
pub struct SessionSummaryDto {
    pub id: String,
    pub character_id: String,
    pub character_name: String,
    pub preview: String,
    pub title: String,
    pub updated_at_ms: f64,
    pub message_count: u32,
}

#[napi(object)]
pub struct CharacterSummaryDto {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[napi(object)]
pub struct ModelSummaryDto {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub provider_name: String,
    pub kind: String,
    pub configured: bool,
}

#[napi(object)]
pub struct ContextSourceDto {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub record_count: u32,
}

#[napi(object)]
pub struct ToolCallDto {
    pub id: String,
    pub name: String,
    pub title: String,
    pub status: String,
    pub is_error: bool,
    pub result_text: String,
}

#[napi(object)]
pub struct MessageDto {
    pub id: String,
    pub role: String,
    pub text: String,
    pub tool_calls: Vec<ToolCallDto>,
}

#[napi(object)]
pub struct ConversationSnapshotDto {
    pub session_id: String,
    pub character: Option<CharacterSummaryDto>,
    pub messages: Vec<MessageDto>,
}

#[napi(object)]
pub struct ProviderConfigDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
}

#[napi(object)]
pub struct TurnRequestDto {
    pub session_id: String,
    pub user_text: String,
    pub provider: Option<ProviderConfigDto>,
    pub api_key: Option<String>,
}

/// Renders a domain enum as the snake_case label the bridge contract uses.
fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

impl From<SessionSummary> for SessionSummaryDto {
    fn from(value: SessionSummary) -> Self {
        Self {
            id: value.id,
            character_id: value.character_id.unwrap_or_default(),
            character_name: value.character_name,
            preview: value.preview,
            title: value.title,
            updated_at_ms: value.updated_at_ms as f64,
            message_count: value.message_count as u32,
        }
    }
}

impl From<CharacterSummary> for CharacterSummaryDto {
    fn from(value: CharacterSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            description: value.description,
        }
    }
}

impl From<ModelSummary> for ModelSummaryDto {
    fn from(value: ModelSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            provider_id: value.provider_id,
            provider_name: value.provider_name,
            kind: value.kind,
            configured: value.configured,
        }
    }
}

impl From<ContextSourceSummary> for ContextSourceDto {
    fn from(value: ContextSourceSummary) -> Self {
        Self {
            id: value.id,
            kind: enum_label(&value.kind),
            name: value.name,
            description: value.description,
            record_count: value.record_count as u32,
        }
    }
}

impl From<ToolCallSummary> for ToolCallDto {
    fn from(value: ToolCallSummary) -> Self {
        Self {
            id: value.id,
            name: value.name,
            title: value.title,
            status: value.status,
            is_error: value.is_error,
            result_text: value.result_text,
        }
    }
}

impl From<ConversationSnapshot> for ConversationSnapshotDto {
    fn from(value: ConversationSnapshot) -> Self {
        Self {
            session_id: value.session_id,
            character: value.character.map(CharacterSummaryDto::from),
            messages: value
                .messages
                .into_iter()
                .map(|message| MessageDto {
                    id: message.id,
                    role: serde_json::to_value(message.role)
                        .ok()
                        .and_then(|role| role.as_str().map(str::to_string))
                        .unwrap_or_else(|| "assistant".to_string()),
                    text: message.text,
                    tool_calls: message
                        .tool_calls
                        .into_iter()
                        .map(ToolCallDto::from)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl ProviderConfigDto {
    fn to_domain(&self) -> ProviderConfig {
        let mut extra = serde_json::Map::new();
        if let Some(max_tokens) = self.max_tokens {
            extra.insert("maxTokens".to_string(), max_tokens.into());
        }
        if let Some(temperature) = self.temperature {
            extra.insert("temperature".to_string(), temperature.into());
        }

        ProviderConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: serde_json::from_value(serde_json::Value::String(self.kind.clone()))
                .unwrap_or(sujiu_core::ProviderKind::OpenAiCompatible),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            credential_ref: None,
            extra,
        }
    }
}

impl TurnRequestDto {
    fn to_domain(self) -> SendTurnRequest {
        SendTurnRequest {
            session_id: self.session_id,
            user_text: self.user_text,
            provider: self.provider.map(|provider| provider.to_domain()),
            api_key: self.api_key,
        }
    }
}

/// Reports each normalized turn event to the platform bridge as JSON.
struct EventReporter {
    sink: ThreadsafeFunction<String>,
}

impl TurnEventReporter for EventReporter {
    fn report(&mut self, event: TurnEvent) {
        let payload = serde_json::to_string(&event).unwrap_or_else(|_| {
            serde_json::json!({ "kind": "turn_failed", "isError": true }).to_string()
        });
        // A closed platform callback must not take the runtime down.
        let _ = self
            .sink
            .call(Ok(payload), ThreadsafeFunctionCallMode::Blocking);
    }
}

/// Runs one turn off the UI thread and streams events back through a callback.
pub struct SendTurnTask {
    runtime: Arc<SujiuRuntime>,
    request: Option<SendTurnRequest>,
    sink: Option<ThreadsafeFunction<String>>,
}

impl Task for SendTurnTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        let request = self
            .request
            .take()
            .ok_or_else(|| napi::Error::from_reason("turn already started"))?;
        let mut reporter = EventReporter {
            sink: self
                .sink
                .take()
                .ok_or_else(|| napi::Error::from_reason("turn already started"))?,
        };

        self.runtime
            .tokio
            .block_on(self.runtime.send_turn(request, &mut reporter));

        // The turn result is a summary for the caller only; the events already
        // carried the full stream to the platform.
        Ok(serde_json::json!({ "ok": true }).to_string())
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

/// Builds a runtime from the seed catalog that ships with the app.
///
/// This is a free function rather than an associated one on purpose: a factory
/// declared inside `impl SujiuRuntimeBridge` is registered as a static method
/// on the exported class, so ArkTS would have to call
/// `SujiuRuntimeBridge.create()` instead of `create()`.
#[napi]
pub fn create() -> Result<SujiuRuntimeBridge> {
    // The seeded catalog, not Seed::default(): a fresh launch would otherwise
    // have no characters, no history and no context sources to show.
    let runtime = SujiuRuntime::new(sujiu_ffi::seed::seed())
        .map_err(|error| napi::Error::from_reason(format!("runtime init failed: {error}")))?;

    Ok(SujiuRuntimeBridge {
        runtime: Arc::new(runtime),
    })
}

#[napi]
impl SujiuRuntimeBridge {
    /// Attaches a directory the runtime may persist to.
    ///
    /// The platform chooses the location; the runtime decides what goes in it.
    /// Attaching after startup restores a document that is already there, so a
    /// frontend can hand the directory over whenever it learns it.
    #[napi]
    pub fn use_data_directory(&self, directory: String) -> Result<()> {
        self.runtime
            .use_directory(&directory)
            .map_err(napi::Error::from_reason)
    }

    /// The directory currently attached, or null when the runtime is in memory.
    #[napi]
    pub fn data_directory(&self) -> Option<String> {
        self.runtime.directory()
    }

    /// The shared runtime version, so a platform can report what it is bound to.
    #[napi]
    pub fn core_version(&self) -> String {
        sujiu_ffi::CORE_VERSION.to_string()
    }

    #[napi]
    pub fn list_sessions(&self) -> Vec<SessionSummaryDto> {
        self.runtime
            .sessions()
            .into_iter()
            .map(SessionSummaryDto::from)
            .collect()
    }

    #[napi]
    pub fn list_characters(&self, query: Option<String>) -> Vec<CharacterSummaryDto> {
        self.runtime
            .characters(query.as_deref().unwrap_or_default())
            .into_iter()
            .map(CharacterSummaryDto::from)
            .collect()
    }

    #[napi]
    pub fn list_models(&self) -> Vec<ModelSummaryDto> {
        self.runtime
            .models()
            .into_iter()
            .map(ModelSummaryDto::from)
            .collect()
    }

    #[napi]
    pub fn list_context_sources(&self) -> Result<Vec<ContextSourceDto>> {
        let sources = self
            .runtime
            .tokio
            .block_on(async { self.runtime.context_sources().await })
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;

        Ok(sources.into_iter().map(ContextSourceDto::from).collect())
    }

    #[napi]
    pub fn conversation_state(&self, session_id: String) -> Result<ConversationSnapshotDto> {
        self.runtime
            .conversation_state(&session_id)
            .map(ConversationSnapshotDto::from)
            .ok_or_else(|| napi::Error::from_reason(format!("unknown session: {session_id}")))
    }

    #[napi]
    pub fn create_session(&self, character_id: String) -> String {
        self.runtime.create_session(Some(character_id.as_str()))
    }

    /// Provider kinds the runtime can drive, so a settings screen offers only
    /// those. A kind it cannot serve would fail every turn.
    #[napi]
    pub fn provider_kinds(&self) -> Vec<String> {
        SujiuRuntime::supported_provider_kinds()
    }

    /// Store the provider a settings screen configured.
    ///
    /// Throws when the kind is not supported, so the screen can tell the user
    /// immediately instead of at the first turn.
    #[napi]
    pub fn configure_provider(&self, provider: ProviderConfigDto) -> Result<()> {
        self.runtime
            .set_provider_config(Some(provider.to_domain()))
            .map_err(|error| napi::Error::from_reason(format!("{error}")))
    }

    /// Sends one turn. The callback receives normalized turn events as JSON.
    #[napi]
    pub fn send_turn(
        &self,
        request: TurnRequestDto,
        on_event: ThreadsafeFunction<String>,
    ) -> AsyncTask<SendTurnTask> {
        AsyncTask::new(SendTurnTask {
            runtime: Arc::clone(&self.runtime),
            request: Some(request.to_domain()),
            sink: Some(on_event),
        })
    }

    #[napi]
    pub fn cancel_turn(&self) {
        self.runtime.cancel();
    }
}
