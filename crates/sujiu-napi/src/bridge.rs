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
use sujiu_ai::DiagnosticEntry;
use sujiu_core::EndpointConfig;
use sujiu_ffi::events::{TurnEvent, TurnEventReporter};
use sujiu_ffi::runtime::{
    CharacterSummary, ContextSourceSummary, ConversationSnapshot, EndpointExploration,
    ModelDiscovery, ModelSummary, SendTurnRequest, SessionSummary, SujiuRuntime, ToolCallSummary,
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
    pub endpoint_id: String,
    /// A word to show a person, derived from the address. It names who answers
    /// and decides nothing: a gateway behind the same name can serve unrelated
    /// models, and the runtime never asks it.
    pub endpoint_label: String,
    pub configured: bool,
}

/// What model discovery found, and what the user may still do about it.
#[napi(object)]
pub struct ModelDiscoveryDto {
    pub listing: String,
    pub note: String,
    /// Always true, so a settings screen can keep the manual field enabled.
    pub manual_entry_allowed: bool,
    pub models: Vec<ModelSummaryDto>,
}

impl From<ModelDiscovery> for ModelDiscoveryDto {
    fn from(discovery: ModelDiscovery) -> Self {
        Self {
            listing: discovery.listing,
            note: discovery.note,
            manual_entry_allowed: discovery.manual_entry_allowed,
            models: discovery
                .models
                .into_iter()
                .map(ModelSummaryDto::from)
                .collect(),
        }
    }
}

/// What an unsaved endpoint turned out to be.
///
/// `protocol` and `models` are two independent answers, and reporting them as
/// one would be a claim Sujiu cannot make: an endpoint that lists nothing may
/// still speak the newest protocol, and one that lists fifty models may speak
/// only the oldest.
#[napi(object)]
pub struct EndpointExplorationDto {
    pub protocol: String,
    pub protocols: Vec<String>,
    /// How the protocol probe came out, as a code to branch on.
    ///
    /// A settings screen shows a different message per code. "Probe failed" as a
    /// single message is the state this replaces: a wrong key, a wrong path and
    /// a switched-off router all read the same, and each of them sends the user
    /// to edit a different field.
    pub status: String,
    /// Why no protocol was settled, when none was. A sentence to read; `status`
    /// is what to switch on.
    pub reason: Option<String>,
    pub models: ModelDiscoveryDto,
}

impl From<EndpointExploration> for EndpointExplorationDto {
    fn from(exploration: EndpointExploration) -> Self {
        Self {
            protocol: exploration.protocol,
            protocols: exploration.protocols,
            status: exploration.status,
            reason: exploration.reason,
            models: ModelDiscoveryDto::from(exploration.models),
        }
    }
}

/// One line of the diagnostic log, as a settings screen or a bug report needs it.
///
/// The text arrived already redacted. Nothing here is a raw body, and nothing
/// here is a credential: the runtime removes those before a line is ever
/// recorded, so a platform can render or share this without a second pass.
#[napi(object)]
pub struct DiagnosticEntryDto {
    /// `discovery` for capability and model probing, `chat` for a conversation.
    /// The two are separate investigations and a reader must be able to tell
    /// which one a line belongs to.
    pub kind: String,
    /// Where in that investigation this line sits: `probe_start`,
    /// `protocol_result`, `chat_request`, `turn_failed`, and so on.
    pub stage: String,
    pub message: String,
    pub at_ms: f64,
    /// The detail as a rendered `key=value` line, ready to show.
    pub detail: String,
}

impl From<&DiagnosticEntry> for DiagnosticEntryDto {
    fn from(entry: &DiagnosticEntry) -> Self {
        Self {
            kind: entry.kind.label().to_string(),
            stage: entry.stage.clone(),
            message: entry.message.clone(),
            at_ms: entry.at_ms as f64,
            detail: entry.render(),
        }
    }
}

