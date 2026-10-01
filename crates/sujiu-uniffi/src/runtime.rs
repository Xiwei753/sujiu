//! The exported runtime object: the whole app-facing surface Android sees.
//!
//! Every method here is a translation from a UniFFI record into a
//! `sujiu_runtime` call and back. No provider wire format, no HTTP detail and no
//! internal store shape appears in any of these signatures, because a platform
//! reading them should be reading Sujiu's own domain model.

use std::sync::Arc;

use sujiu_runtime::events::{TurnEvent as RuntimeTurnEvent, TurnEventReporter};
use sujiu_runtime::runtime::SujiuRuntime as InnerRuntime;

use crate::error::UniError;
use crate::records::*;

/// Receives normalized turn events while a turn runs.
///
/// A callback interface rather than a returned list, because a turn is a
/// sequence the user watches: buffering it to return at the end would make a
/// streamed answer arrive all at once, which is the thing streaming was for.
#[uniffi::export(callback_interface)]
pub trait TurnEventListener: Send + Sync {
    fn on_turn_event(&self, event: TurnEventRecord);
}

/// One normalized event, as the platforms render it.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TurnEventRecord {
    pub kind: TurnEventKind,
    /// Present only on the events that carry text.
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub is_error: Option<bool>,
}

impl From<RuntimeTurnEvent> for TurnEventRecord {
    fn from(event: RuntimeTurnEvent) -> Self {
        Self {
            kind: event.kind.into(),
            text: event.text,
            tool_name: event.tool_name,
            tool_call_id: event.tool_call_id,
            is_error: event.is_error,
        }
    }
}

/// Bridges the exported listener into the runtime's reporter.
///
/// The runtime reports through `&mut self`, and a UniFFI callback is reached
/// through a shared reference, so the calls are serialized behind a lock. Two
/// concurrent reports cannot interleave inside the runtime anyway, because a
/// turn is driven by one task.
struct ListenerReporter {
    listener: Box<dyn TurnEventListener>,
}

impl TurnEventReporter for ListenerReporter {
    fn report(&mut self, event: RuntimeTurnEvent) {
        self.listener.on_turn_event(event.into());
    }
}

/// The shared runtime, as Android sees it.
///
/// An exported object, not a record: it holds a live runtime with a tokio
/// handle and a cancel token, and handing Kotlin a copy of that would hand it a
/// copy of the cancel token too, so cancelling from the copy would leave the
/// real turn running.
#[derive(uniffi::Object)]
pub struct SujiuApp {
    inner: Arc<InnerRuntime>,
}

/// Open a runtime over the seeded catalog and in-memory storage.
///
/// The seeded catalog rather than an empty one, for the same reason the N-API
/// bridge seeds: a fresh launch with no characters, no history and no context
/// sources has nothing to show.
#[uniffi::export]
pub fn create() -> Result<Arc<SujiuApp>, UniError> {
    let runtime = InnerRuntime::new(sujiu_runtime::seed::seed())
        .map_err(|error| UniError::Storage(error.to_string()))?;
    Ok(Arc::new(SujiuApp {
        inner: Arc::new(runtime),
    }))
}

/// The protocols this build can speak, best first.
///
/// A fixed priority for every endpoint, in one place. Nothing a user picks
/// reorders it. A free function rather than a method because it describes the
/// build, not one runtime: two apps sharing a library link get the same answer,
/// and asking a runtime object invites the reading that it negotiated its own.
#[uniffi::export]
pub fn supported_protocols() -> Vec<String> {
    InnerRuntime::supported_protocols()
}

/// One tokio runtime behind every object, rather than one per exported method.
///
/// The app already runs a multi-threaded tokio runtime for the provider calls;
/// a second runtime for the exported futures would make the two disagree about
/// which worker a blocking store write runs on.
#[uniffi::export(async_runtime = "tokio")]
impl SujiuApp {
    /// Point the runtime at a directory and load what is there.
    pub fn use_directory(&self, path: String) -> Result<(), UniError> {
        self.inner.use_directory(&path).map_err(UniError::Storage)
    }

    /// The document that must not be overwritten, when one is protected.
    ///
    /// A stored document the runtime cannot read is protected from every later
    /// save, not only from the one that discovered it.
    pub fn storage_protection(&self) -> Option<String> {
        self.inner.storage_protection()
    }

    /// Release the protection, so the user has said to write over it.
    ///
    /// The fresh catalog is what it falls back to, which is why this is not
    /// merely "make the store writable": the document that could not be read is
    /// still there, and writing over it has to start from a known state.
    pub fn discard_protected_document(&self) -> Result<(), UniError> {
        self.inner
            .discard_protected_document(sujiu_runtime::seed::seed())
            .map_err(UniError::from)
    }

    /// Where the runtime is storing its documents, when it has been told to
    /// store them.
    pub fn data_directory(&self) -> Option<String> {
        self.inner.directory()
    }

