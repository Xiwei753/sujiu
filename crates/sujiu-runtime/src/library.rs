//! The library as four independently managed resources.
//!
//! A character, a persona, a world book and a prompt profile are four different
//! things a user owns, not four tabs inside one settings page and not four
//! sub-objects of a character card. This module is the runtime side of that
//! split: it can list, read, create, edit and delete each kind on its own, and
//! separately change which of them a conversation binds.
//!
//! The separation is the whole point, so it is worth stating what each half
//! answers:
//!
//! - this module answers "what characters, personas, world books and prompt
//!   profiles do I have, and what does each one say?";
//! - the conversation answers "which of those does *this* chat use?", and stores
//!   nothing but ids.
//!
//! Nothing here decides that a character is "the" one, or that a world book
//! belongs to a card. A world book bound by three conversations is three
//! references to one stored entity, and editing it once changes all three.

use serde::{Deserialize, Serialize};
use sujiu_ai::project_library;
use sujiu_core::{
    Character, Participant, ParticipantRole, Persona, PromptProfile, WorldBook, WorldBookEntry,
    WorldBookPosition,
};

use crate::runtime::{CreateConversationRequest, SujiuRuntime, TurnError};

/// One persona, as a list row.
///
/// Only what a row needs. The full body is available from
/// [`SujiuRuntime::persona`], because a list of every persona's user prompt is
/// not a list.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    /// How many world books this persona brings of its own.
    ///
    /// A count and not the ids, for the same reason as everything else here:
    /// the binding ids belong to the conversation that chose them, and a persona
    /// is reusable, so listing them as if they were fixed would be a claim
    /// about ownership that is not true.
    pub worldbook_count: usize,
}

/// One world book entry, as a list row.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldBookEntrySummary {
    pub id: String,
    pub name: String,
    /// The entry's own text, unshortened.
    ///
    /// A world book is lore the model reads, so a preview here would be a
    /// truncation rule in the runtime, and choosing what counts as the
    /// interesting sentence of a piece of lore is a judgement no runtime should
    /// be making.
    pub content: String,
    pub keys: Vec<String>,
    pub enabled: bool,
    pub constant: bool,
    pub position: WorldBookPosition,
}

/// One world book, with its entries.
///
/// Entries travel with the book rather than behind a second call, because an
/// entry has no meaning outside the book that holds it: an editor showing a book
/// needs the whole book to save one entry back.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldBookSummary {
    pub id: String,
    pub name: String,
    pub entries: Vec<WorldBookEntrySummary>,
}

/// One prompt profile, as a list row.
///
/// The four prompt bodies are *not* here. A profile row is a name; the bodies
/// are what the profile says, and a list that carried all of them would be a
/// wall of text that also made the row itself impossible to read. Read one with
/// [`SujiuRuntime::prompt_profile`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptProfileSummary {
    pub id: String,
    pub name: String,
}

/// A persona to store.
///
/// `id` is optional so that "make me a new one" and "change this one" are the
/// same call: an absent or blank id means create, and the runtime allocates one.
/// Letting a frontend invent ids is how two entities end up sharing one and the
/// second silently overwrites the first.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_prompt: String,
    #[serde(default)]
    pub worldbook_ids: Vec<String>,
}

/// A character to store. Same id rule as [`PersonaRequest`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub first_message: String,
    #[serde(default)]
    pub alternate_greetings: Vec<String>,
    #[serde(default)]
    pub example_dialogue: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub post_history_instructions: String,
    #[serde(default)]
    pub worldbook_ids: Vec<String>,
}

/// One world-book entry to store.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldBookEntryRequest {
    /// Absent means "a new entry", and the runtime names it.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub keys: Vec<String>,
    /// Defaults to enabled, because an entry the user just wrote and cannot see
    /// is a worse default than one that works.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub constant: bool,
    #[serde(default)]
    pub position: WorldBookPosition,
}

/// A world book to store. Same id rule as [`PersonaRequest`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldBookRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub entries: Vec<WorldBookEntryRequest>,
}