/// An endpoint that is already stored, as far as a form may know one.
///
/// No credential field, and none is wanted: a form that could read the key back
/// would be one `dump the view state` away from writing it to a log. The key is
/// the one thing a settings screen asks to be told "yes you have one" and
/// nothing more.
#[napi(object)]
pub struct StoredEndpointDto {
    pub id: String,
    pub base_url: String,
    /// `None` until the user has chosen one, which is a normal state: an
    /// endpoint can be asked what it offers before anyone picks a model.
    pub selected_model: Option<String>,
    /// A word to show a person. Decides nothing.
    pub label: String,
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
    pub base_url: String,
    /// The model the user picked, if they have picked one yet.
    ///
    /// Optional because exploration runs first: an endpoint is worth asking
    /// about before anyone has chosen what to ask it. A form that insists on
    /// a model before it will save is the reason discovery used to have to
    /// come last.
    pub selected_model: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    /// Whether this endpoint needs the assistant reasoning that produced a tool
    /// call sent back with it.
    ///
    /// Optional because most endpoints do not, and a known thinking-mode
    /// service is recognised without it. A platform only describes the provider
    /// it was given; the protocol requirement is the runtime's answer, so this
    /// is a stated capability rather than something a form has to know about.
    pub replays_assistant_reasoning: Option<bool>,
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
            endpoint_id: value.endpoint_id,
            endpoint_label: value.endpoint_label,
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
    fn to_domain(&self) -> EndpointConfig {
        let mut extra = serde_json::Map::new();
        if let Some(max_tokens) = self.max_tokens {
            extra.insert("maxTokens".to_string(), max_tokens.into());
        }
        if let Some(temperature) = self.temperature {
            extra.insert("temperature".to_string(), temperature.into());
        }
        if let Some(replays) = self.replays_assistant_reasoning {
            extra.insert(
                sujiu_core::REPLAYS_ASSISTANT_REASONING_KEY.to_string(),
                replays.into(),
            );
        }

        EndpointConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            // A blank model means "not chosen yet", not "the empty model".
            selected_model: self
                .selected_model
                .as_deref()
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string),
            credential_ref: None,
            overrides: extra,
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

/// What a settings form gets back about an endpoint that is already configured.
///
/// A free function so the conversion can be tested on its own. The generated
/// binding around it needs a live NAPI runtime and so cannot be reached from a
/// test binary at all, which makes this the part that has to be right by
/// construction rather than by luck.
///
/// Note what is absent: a credential. Every field here becomes a property the
/// form can read, and a form that could read the key back would be one careless
/// log line away from printing it. The address and the chosen model are enough
/// to redraw the screen, and nothing more is offered.
fn stored_endpoint_of(config: Option<EndpointConfig>) -> Option<StoredEndpointDto> {
    config.map(|config| StoredEndpointDto {
        id: config.id.clone(),
        base_url: config.base_url.clone(),
        selected_model: config.selected_model.clone(),
        label: config.display_label(),
    })
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

    /// Ask the endpoint which models it serves.
    ///
    /// A convenience for the settings screen. It never decides how a turn
    /// runs, and `manualEntryAllowed` is always true: a gateway with no model
    /// list still chats, so a listing failure must not take the manual choice
    /// away.
    #[napi]
    pub fn discover_models(&self, api_key: Option<String>) -> Result<ModelDiscoveryDto> {
        let discovery = self
            .runtime
            .tokio
            .block_on(self.runtime.discover_models(api_key.as_deref()));
        Ok(ModelDiscoveryDto::from(discovery))
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

    /// The wire formats the runtime speaks, in the order it tries them.
    ///
    /// This replaces the list of provider kinds a settings screen used to be
    /// given. A vendor list asked the user to decide something negotiation
    /// settles by asking the endpoint, and a name in that list could be wrong
    /// for the very address the user typed.
    #[napi]
    pub fn supported_protocols(&self) -> Vec<String> {
        SujiuRuntime::supported_protocols()
    }

    /// Store the endpoint a settings screen configured.
    ///
    /// There is no kind to reject: an address the runtime has never heard of
    /// is exactly the self-hosted gateway this is meant to support. Whether it
    /// can be reached, and what it speaks, is what the first turn's
    /// negotiation answers.
    #[napi]
    pub fn configure_provider(&self, provider: ProviderConfigDto) -> Result<()> {
        self.runtime
            .set_endpoint(Some(provider.to_domain()))
            .map_err(|error| napi::Error::from_reason(format!("{error}")))
    }

    /// Read back what is already configured, so a settings form can open on
    /// the truth rather than on three empty fields.
    ///
    /// No credential comes back. The key lives in platform storage and is
    /// never something a form can read into a field and then leak into a log
    /// line or a state dump.
    #[napi]
    pub fn stored_endpoint(&self) -> Option<StoredEndpointDto> {
        stored_endpoint_of(self.runtime.endpoint())
    }

    /// What an endpoint told us before, without asking it again.
    ///
    /// This is what a platform reads at launch so the model list is already
    /// there. It never reaches the network, which is the difference from
    /// `discover_endpoint`: a launch that quietly asked would cost every user a
    /// round trip every time they opened the app, and on a free tier a rate
    /// limit as well.
    #[napi]
    pub fn remembered_endpoint(&self, base_url: String) -> Option<EndpointExplorationDto> {
        self.runtime
            .remembered_endpoint(&base_url)
            .map(EndpointExplorationDto::from)
    }

    /// Find out what an endpoint is, from an address and a key and nothing else.
    ///
    /// This is the order the user actually works in, and it is why the settings
    /// screen can be shown a model list before anything has been saved: there is
    /// no configuration to complete first, and no model to know in advance.
    /// The model list and the protocol are reported as two independent answers,
    /// because a gateway can list nothing and still speak Responses.
    ///
    /// `refresh` is what a platform's "ask again" button passes. Leaving it out
    /// asks the cache, which is what an ordinary open of a settings screen
    /// wants: the endpoint already answered this, and re-asking costs the user a
    /// wait and the endpoint a rate limit.
    #[napi]
    pub fn discover_endpoint(
        &self,
        base_url: String,
        api_key: String,
        refresh: Option<bool>,
    ) -> Result<EndpointExplorationDto> {
        let exploration = self.runtime.tokio.block_on(self.runtime.discover_endpoint(
            &base_url,
            &api_key,
            refresh.unwrap_or(false),
        ));
        Ok(EndpointExplorationDto::from(exploration))
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

    /// The diagnostic log, oldest entry first.
    ///
    /// This is what makes a failure debuggable instead of guessable: it names the
    /// endpoint, every protocol that was tried, what each one answered, whether
    /// the model list worked, which protocol and model the turn actually used,
    /// and which stage a failure happened in. `limit` is a number of most
    /// recent entries; zero means everything the log still holds.
    #[napi]
    pub fn diagnostics(&self, limit: Option<u32>) -> Vec<DiagnosticEntryDto> {
        self.runtime
            .diagnostics(limit.unwrap_or(0) as usize)
            .iter()
            .map(DiagnosticEntryDto::from)
            .collect()
    }

    /// Only the capability and model probing half of the log.
    ///
    /// Someone deciding which key to type wants the endpoint's behaviour, not
    /// the last conversation's traffic.
    #[napi]
    pub fn discovery_diagnostics(&self, limit: Option<u32>) -> Vec<DiagnosticEntryDto> {
        self.runtime
            .discovery_diagnostics(limit.unwrap_or(0) as usize)
            .iter()
            .map(DiagnosticEntryDto::from)
            .collect()
    }

    /// The whole log as text, for the "copy this into a bug report" case.
    #[napi]
    pub fn diagnostics_text(&self) -> String {
        self.runtime.diagnostics_text()
    }

    /// Start a fresh investigation. The stored endpoint and key are untouched:
    /// the log is evidence, not configuration.
    #[napi]
    pub fn clear_diagnostics(&self) {
        self.runtime.clear_diagnostics();
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderConfigDto;
    use sujiu_core::{apply_reasoning_override, EndpointCapabilities, Protocol};
    use sujiu_ffi::events::TurnEventReporter;

    fn form_fields(replays_assistant_reasoning: Option<bool>) -> ProviderConfigDto {
        ProviderConfigDto {
            id: "provider-1".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            selected_model: Some("deepseek-reasoner".into()),
            max_tokens: Some(2048),
            temperature: Some(0.7),
            replays_assistant_reasoning,
        }
    }

    /// A platform only describes the endpoint: an id, a URL, a model and a
    /// key. It does not declare which wire protocol the endpoint speaks, and it
    /// must not have to, because that answer goes stale and a form cannot know
    /// it. So the settings form's fields convert to a config that asks the
    /// endpoint rather than asserting anything about it.
    #[test]
    fn a_settings_form_does_not_have_to_declare_any_protocol_capability() {
        let config = form_fields(None).to_domain();

        assert_eq!(config.base_url, "https://api.deepseek.com/v1");
        assert_eq!(config.selected_model.as_deref(), Some("deepseek-reasoner"));
        assert_eq!(
            config.forced_reasoning_replay(),
            None,
            "the form stated nothing, so nothing may be inferred from a vendor or model name"
        );
        assert_eq!(
            config.protocols(),
            Protocol::PRIORITY.to_vec(),
            "there is nothing for a form to choose and nothing to derive from a name"
        );
    }

    /// The model may be left unchosen, because exploration happens before
    /// anyone has picked one. A form that cannot express "not yet" would force
    /// the user to know the answer before the question could be asked.
    #[test]
    fn an_endpoint_crosses_the_bridge_before_a_model_is_chosen() {
        let config = ProviderConfigDto {
            selected_model: None,
            ..form_fields(None)
        }
        .to_domain();

        assert!(
            config.selected_model.is_none(),
            "no model chosen yet is a real state, not a missing field"
        );
        assert!(config.protocols().contains(&Protocol::OpenAiResponses));
    }

    /// A blank model means the same thing, rather than becoming a request for
    /// a model whose name is nothing.
    #[test]
    fn a_blank_model_is_treated_as_unchosen_rather_than_as_a_name() {
        let config = ProviderConfigDto {
            selected_model: Some("   ".into()),
            ..form_fields(None)
        }
        .to_domain();

        assert!(config.selected_model.is_none());
    }

    /// The same fields, whatever model name they carry, lead to the same
    /// question. This is the behaviour the old vendor-name lookup had and the
    /// negotiation replaced: a gateway serving any number of models is still
    /// one endpoint, and a model name is a request parameter.
    #[test]
    fn no_model_name_decides_what_the_endpoint_speaks() {
        for model in ["deepseek-reasoner", "r1", "thinking-v2", "gpt-4o-mini"] {
            let dto = ProviderConfigDto {
                selected_model: Some(model.into()),
                ..form_fields(None)
            };
            let config = dto.to_domain();

            assert_eq!(config.forced_reasoning_replay(), None, "{model}");
            assert_eq!(
                config.capability_key(),
                "provider-1|https://api.deepseek.com/v1",
                "{model} is a parameter, not an endpoint"
            );
        }
    }

    /// The display name a user typed is a label and stays one.
    #[test]
    fn a_display_name_is_never_part_of_how_the_endpoint_is_asked() {
        let named = ProviderConfigDto {
            name: "My own gateway".into(),
            ..form_fields(None)
        }
        .to_domain();
        let unnamed = ProviderConfigDto {
            name: "  ".into(),
            ..form_fields(None)
        }
        .to_domain();

        assert_eq!(named.display_label(), "My own gateway");
        assert_ne!(
            named.display_label(),
            unnamed.display_label(),
            "an unnamed endpoint still shows something a person can recognise"
        );
        assert_eq!(
            named.capability_key(),
            unnamed.capability_key(),
            "what the two are asked is decided by where they point, not what they are called"
        );
    }

    /// A platform that does know something the negotiation cannot see may still
    /// say so, and saying it has to survive the conversion in both directions.
    #[test]
    fn a_stated_capability_survives_the_bridge_in_both_directions() {
        let on = form_fields(Some(true)).to_domain();
        assert_eq!(on.forced_reasoning_replay(), Some(true));

        let off = form_fields(Some(false)).to_domain();
        assert_eq!(off.forced_reasoning_replay(), Some(false));
    }

    /// A stated capability is applied on top of what the endpoint was found to
    /// do, and it only ever changes the reasoning answer.
    #[test]
    fn a_stated_capability_is_applied_on_top_of_a_negotiated_one() {
        let negotiated = EndpointCapabilities::negotiate(&[Protocol::OpenAiChatCompletions])
            .expect("chat completions is implemented");

        let forced = apply_reasoning_override(negotiated.clone(), Some(true));

        assert!(forced.replays_assistant_reasoning);
        assert_eq!(forced.protocol, negotiated.protocol);
        assert_eq!(forced.supported, negotiated.supported);
    }

    /// A settings form has to be able to redraw itself on what is configured.
    ///
    /// This drives the round trip a platform actually performs: a DTO goes in,
    /// the runtime stores it, and a DTO comes back out for the form. The
    /// generated NAPI binding around those two calls needs a live NAPI runtime
    /// and cannot be linked into a test binary at all, so the two mappings on
    /// either side of the runtime are driven directly. Those are the only parts
    /// that can quietly go wrong -- a dropped field, a model arriving as an
    /// empty string instead of absent, a credential appearing by accident.
    #[test]
    fn a_settings_form_can_open_on_what_is_already_configured() {
        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");

        assert!(
            super::stored_endpoint_of(runtime.endpoint()).is_none(),
            "nothing is configured until something is"
        );

        // An address on its own is still an endpoint: a model is chosen after
        // discovery, not before it.
        runtime
            .set_endpoint(Some(
                ProviderConfigDto {
                    id: "endpoint-default".into(),
                    name: "Gateway".into(),
                    base_url: "https://gateway.example/v1".into(),
                    selected_model: None,
                    max_tokens: None,
                    temperature: None,
                    replays_assistant_reasoning: None,
                }
                .to_domain(),
            ))
            .expect("an endpoint without a model");

        let chosen = super::stored_endpoint_of(runtime.endpoint()).expect("it was just stored");
        assert_eq!(chosen.base_url, "https://gateway.example/v1");
        assert_eq!(
            chosen.selected_model, None,
            "a model that was never chosen is absent, not an empty string"
        );

        runtime
            .set_endpoint(Some(
                ProviderConfigDto {
                    id: "endpoint-default".into(),
                    name: "Gateway".into(),
                    base_url: "https://gateway.example/v1".into(),
                    selected_model: Some("picked-model".into()),
                    max_tokens: None,
                    temperature: None,
                    replays_assistant_reasoning: None,
                }
                .to_domain(),
            ))
            .expect("a chosen model");

        let chosen = super::stored_endpoint_of(runtime.endpoint()).expect("it is still stored");
        assert_eq!(chosen.base_url, "https://gateway.example/v1");
        assert_eq!(chosen.selected_model.as_deref(), Some("picked-model"));
        assert_eq!(chosen.label, "Gateway");
    }

    /// The log is only useful if a platform can render it, and it is only safe if
    /// a platform can never be the thing that leaks the key. The redaction has
    /// already happened by the time a DTO exists, so what crosses is a finished
    /// line and there is no second pass to forget.
    ///
    /// The NAPI methods themselves need a live runtime binding and cannot run in
    /// a test binary, so this drives the mapping and the log the mapping reads.
    #[test]
    fn a_diagnostic_line_crosses_the_bridge_ready_to_show_and_without_the_key() {
        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        let key = "sk-sujiu-bridge-1234567890abcdef";

        runtime.tokio.block_on(runtime.discover_endpoint(
            "https://sujiu-does-not-resolve.invalid/v1".into(),
            key.into(),
            true,
        ));

        let entries: Vec<super::DiagnosticEntryDto> = runtime
            .diagnostics(0)
            .iter()
            .map(super::DiagnosticEntryDto::from)
            .collect();

        assert!(!entries.is_empty(), "the probe has to have said something");

        for entry in &entries {
            assert!(
                !entry.message.contains(key) && !entry.detail.contains(key),
                "a credential reached the platform: {}",
                entry.detail
            );
            assert!(
                !entry.stage.is_empty(),
                "a line with no stage cannot be read"
            );
            assert!(entry.at_ms > 0.0, "a line with no time cannot be ordered");
        }

        let requested = entries
            .iter()
            .find(|entry| entry.stage == "discovery_request")
            .expect("the request that started it all");
        assert_eq!(requested.kind, "discovery");
        assert!(
            requested.detail.contains("sujiu-does-not-resolve.invalid"),
            "the line has to name the endpoint, or it is not diagnosable: {}",
            requested.detail
        );
        assert!(
            !requested.message.contains("sk-sujiu-bridge"),
            "the masked form must not be the raw one"
        );
    }

    /// Asking for one half of the log has to return one half. A settings screen
    /// showing the endpoint's behaviour should not have the last conversation
    /// interleaved into it, or the relevant line is impossible to pick out.
    #[test]
    fn a_platform_reading_only_the_discovery_half_gets_only_the_discovery_half() {
        let runtime =
            sujiu_ffi::runtime::SujiuRuntime::new(sujiu_ffi::seed::seed()).expect("runtime");
        runtime
            .set_endpoint(Some(
                ProviderConfigDto {
                    id: "endpoint-default".into(),
                    name: "Gateway".into(),
                    base_url: "https://gateway.example/v1".into(),
                    selected_model: Some("picked-model".into()),
                    max_tokens: None,
                    temperature: None,
                    replays_assistant_reasoning: None,
                }
                .to_domain(),
            ))
            .expect("provider");

        let mut reporter = sujiu_ffi::runtime::CollectingReporter::default();
        runtime.tokio.block_on(runtime.send_turn(
            sujiu_ffi::runtime::SendTurnRequest {
                session_id: runtime.sessions()[0].id.clone(),
                user_text: "hello".into(),
                provider: None,
                api_key: Some("sk-a-key-1234567890abcdef".into()),
            },
            &mut reporter,
        ));

        let everything: Vec<super::DiagnosticEntryDto> = runtime
            .diagnostics(0)
            .iter()
            .map(super::DiagnosticEntryDto::from)
            .collect();
        let discovery: Vec<super::DiagnosticEntryDto> = runtime
            .discovery_diagnostics(0)
            .iter()
            .map(super::DiagnosticEntryDto::from)
            .collect();

        assert!(
            everything.len() > discovery.len(),
            "the turn produced lines the discovery filter should have removed"
        );
        assert!(
            discovery.iter().all(|entry| entry.kind == "discovery"),
            "the filtered read leaked the conversation half"
        );
    }
}