    pub fn core_version(&self) -> String {
        sujiu_core::CORE_VERSION.to_string()
    }

    // --- Endpoint and provider configuration -----------------------------

    /// The saved endpoint, if there is one.
    pub fn endpoint(&self) -> Option<EndpointRecord> {
        self.inner.endpoint().as_ref().map(EndpointRecord::from)
    }

    /// Save or replace the endpoint, and forget anything negotiated about the
    /// previous address.
    pub fn configure_endpoint(&self, endpoint: Option<EndpointRecord>) -> Result<(), UniError> {
        self.inner
            .set_endpoint(endpoint.map(Into::into))
            .map_err(Into::into)
    }

    // --- Discovery --------------------------------------------------------

    /// The models the saved endpoint has already told us about.
    ///
    /// The cached list, not a new question: a model picker that probed on every
    /// open would turn opening a screen into a network request the user did not
    /// ask for, and a gateway that has just changed its mind would silently
    /// change which model a conversation is about to use. Use
    /// [`SujiuApp::discover_models`] to ask again on purpose.
    pub fn list_models(&self) -> Vec<ModelRecord> {
        self.inner.models().into_iter().map(Into::into).collect()
    }

    /// Ask the endpoint what models it has.
    ///
    /// Discovery runs before a model is known: a base URL and a key are enough to
    /// explore an endpoint, so the choice is not hidden behind a saved
    /// configuration. Exploring writes nothing.
    pub async fn discover_models(&self, api_key: Option<String>) -> ModelDiscoveryRecord {
        self.inner.discover_models(api_key.as_deref()).await.into()
    }

    /// Probe an unsaved address, so a user can find out whether it works before
    /// committing to it.
    pub async fn discover_endpoint(
        &self,
        base_url: String,
        api_key: String,
        refresh: bool,
    ) -> EndpointExplorationRecord {
        self.inner
            .discover_endpoint(&base_url, &api_key, refresh)
            .await
            .into()
    }

    /// What an address said earlier, if anything.
    pub fn remembered_endpoint(&self, base_url: String) -> Option<EndpointExplorationRecord> {
        self.inner.remembered_endpoint(&base_url).map(Into::into)
    }

    /// Everything the runtime already knows about an endpoint, without asking it
    /// again.
    pub async fn explore_endpoint(
        &self,
        endpoint: EndpointRecord,
        api_key: Option<String>,
        refresh: bool,
    ) -> EndpointExplorationRecord {
        self.inner
            .explore_endpoint(&endpoint.into(), api_key.as_deref(), refresh)
            .await
            .into()
    }

    // --- Conversations -----------------------------------------------------