/// A prompt profile to store. Same id rule as [`PersonaRequest`].
///
/// The four prompt bodies are the whole point of a profile, so they are stored
/// as written — including the empty ones. A profile whose system prompt was
/// cleared says "do not override the app prompt", which is different from a
/// profile that was never given one, and collapsing the two would make that
/// difference unexpressible.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptProfileRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub user_prompt: String,
    #[serde(default)]
    pub post_history_instructions: String,
    #[serde(default)]
    pub format_rules: String,
}

/// A new entry is enabled.
///
/// Implemented by hand rather than derived, because a derived `Default` would
/// disagree with the serde default above: a caller that builds a request in Rust
/// and one that arrives as JSON would then store two different things. An entry
/// the user just wrote and cannot see is worse than one that works, so both paths
/// have to land on the same answer.
impl Default for WorldBookEntryRequest {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            content: String::new(),
            keys: Vec::new(),
            enabled: true,
            constant: false,
            position: WorldBookPosition::default(),
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

/// Build a book's entries, naming the ones the caller did not name.
///
/// Entry ids have to be stable. A context record URI is built from one, so an id
/// that was regenerated on every save would leave the model holding references to
/// records that no longer exist. Names are therefore allocated once per request
/// and only for entries that arrived without one — an entry that came back from
/// the editor keeps the id it had.
fn build_entries(requests: &[WorldBookEntryRequest]) -> Vec<WorldBookEntry> {
    let mut taken: Vec<String> = requests
        .iter()
        .filter_map(|entry| requested_id(entry.id.as_deref()))
        .collect();

    requests
        .iter()
        .map(|entry| {
            let id = match requested_id(entry.id.as_deref()) {
                Some(id) => id,
                None => {
                    let mut number = taken.len() + 1;
                    let id = loop {
                        let candidate = format!("entry-{number}");
                        if !taken.contains(&candidate) {
                            break candidate;
                        }
                        number += 1;
                    };
                    taken.push(id.clone());
                    id
                }
            };

            WorldBookEntry {
                id,
                name: entry.name.clone(),
                content: entry.content.clone(),
                keys: entry.keys.clone(),
                enabled: entry.enabled,
                constant: entry.constant,
                position: entry.position,
                // Not part of a basic entry edit: priority orders lore inside a
                // book and stays whatever it was. A new entry starts at zero, the
                // same as one a decoder produces.
                priority: 0,
                extensions: serde_json::Map::new(),
            }
        })
        .collect()
}

/// Whether an id names an existing entity of that kind rather than a new one.
///
/// A blank id means create. Anything else is an edit, and editing an id that is
/// not there is an error rather than a silent create: a frontend that lost the
/// entity it was editing would otherwise write a second copy and tell the user
/// it saved.
fn requested_id(id: Option<&str>) -> Option<String> {
    id.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

impl SujiuRuntime {
    pub fn personas(&self) -> Vec<PersonaSummary> {
        let inner = self.inner.lock().unwrap();
        inner
            .library
            .personas
            .iter()
            .map(|persona| PersonaSummary {
                id: persona.id.clone(),
                name: persona.name.clone(),
                description: persona.description.clone(),
                worldbook_count: persona.worldbook_ids.len(),
            })
            .collect()
    }

    pub fn persona(&self, id: &str) -> Option<Persona> {
        self.inner.lock().unwrap().library.persona(id).cloned()
    }

    pub fn world_books(&self) -> Vec<WorldBookSummary> {
        let inner = self.inner.lock().unwrap();
        inner
            .library
            .world_books
            .iter()
            .map(|book| WorldBookSummary {
                id: book.id.clone(),
                name: book.name.clone(),
                entries: book
                    .entries
                    .iter()
                    .map(|entry| WorldBookEntrySummary {
                        id: entry.id.clone(),
                        name: entry.name.clone(),
                        content: entry.content.clone(),
                        keys: entry.keys.clone(),
                        enabled: entry.enabled,
                        constant: entry.constant,
                        position: entry.position,
                    })
                    .collect(),
            })
            .collect()
    }

    pub fn world_book(&self, id: &str) -> Option<WorldBook> {
        self.inner.lock().unwrap().library.world_book(id).cloned()
    }

    pub fn prompt_profiles(&self) -> Vec<PromptProfileSummary> {
        let inner = self.inner.lock().unwrap();
        inner
            .library
            .prompt_profiles
            .iter()
            .map(|profile| PromptProfileSummary {
                id: profile.id.clone(),
                name: profile.name.clone(),
            })
            .collect()
    }

    pub fn prompt_profile(&self, id: &str) -> Option<PromptProfile> {
        self.inner
            .lock()
            .unwrap()
            .library
            .prompt_profile(id)
            .cloned()
    }

    /// Store a persona and report the id it ended up under.
    ///
    /// Personas are projected into the searchable context store, so this has to
    /// reproject: editing the user prompt of a persona that is bound to a
    /// conversation must change what `search_context` and `read_context` return,
    /// not only what the next prompt compiles from.
    pub fn save_persona(&self, request: &PersonaRequest) -> Result<String, TurnError> {
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = self.resolve_persona_id(&mut inner.library, request)?;

            inner.library.upsert_persona(Persona {
                id: id.clone(),
                name: request.name.clone(),
                description: request.description.clone(),
                user_prompt: request.user_prompt.clone(),
                worldbook_ids: request.worldbook_ids.clone(),
                ..Persona::default()
            });

            id
        };

        self.reproject()?;
        self.persist()
            .map_err(|error| TurnError::Storage(error.to_string()))?;
        Ok(id)
    }

    /// Store a world book and report the id it ended up under.
    ///
    /// Reprojects for the same reason [`SujiuRuntime::save_persona`] does, and
    /// for a stronger one: a world book's entries *are* lore the model reads
    /// through the context tools.
    pub fn save_world_book(&self, request: &WorldBookRequest) -> Result<String, TurnError> {
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let existing = self.resolve_world_book_id(&mut inner.library, request)?;
            let id = match existing {
                Some(id) => id,
                None => inner.library.next_world_book_id(),
            };

            let entries = build_entries(&request.entries);

            inner.library.upsert_world_book(WorldBook {
                id: id.clone(),
                name: request.name.clone(),
                entries,
                ..WorldBook::default()
            });

            id
        };

        self.reproject()?;
        self.persist()
            .map_err(|error| TurnError::Storage(error.to_string()))?;
        Ok(id)
    }

    pub fn save_character(&self, request: &CharacterRequest) -> Result<String, TurnError> {
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = match requested_id(request.id.as_deref()) {
                Some(id) => {
                    if inner.library.character(&id).is_none() {
                        return Err(TurnError::EntityNotFound(id));
                    }
                    id
                }
                None => inner.library.next_character_id(),
            };

            inner.library.upsert_character(Character {
                id: id.clone(),
                name: request.name.clone(),
                description: request.description.clone(),
                personality: request.personality.clone(),
                scenario: request.scenario.clone(),
                first_message: request.first_message.clone(),
                alternate_greetings: request.alternate_greetings.clone(),
                example_dialogue: request.example_dialogue.clone(),
                system_prompt: request.system_prompt.clone(),
                post_history_instructions: request.post_history_instructions.clone(),
                worldbook_ids: request.worldbook_ids.clone(),
                ..Character::default()
            });

            id
        };

        self.persist()
            .map_err(|error| TurnError::Storage(error.to_string()))?;
        Ok(id)
    }

    pub fn save_prompt_profile(&self, request: &PromptProfileRequest) -> Result<String, TurnError> {
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = match requested_id(request.id.as_deref()) {
                Some(id) => {
                    if inner.library.prompt_profile(&id).is_none() {
                        return Err(TurnError::EntityNotFound(id));
                    }
                    id
                }
                None => inner.library.next_prompt_profile_id(),
            };

            // The stored profile is edited field by field rather than replaced,
            // because `segments` is not part of this request. Replacing would make
            // saving a name from a simple editor delete every fixed segment the
            // profile had, silently and without any field having been cleared.
            let existing = inner.library.prompt_profile(&id).cloned();
            let mut profile =
                existing.unwrap_or_else(|| PromptProfile::new(id.clone(), String::new()));
            profile.name = request.name.clone();
            profile.system_prompt = request.system_prompt.clone();
            profile.user_prompt = request.user_prompt.clone();
            profile.post_history_instructions = request.post_history_instructions.clone();
            profile.format_rules = request.format_rules.clone();

            inner.library.upsert_prompt_profile(profile);
            id
        };

        self.persist()
            .map_err(|error| TurnError::Storage(error.to_string()))?;
        Ok(id)
    }

    /// Remove a resource and report whether there was one.
    ///
    /// The bindings that named it are cleaned up by the library, not left
    /// dangling: see [`sujiu_core::Library::remove_character`] and its siblings.
    pub fn delete_character(&self, id: &str) -> Result<bool, TurnError> {
        self.delete_from(|library| library.remove_character(id), false)
    }

    pub fn delete_persona(&self, id: &str) -> Result<bool, TurnError> {
        self.delete_from(|library| library.remove_persona(id), true)
    }

    pub fn delete_world_book(&self, id: &str) -> Result<bool, TurnError> {
        self.delete_from(|library| library.remove_world_book(id), true)
    }

    pub fn delete_prompt_profile(&self, id: &str) -> Result<bool, TurnError> {
        self.delete_from(|library| library.remove_prompt_profile(id), false)
    }

    /// Replace the participants, persona, world books and prompt profile of a
    /// conversation.
    ///
    /// This is the conversation half of the split, and it is deliberately not a
    /// resource operation: it changes what one chat *uses*, and it never touches
    /// the entities themselves. The same persona bound here and in another
    /// conversation is still one persona.
    ///
    /// The transcript is not touched. Changing who is in a room is not the same
    /// as rewriting what has already been said in it.
    pub fn set_conversation_bindings(
        &self,
        session_id: &str,
        request: &CreateConversationRequest,
    ) -> Result<(), TurnError> {
        {
            let mut inner = self.inner.lock().unwrap();
            let Some(conversation) = inner.library.conversation_mut(session_id) else {
                return Err(TurnError::SessionNotFound(session_id.to_owned()));
            };

            conversation.participants = request
                .participants
                .iter()
                .map(|participant| Participant {
                    character_id: participant.character_id.clone(),
                    role: participant.role.unwrap_or(ParticipantRole::Character),
                    display_name: participant.display_name.clone(),
                })
                .collect();
            conversation.persona_id = request.persona_id.clone();
            conversation.worldbook_ids = request.worldbook_ids.clone();
            conversation.prompt_profile_id = request.prompt_profile_id.clone();
        }

        self.persist()
            .map_err(|error| TurnError::Storage(error.to_string()))?;
        Ok(())
    }

    fn resolve_persona_id(
        &self,
        library: &mut sujiu_core::Library,
        request: &PersonaRequest,
    ) -> Result<String, TurnError> {
        match requested_id(request.id.as_deref()) {
            Some(id) if library.persona(&id).is_none() => Err(TurnError::EntityNotFound(id)),
            Some(id) => Ok(id),
            None => Ok(library.next_persona_id()),
        }
    }

    fn resolve_world_book_id(
        &self,
        library: &mut sujiu_core::Library,
        request: &WorldBookRequest,
    ) -> Result<Option<String>, TurnError> {
        match requested_id(request.id.as_deref()) {
            Some(id) if library.world_book(&id).is_none() => Err(TurnError::EntityNotFound(id)),
            Some(id) => Ok(Some(id)),
            None => Ok(None),
        }
    }

    /// Rebuild the searchable view from the stored entities.
    ///
    /// The context store holds both its own sources and the projection of the
    /// library, so reprojecting means clearing the projected half and leaving
    /// the rest alone — otherwise a deleted world book would stay searchable
    /// forever, and an edited persona would keep answering with its old text.
    fn reproject(&self) -> Result<(), TurnError> {
        let inner = self.inner.lock().unwrap();
        let (sources, records) = inner.store.snapshot();
        let (mut sources, mut records) = (sources, records);
        // The same filter `persist` writes with, in the other direction: keep
        // what the library owns as sources, drop what is only a projection of an
        // entity, so the entities below can be projected again from scratch.
        sources.retain(|source| !crate::documents::is_projected(&source.id));
        records.retain(|record| !crate::documents::is_projected(&record.source_id));

        inner.store.clear();
        for source in sources {
            inner.store.add_source(source);
        }
        for record in records {
            inner.store.add_record(record);
        }
        project_library(&inner.store, &inner.library);

        Ok(())
    }

    fn delete_from(
        &self,
        remove: impl FnOnce(&mut sujiu_core::Library) -> bool,
        reprojects: bool,
    ) -> Result<bool, TurnError> {
        let removed = {
            let mut inner = self.inner.lock().unwrap();
            remove(&mut inner.library)
        };

        if !removed {
            // Nothing changed, so there is nothing to write or reproject.
            return Ok(false);
        }

        if reprojects {
            self.reproject()?;
        }
        self.persist()
            .map_err(|error| TurnError::Storage(error.to_string()))?;
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ParticipantRequest;
    use crate::seed;

    fn runtime() -> SujiuRuntime {
        SujiuRuntime::new(seed::seed()).expect("runtime")
    }

    #[test]
    fn each_kind_is_listed_on_its_own_rather_than_as_one_catalog() {
        let runtime = runtime();

        // The seed holds one of each, which is exactly the shape the frontend
        // split has to cope with: four lists, not one.
        assert_eq!(runtime.personas().len(), 1);
        assert_eq!(runtime.personas()[0].id, "persona-insomniac");
        assert_eq!(runtime.world_books().len(), 1);
        assert_eq!(runtime.world_books()[0].entries.len(), 3);
        assert_eq!(runtime.prompt_profiles().len(), 1);
        assert_eq!(runtime.prompt_profiles()[0].id, "profile-roleplay");

        // A prompt profile row is a name. Carrying its four bodies in a list
        // would make the row unreadable, and it is not what the runtime is for.
        let json = serde_json::to_string(&runtime.prompt_profiles()[0]).expect("summary");
        assert!(!json.contains("formatRules"), "{json}");
    }

    #[test]
    fn creating_a_resource_allocates_its_own_id() {
        let runtime = runtime();

        let persona = runtime
            .save_persona(&PersonaRequest {
                name: "The archivist".into(),
                description: "Reads everything twice.".into(),
                ..PersonaRequest::default()
            })
            .expect("saved");
        assert!(!persona.is_empty(), "the caller needs an id to bind");
        assert_eq!(runtime.personas().len(), 2);
        assert_eq!(
            runtime.persona(&persona).expect("readable back").name,
            "The archivist"
        );

        let book = runtime
            .save_world_book(&WorldBookRequest {
                name: "The stacks".into(),
                entries: vec![WorldBookEntryRequest {
                    name: "Shelf nine".into(),
                    content: "Water damage reaches shelf nine first.".into(),
                    keys: vec!["shelf nine".into()],
                    ..WorldBookEntryRequest::default()
                }],
                ..WorldBookRequest::default()
            })
            .expect("saved");
        assert_eq!(runtime.world_books().len(), 2);
        let stored = runtime.world_book(&book).expect("readable back");
        assert_eq!(stored.entries.len(), 1);
        // The runtime names an unnamed entry rather than refusing it.
        assert!(!stored.entries[0].id.is_empty());
        assert!(stored.entries[0].enabled, "a new entry works by default");

        let profile = runtime
            .save_prompt_profile(&PromptProfileRequest {
                name: "Direct".into(),
                format_rules: "Answer in one paragraph.".into(),
                ..PromptProfileRequest::default()
            })
            .expect("saved");
        assert_eq!(runtime.prompt_profiles().len(), 2);
        assert_eq!(
            runtime
                .prompt_profile(&profile)
                .expect("readable back")
                .format_rules,
            "Answer in one paragraph."
        );
    }

    #[test]
    fn an_entry_id_survives_an_edit_so_its_context_uri_still_resolves() {
        let runtime = runtime();
        let book_id = runtime.world_books()[0].id.clone();
        let first_entry = runtime.world_book(&book_id).expect("book").entries[0]
            .id
            .clone();

        // Read it back the way an editor would, change one thing, and save.
        let mut book = runtime.world_book(&book_id).expect("book");
        book.entries[0].content = "Rewritten.".into();
        let request = WorldBookRequest {
            id: Some(book_id.clone()),
            name: book.name.clone(),
            entries: book
                .entries
                .iter()
                .map(|entry| WorldBookEntryRequest {
                    id: Some(entry.id.clone()),
                    name: entry.name.clone(),
                    content: entry.content.clone(),
                    keys: entry.keys.clone(),
                    enabled: entry.enabled,
                    constant: entry.constant,
                    position: entry.position,
                })
                .collect(),
        };

        runtime.save_world_book(&request).expect("saved");

        let stored = runtime.world_book(&book_id).expect("book");
        assert_eq!(
            stored.entries[0].id, first_entry,
            "a regenerated id would leave the model holding a dead record URI"
        );
        assert_eq!(stored.entries[0].content, "Rewritten.");
    }

    #[test]
    fn editing_a_resource_that_is_not_there_is_an_error_not_a_second_copy() {
        let runtime = runtime();

        let error = runtime
            .save_persona(&PersonaRequest {
                id: Some("persona-vanished".into()),
                name: "Ghost".into(),
                ..PersonaRequest::default()
            })
            .expect_err("there is no such persona");
        assert!(error.to_string().contains("entity_not_found"), "{error}");
        assert_eq!(
            runtime.personas().len(),
            1,
            "a refused save must not have created anything"
        );
    }

    #[test]
    fn editing_a_prompt_profile_does_not_delete_its_fixed_segments() {
        let runtime = runtime();
        let id = runtime.prompt_profiles()[0].id.clone();

        // Give it a segment first, through the entity the runtime stores.
        {
            let mut inner = runtime.inner.lock().unwrap();
            let profile = inner.library.prompt_profile_mut(&id).expect("profile");
            profile.segments = vec![sujiu_core::PromptProfileSegment {
                id: "segment-1".into(),
                name: "Safety".into(),
                content: "Refuse to describe violence in detail.".into(),
                role: sujiu_core::ChatRole::System,
                position: sujiu_core::PromptPosition::Prefix,
                extensions: serde_json::Map::new(),
            }];
        }

        // Then save only the named fields, the way a simple editor can.
        runtime
            .save_prompt_profile(&PromptProfileRequest {
                id: Some(id.clone()),
                name: "Roleplay (revised)".into(),
                format_rules: "Third person.".into(),
                ..PromptProfileRequest::default()
            })
            .expect("saved");

        let stored = runtime.prompt_profile(&id).expect("readable back");
        assert_eq!(stored.name, "Roleplay (revised)");
        assert_eq!(
            stored.segments.len(),
            1,
            "saving a name must not silently drop a fixed prompt segment"
        );
    }

    #[test]
    fn deleting_a_resource_clears_the_bindings_that_named_it() {
        let runtime = runtime();

        let persona = runtime
            .save_persona(&PersonaRequest {
                name: "Temporary".into(),
                ..PersonaRequest::default()
            })
            .expect("saved");
        let session = runtime.create_session(None);
        runtime
            .set_conversation_bindings(
                &session,
                &CreateConversationRequest {
                    participants: vec![ParticipantRequest {
                        character_id: "character-lin".into(),
                        role: None,
                        display_name: None,
                    }],
                    persona_id: Some(persona.clone()),
                    worldbook_ids: vec!["world-book-coast".into()],
                    prompt_profile_id: Some("profile-roleplay".into()),
                    ..CreateConversationRequest::default()
                },
            )
            .expect("bound");

        let before = runtime.conversation_state(&session).expect("state");
        assert_eq!(before.persona_id.as_deref(), Some(persona.as_str()));

        assert!(runtime.delete_persona(&persona).expect("deleted"));
        assert!(runtime
            .delete_world_book("world-book-coast")
            .expect("deleted"));
        assert!(runtime
            .delete_prompt_profile("profile-roleplay")
            .expect("deleted"));

        let after = runtime.conversation_state(&session).expect("state");
        assert!(
            after.persona_id.is_none(),
            "a deleted persona left a dangling id"
        );
        assert!(
            after.worldbook_ids.is_empty(),
            "a deleted world book left a dangling id"
        );
        assert!(
            after.prompt_profile_id.is_none(),
            "a deleted profile left a dangling id"
        );
        // The participant was not deleted, so the conversation still has one.
        assert_eq!(after.participants.len(), 1);
    }

    #[test]
    fn deleting_something_that_is_not_there_reports_no_change() {
        let runtime = runtime();
        assert!(!runtime
            .delete_character("character-nobody")
            .expect("no error"));
        assert_eq!(runtime.characters("").len(), 3, "the seed is untouched");
    }

    /// A conversation binds references and owns no copies.
    ///
    /// If binding copied a persona into the conversation, "the same persona in
    /// two chats" would quietly become two personas, and editing one would stop
    /// affecting the other — which is the exact confusion this split exists to
    /// remove.
    #[test]
    fn a_binding_is_a_reference_and_the_resource_is_shared() {
        let runtime = runtime();

        let first = runtime.create_session(None);
        let second = runtime.create_session(None);
        for session in [&first, &second] {
            runtime
                .set_conversation_bindings(
                    session,
                    &CreateConversationRequest {
                        persona_id: Some("persona-insomniac".into()),
                        ..CreateConversationRequest::default()
                    },
                )
                .expect("bound");
        }

        runtime
            .save_persona(&PersonaRequest {
                id: Some("persona-insomniac".into()),
                name: "The insomniac".into(),
                description: "Now also a radio operator.".into(),
                user_prompt: "Keep replies short.".into(),
                ..PersonaRequest::default()
            })
            .expect("saved once");

        assert_eq!(runtime.personas().len(), 1, "one edit, one entity");
        for session in [&first, &second] {
            let state = runtime.conversation_state(session).expect("state");
            assert_eq!(state.persona_id.as_deref(), Some("persona-insomniac"));
        }
        assert_eq!(
            runtime
                .persona("persona-insomniac")
                .expect("persona")
                .description,
            "Now also a radio operator."
        );
    }

    #[test]
    fn changing_the_bindings_leaves_the_transcript_alone() {
        let runtime = runtime();
        let session = "session-1";
        let before = runtime.conversation_state(session).expect("state");
        let messages_before = before.messages.len();

        runtime
            .set_conversation_bindings(
                session,
                &CreateConversationRequest {
                    participants: vec![ParticipantRequest {
                        character_id: "character-wen".into(),
                        role: None,
                        display_name: None,
                    }],
                    ..CreateConversationRequest::default()
                },
            )
            .expect("bound");

        let after = runtime.conversation_state(session).expect("state");
        assert_eq!(after.messages.len(), messages_before);
        assert_eq!(after.participants[0].character_id, "character-wen");
    }

    #[test]
    fn binding_a_conversation_that_is_not_there_says_so() {
        let runtime = runtime();
        let error = runtime
            .set_conversation_bindings("conversation-nobody", &CreateConversationRequest::default())
            .expect_err("there is no such conversation");
        assert!(error.to_string().contains("session_not_found"), "{error}");
    }

    /// Reprojection is the part of editing a resource that is easy to leave out.
    ///
    /// A persona and a world book reach the model through the context tools as
    /// well as through the prompt. If an edit changed the stored entity but not
    /// the projection, the model would answer with the old text while the
    /// library showed the new one, and nothing anywhere would say so.
    #[test]
    fn editing_a_resource_changes_what_the_context_tools_would_return() {
        use sujiu_ai::{ContextSearchQuery, ContextStore};

        let runtime = runtime();

        let search = |rt: &SujiuRuntime, query: &str| {
            let store = rt.inner.lock().unwrap().store.clone();
            rt.tokio
                .block_on(store.search(&ContextSearchQuery {
                    query: query.to_string(),
                    limit: 20,
                    ..ContextSearchQuery::default()
                }))
                .expect("the in-memory store does not fail")
        };

        // The seeded persona's own text is searchable before anything changes.
        assert!(
            !search(&runtime, "bored").is_empty(),
            "the seed projects personas"
        );

        runtime
            .save_persona(&PersonaRequest {
                id: Some("persona-insomniac".into()),
                name: "The insomniac".into(),
                description: "Now a lighthouse keeper.".into(),
                user_prompt: "Speak only in weather reports.".into(),
                ..PersonaRequest::default()
            })
            .expect("saved");

        assert!(
            !search(&runtime, "lighthouse").is_empty(),
            "the edited persona is not in the projection"
        );
        assert!(
            search(&runtime, "bored").is_empty(),
            "the old persona text is still searchable after an edit"
        );

        // And deleting it has to take the projection with it, or a persona the
        // user deleted stays available to the model.
        runtime
            .delete_persona("persona-insomniac")
            .expect("deleted");
        assert!(
            search(&runtime, "lighthouse").is_empty(),
            "a deleted persona is still searchable"
        );
    }

    /// The projection is a view, so saving a resource must not leave the
    /// library's own records in the document twice.
    #[test]
    fn a_saved_world_book_survives_a_reopen_without_growing_the_store() {
        let dir = std::env::temp_dir().join(format!(
            "sujiu-library-reopen-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");

        {
            let runtime = SujiuRuntime::new_persistent(
                seed::seed(),
                std::sync::Arc::new(crate::storage::FileStorage::new(&dir).expect("storage")),
            )
            .expect("runtime");
            runtime
                .save_world_book(&WorldBookRequest {
                    name: "The stacks".into(),
                    entries: vec![WorldBookEntryRequest {
                        name: "Shelf nine".into(),
                        content: "Water damage reaches shelf nine first.".into(),
                        keys: vec!["shelf nine".into()],
                        ..WorldBookEntryRequest::default()
                    }],
                    ..WorldBookRequest::default()
                })
                .expect("saved");
            let session = runtime.create_session(Some("character-lin"));
            runtime
                .set_conversation_bindings(
                    &session,
                    &CreateConversationRequest {
                        persona_id: Some("persona-insomniac".into()),
                        worldbook_ids: vec!["world-book-coast".into()],
                        ..CreateConversationRequest::default()
                    },
                )
                .expect("bound");
        }

        let reopened = SujiuRuntime::new_persistent(
            seed::seed(),
            std::sync::Arc::new(crate::storage::FileStorage::new(&dir).expect("storage")),
        )
        .expect("runtime");

        let books = reopened.world_books();
        assert_eq!(books.len(), 2, "the seeded book and the saved one");
        let saved = books
            .iter()
            .find(|book| book.name == "The stacks")
            .expect("the saved book survived");
        assert_eq!(saved.entries.len(), 1);
        assert_eq!(saved.entries[0].keys, vec!["shelf nine".to_string()]);

        let session = reopened.sessions()[0].id.clone();
        let state = reopened.conversation_state(&session).expect("state");
        assert_eq!(state.persona_id.as_deref(), Some("persona-insomniac"));
        assert_eq!(state.worldbook_ids, vec!["world-book-coast".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