    pub fn list_conversations(&self) -> Vec<ConversationRecord> {
        self.inner
            .conversations()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn create_conversation(&self, bindings: ConversationBindingRecord) -> String {
        self.inner.create_conversation(&bindings.into())
    }

    /// Create a conversation with one character, which is what a new chat from a
    /// character card means.
    pub fn create_conversation_with(&self, character_id: String) -> String {
        self.inner.create_session(Some(character_id.as_str()))
    }

    /// The conversation's bindings and its whole transcript.
    pub fn conversation_state(
        &self,
        conversation_id: String,
    ) -> Option<ConversationSnapshotRecord> {
        self.inner
            .conversation_state(&conversation_id)
            .map(Into::into)
    }

    /// Replace what a conversation binds. Its transcript is left alone.
    pub fn set_conversation_bindings(
        &self,
        conversation_id: String,
        bindings: ConversationBindingRecord,
    ) -> Result<(), UniError> {
        self.inner
            .set_conversation_bindings(&conversation_id, &bindings.into())
            .map_err(Into::into)
    }

    /// What a compaction would be given, so a screen can show it before running.
    ///
    /// Nothing here asks the caller to remember what it cannot see: the previous
    /// summary and every turn a new one has to cover are both included, and the
    /// prompt text already leads with the previous summary so a summarizer
    /// extends it instead of starting over. Empty means there is nothing to
    /// compact yet.
    pub fn compaction_input(
        &self,
        conversation_id: String,
        keep_recent: u32,
    ) -> Result<Option<CompactionInputRecord>, UniError> {
        self.inner
            .compaction_input(&conversation_id, keep_recent as usize)
            .map(|input| input.as_ref().map(CompactionInputRecord::from))
            .map_err(Into::into)
    }

    /// Run a compaction now, with a summary the caller wrote.
    ///
    /// The summary is the caller's because writing one needs a model turn of its
    /// own; the runtime's job is to cut on a turn boundary and keep the archived
    /// turns retrievable. Reports whether anything actually moved, because
    /// "nothing to compact" is an outcome and not a failure.
    pub fn compact_conversation(
        &self,
        conversation_id: String,
        keep_recent: u32,
        summary: String,
    ) -> Result<bool, UniError> {
        self.inner
            .compact_session(&conversation_id, keep_recent as usize, &summary)
            .map_err(Into::into)
    }

    // --- Sending a turn ----------------------------------------------------

    /// Run one turn, reporting events to `listener` as they happen.
    ///
    /// The transcript is committed whatever happens: completion, failure,
    /// cancellation and an exhausted tool budget all leave the steps that did
    /// finish in place, because those steps carry tool calls whose results the
    /// next request has to repeat. A turn reports its outcome through events.
    pub async fn send_turn(
        &self,
        request: SendTurnRequestRecord,
        listener: Box<dyn TurnEventListener>,
    ) {
        let mut reporter = ListenerReporter { listener };
        self.inner.send_turn(request.into(), &mut reporter).await;
    }

    /// Ask the running turn to stop.
    ///
    /// The turn still returns its transcript; an interrupted tool call is
    /// recorded as interrupted rather than dropped.
    pub fn cancel_turn(&self) {
        self.inner.cancel();
    }

    // --- Library: characters ----------------------------------------------

    pub fn list_characters(&self, query: Option<String>) -> Vec<CharacterCard> {
        self.inner
            .characters(query.as_deref().unwrap_or_default())
            .into_iter()
            .map(Into::into)
            .collect()
    }

    /// One character in full, for an editor to prefill from.
    pub fn character(&self, id: String) -> Option<CharacterDraftRecord> {
        self.inner.character(&id).as_ref().map(Into::into)
    }

    pub fn save_character(&self, draft: CharacterDraftRecord) -> Result<String, UniError> {
        self.inner.save_character(&draft.into()).map_err(Into::into)
    }

    pub fn delete_character(&self, id: String) -> Result<bool, UniError> {
        self.inner.delete_character(&id).map_err(Into::into)
    }

    // --- Library: personas -------------------------------------------------

    pub fn list_personas(&self) -> Vec<PersonaCard> {
        self.inner.personas().into_iter().map(Into::into).collect()
    }

    pub fn persona(&self, id: String) -> Option<PersonaDraftRecord> {
        self.inner.persona(&id).as_ref().map(Into::into)
    }

    pub fn save_persona(&self, draft: PersonaDraftRecord) -> Result<String, UniError> {
        self.inner.save_persona(&draft.into()).map_err(Into::into)
    }

    pub fn delete_persona(&self, id: String) -> Result<bool, UniError> {
        self.inner.delete_persona(&id).map_err(Into::into)
    }

    // --- Library: world books ---------------------------------------------

    pub fn list_world_books(&self) -> Vec<WorldBookCard> {
        self.inner
            .world_books()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn world_book(&self, id: String) -> Option<WorldBookDraftRecord> {
        self.inner
            .world_book(&id)
            .as_ref()
            .map(|book| WorldBookDraftRecord {
                id: book.id.clone(),
                name: book.name.clone(),
                entries: book.entries.iter().map(Into::into).collect(),
            })
    }

    pub fn save_world_book(&self, draft: WorldBookDraftRecord) -> Result<String, UniError> {
        self.inner
            .save_world_book(&draft.into())
            .map_err(Into::into)
    }

    pub fn delete_world_book(&self, id: String) -> Result<bool, UniError> {
        self.inner.delete_world_book(&id).map_err(Into::into)
    }

    // --- Library: prompt profiles -----------------------------------------

    pub fn list_prompt_profiles(&self) -> Vec<PromptProfileCard> {
        self.inner
            .prompt_profiles()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn prompt_profile(&self, id: String) -> Option<PromptProfileDraftRecord> {
        self.inner.prompt_profile(&id).as_ref().map(Into::into)
    }

    pub fn save_prompt_profile(&self, draft: PromptProfileDraftRecord) -> Result<String, UniError> {
        self.inner
            .save_prompt_profile(&draft.into())
            .map_err(Into::into)
    }

    pub fn delete_prompt_profile(&self, id: String) -> Result<bool, UniError> {
        self.inner.delete_prompt_profile(&id).map_err(Into::into)
    }

    // --- Readable context --------------------------------------------------

    /// The context sources a settings screen can offer as searchable material.
    pub async fn list_context_sources(&self) -> Result<Vec<ContextSourceRecord>, UniError> {
        self.inner
            .context_sources()
            .await
            .map(|sources| sources.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    // --- Diagnostics -------------------------------------------------------

    pub fn diagnostics(&self, limit: u32) -> Vec<DiagnosticRecord> {
        self.inner
            .diagnostics(limit as usize)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn discovery_diagnostics(&self, limit: u32) -> Vec<DiagnosticRecord> {
        self.inner
            .discovery_diagnostics(limit as usize)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn diagnostics_text(&self) -> String {
        self.inner.diagnostics_text()
    }

    pub fn clear_diagnostics(&self) {
        self.inner.clear_diagnostics();
    }
}
