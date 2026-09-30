//! How domain entities are written to a platform's storage.
//!
//! The layout is by domain, and a conversation owns its own directory:
//!
//! ```text
//! sujiu-library.json                      manifest + endpoint + context store
//! conversations/<conversation-id>/conversation.json
//! characters/<character-id>.json
//! personas/<persona-id>.json
//! worldbooks/<world-book-id>.json
//! prompt_profiles/<profile-id>.json
//! ```
//!
//! The direction matters. It used to be `characters/<id>/chats/...`, which made
//! a conversation a child of one card. Characters, personas, world books and
//! prompt profiles are independent resources that a conversation *binds* by id,
//! so nothing is stored under a character's directory, and a conversation is the
//! unit a transcript is filed under.
//!
//! Two things live outside the five domain collections and are said so here:
//! the endpoint configuration and the context store. Neither is a domain
//! entity, and the context store is a projection (see `sujiu_ai::project_library`),
//! so persisting it must never be what keeps a character or a world book alive.
//! Projected sources are therefore filtered back out on the way out and rebuilt
//! from the domain documents on the way in.

use serde::{Deserialize, Serialize};
use sujiu_core::{
    split_embedded_world_book, Character, ContextRecord, ContextSource, Conversation,
    EndpointConfig, Library, Transcript, WorldBook,
};

use crate::storage::AppStorage;

/// The manifest. Names every entity document, so loading never has to scan a
/// directory, and carries the two things that are not domain entities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIndex {
    /// Bumped when the document shape changes incompatibly.
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub endpoint: Option<EndpointConfig>,
    #[serde(default)]
    pub global_worldbook_ids: Vec<String>,
    #[serde(default)]
    pub conversation_ids: Vec<String>,
    #[serde(default)]
    pub character_ids: Vec<String>,
    #[serde(default)]
    pub persona_ids: Vec<String>,
    #[serde(default)]
    pub world_book_ids: Vec<String>,
    #[serde(default)]
    pub prompt_profile_ids: Vec<String>,
    /// The context store, minus everything projected out of a domain document.
    #[serde(default)]
    pub sources: Vec<ContextSource>,
    #[serde(default)]
    pub records: Vec<ContextRecord>,
}

/// The version this build writes.
pub const LIBRARY_VERSION: u32 = 1;

/// The manifest's name.
pub const LIBRARY_FILE: &str = "sujiu-library.json";

/// The single document versions 1 to 3 kept everything in.
///
/// Read once, to migrate it. It is never written again: one file for every chat
/// cannot hold a conversation per directory, and it is the shape that made a
/// character the root of a store.
pub const LEGACY_SNAPSHOT_FILE: &str = "sujiu-runtime.json";

/// Source-id namespaces owned by the projection rather than by stored data.
const PROJECTED_PREFIXES: [&str; 2] = ["worldbook:", "persona:"];

pub fn conversation_key(id: &str) -> String {
    format!("conversations/{id}/conversation.json")
}

pub fn character_key(id: &str) -> String {
    format!("characters/{id}.json")
}

pub fn persona_key(id: &str) -> String {
    format!("personas/{id}.json")
}

pub fn world_book_key(id: &str) -> String {
    format!("worldbooks/{id}.json")
}

pub fn prompt_profile_key(id: &str) -> String {
    format!("prompt_profiles/{id}.json")
}

/// Whether a source id belongs to the projection of a domain entity.
///
/// A record under such an id is not stored data: the character, persona or world
/// book it came from is stored as its own document, and this is a view of it.
/// Saving the view as well would make the projection the second source of truth,
/// which is exactly what the split exists to prevent.
pub fn is_projected(source_id: &str) -> bool {
    PROJECTED_PREFIXES
        .iter()
        .any(|prefix| source_id.starts_with(prefix))
}

/// A store read back, with the manifest and the entities it named.
#[derive(Debug, Default)]
pub struct LoadedLibrary {
    pub library: Library,
    pub index: LibraryIndex,
}

/// Read the per-entity documents the manifest names.
///
/// `None` means the manifest is there and could not be read, which the caller
/// has to treat as "do not write": a store whose shape this build does not know
/// is a store whose contents are still worth keeping.
pub fn load(storage: &dyn AppStorage) -> Option<LoadedLibrary> {
    let manifest = storage.load(LIBRARY_FILE)?;
    let index: LibraryIndex = serde_json::from_str(&manifest).ok()?;

    let mut library = Library {
        global_worldbook_ids: index.global_worldbook_ids.clone(),
        ..Library::default()
    };

    for id in &index.conversation_ids {
        library
            .conversations
            .push(read_entity(storage, &conversation_key(id))?);
    }
    for id in &index.character_ids {
        library
            .characters
            .push(read_entity(storage, &character_key(id))?);
    }
    for id in &index.persona_ids {
        library
            .personas
            .push(read_entity(storage, &persona_key(id))?);
    }
    for id in &index.world_book_ids {
        library
            .world_books
            .push(read_entity(storage, &world_book_key(id))?);
    }
    for id in &index.prompt_profile_ids {
        library
            .prompt_profiles
            .push(read_entity(storage, &prompt_profile_key(id))?);
    }

    Some(LoadedLibrary { library, index })
}

fn read_entity<T: serde::de::DeserializeOwned>(storage: &dyn AppStorage, key: &str) -> Option<T> {
    let document = storage.load(key)?;
    serde_json::from_str(&document).ok()
}

/// Write every entity as its own document, then the manifest that names them.
///
/// The manifest is written last on purpose. It is the only document a load
/// starts from, so a run interrupted part way through leaves a store that still
/// reads exactly what it read before, instead of a manifest naming documents
/// that were never written.
pub fn save(
    storage: &dyn AppStorage,
    library: &Library,
    index: &LibraryIndex,
) -> std::io::Result<()> {
    let mut index = index.clone();
    index.version = LIBRARY_VERSION;
    index.global_worldbook_ids = library.global_worldbook_ids.clone();
    index.conversation_ids = library
        .conversations
        .iter()
        .map(|item| item.id.clone())
        .collect();
    index.character_ids = library
        .characters
        .iter()
        .map(|item| item.id.clone())
        .collect();
    index.persona_ids = library
        .personas
        .iter()
        .map(|item| item.id.clone())
        .collect();
    index.world_book_ids = library
        .world_books
        .iter()
        .map(|item| item.id.clone())
        .collect();
    index.prompt_profile_ids = library
        .prompt_profiles
        .iter()
        .map(|item| item.id.clone())
        .collect();
    index.sources.retain(|source| !is_projected(&source.id));
    index
        .records
        .retain(|record| !is_projected(&record.source_id));

    for item in &library.conversations {
        write_entity(storage, &conversation_key(&item.id), item)?;
    }
    for item in &library.characters {
        write_entity(storage, &character_key(&item.id), item)?;
    }
    for item in &library.personas {
        write_entity(storage, &persona_key(&item.id), item)?;
    }
    for item in &library.world_books {
        write_entity(storage, &world_book_key(&item.id), item)?;
    }
    for item in &library.prompt_profiles {
        write_entity(storage, &prompt_profile_key(&item.id), item)?;
    }

    let document = serde_json::to_string(&index).map_err(invalid_data)?;
    storage.save(LIBRARY_FILE, &document);
    Ok(())
}

fn write_entity<T: Serialize>(
    storage: &dyn AppStorage,
    key: &str,
    entity: &T,
) -> std::io::Result<()> {
    let document = serde_json::to_string(entity).map_err(invalid_data)?;
    storage.save(key, &document);
    Ok(())
}

fn invalid_data(error: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
}

/// Turn a version 1 to 3 document into the domain entities it described.
///
/// The two migrations that matter are both about not losing anything:
///
/// - a session became a conversation with a single participant, and it keeps
///   its id. Renaming it would be tidier and would also break every row a
///   platform had already stored, so the id survives the move;
/// - a world book embedded in a character card became an independent world book,
///   and the card was left referencing it. The card is where the book was found,
///   so the book is given an id derived from the card, and the card's default
///   bindings now name it — otherwise a migration would look successful and then
///   leave a conversation with no lore at all.
///
/// A document from a *newer* version than this build knows is not migrated. It
/// reads as unreadable, which is what keeps it safe.
pub fn migrate_legacy_document(document: &str) -> Option<LoadedLibrary> {
    let value: serde_json::Value = serde_json::from_str(document).ok()?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);

    // Version 2 wrote its endpoint under `providerConfig` and version 3 under
    // `endpoint`; both are read. A version 2 document read as version 3 would
    // find no endpoint at all and quietly look like a fresh install.
    match version {
        0 | 1 => {
            let legacy: LegacySnapshot = serde_json::from_value(value).ok()?;
            let (characters, world_books) = split_world_books(legacy.characters);

            Some(LoadedLibrary {
                library: Library {
                    conversations: legacy
                        .sessions
                        .into_iter()
                        .map(flat_session_into_conversation)
                        .collect(),
                    characters,
                    world_books,
                    ..Library::default()
                },
                index: LibraryIndex {
                    version: LIBRARY_VERSION,
                    endpoint: legacy
                        .provider_config
                        .map(LegacyEndpointConfig::into_endpoint),
                    sources: legacy.sources,
                    records: legacy.records,
                    ..LibraryIndex::default()
                },
            })
        }
        2 | 3 => {
            let stored: StoredSnapshot = serde_json::from_value(value).ok()?;
            let (characters, world_books) = split_world_books(stored.characters);

            Some(LoadedLibrary {
                library: Library {
                    conversations: stored
                        .sessions
                        .into_iter()
                        .map(stored_session_into_conversation)
                        .collect(),
                    characters,
                    world_books,
                    ..Library::default()
                },
                index: LibraryIndex {
                    version: LIBRARY_VERSION,
                    endpoint: stored.endpoint.or_else(|| {
                        stored
                            .provider_config
                            .map(LegacyEndpointConfig::into_endpoint)
                    }),
                    sources: stored.sources,
                    records: stored.records,
                    ..LibraryIndex::default()
                },
            })
        }
        _ => None,
    }
}

/// Read a world book out of every card that still carries one, and leave the
/// card referencing it.
///
/// The cards are read from their stored JSON rather than from a `Character`,
/// because `Character` no longer has the embedded field and serde ignores what
/// it does not know. Reading the typed struct first is how a migration could
/// succeed and still drop every line of lore the user had.
fn split_world_books(stored: Vec<serde_json::Value>) -> (Vec<Character>, Vec<WorldBook>) {
    let mut characters = Vec::with_capacity(stored.len());
    let mut books = Vec::new();

    for mut value in stored {
        let owner = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();

        let embedded = split_embedded_world_book(&mut value, &owner);
        let Ok(mut character) = serde_json::from_value::<Character>(value) else {
            continue;
        };

        if let Some(book) = embedded {
            let id = book.id.clone();
            if !character.worldbook_ids.contains(&id) {
                character.worldbook_ids.insert(0, id);
            }
            books.push(book);
        }

        characters.push(character);
    }

    (characters, books)
}

/// A session as version 1 stored it: a flat list of text messages.
///
/// Reading one of these is a migration, not a normal load. The old shape had
/// no place to record a tool call, so a migration can restore the turn/step
/// structure but never the steps that were already lost.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySession {
    id: String,
    #[serde(default)]
    character_id: Option<String>,
    #[serde(default)]
    messages: Vec<sujiu_core::ChatMessage>,
    #[serde(default)]
    metadata: serde_json::Map<String, serde_json::Value>,
}

/// Version 1 of the document, read only to migrate it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySnapshot {
    #[serde(default)]
    characters: Vec<serde_json::Value>,
    #[serde(default)]
    sessions: Vec<LegacySession>,
    #[serde(default)]
    sources: Vec<ContextSource>,
    #[serde(default)]
    records: Vec<ContextRecord>,
    #[serde(default)]
    provider_config: Option<LegacyEndpointConfig>,
}

/// Version 2 and 3 of the document.
///
/// Their sessions were already real transcripts, with the tool calls and results
/// the older flat shape had nowhere to put, so they are read as conversations
/// with every turn intact. Running them through the version 1 migration instead
/// would rebuild them from text and drop every tool call — and the unreadable
/// continuation event inside one of those turns would be dropped with it, which
/// is how a store can lose a conversation without ever saying so.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    #[serde(default)]
    characters: Vec<serde_json::Value>,
    #[serde(default)]
    sessions: Vec<StoredSession>,
    #[serde(default)]
    sources: Vec<ContextSource>,
    #[serde(default)]
    records: Vec<ContextRecord>,
    #[serde(default)]
    provider_config: Option<LegacyEndpointConfig>,
    #[serde(default)]
    endpoint: Option<EndpointConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSession {
    id: String,
    #[serde(default)]
    character_id: Option<String>,
    #[serde(default)]
    transcript: Transcript,
    #[serde(default)]
    metadata: serde_json::Map<String, serde_json::Value>,
}

/// The single participant an old session had.
///
/// A session named at most one character, and that is the whole of what the
/// migration can honestly claim: it is not a group, and it is not a primary
/// character either — it is the one participant the document recorded.
fn sole_participant(character_id: Option<String>) -> Vec<sujiu_core::Participant> {
    character_id
        .filter(|id| !id.trim().is_empty())
        .into_iter()
        .map(sujiu_core::Participant::character)
        .collect()
}

fn flat_session_into_conversation(session: LegacySession) -> Conversation {
    use sujiu_core::{AssistantStep, ChatRole, Turn, TurnState};

    let mut transcript = Transcript::default();

    for message in session.messages {
        let at_ms = message
            .metadata
            .get("atMs")
            .and_then(serde_json::Value::as_i64);

        match message.role {
            // A user message opens a turn. Reusing the message id as the turn id
            // keeps the ids a platform already stored recognizable.
            ChatRole::User => transcript.push(Turn {
                id: message.id,
                user: message.content,
                steps: Vec::new(),
                state: TurnState::Completed,
                created_at_ms: at_ms,
            }),
            ChatRole::Assistant => {
                let step = AssistantStep::text_only(message.content);
                match transcript.turns.last_mut() {
                    Some(turn) => {
                        turn.steps.push(step);
                        turn.created_at_ms = turn.created_at_ms.or(at_ms);
                    }
                    // An assistant message with no turn before it is kept, so a
                    // migration never silently drops history.
                    None => {
                        let mut turn = Turn::new(message.id, String::new());
                        turn.created_at_ms = at_ms;
                        turn.steps.push(step);
                        transcript.push(turn);
                    }
                }
            }
            ChatRole::System | ChatRole::Developer => {}
        }
    }

    Conversation {
        id: session.id.clone(),
        participants: sole_participant(session.character_id),
        transcript,
        metadata: session.metadata,
        ..Conversation::default()
    }
}

fn stored_session_into_conversation(session: StoredSession) -> Conversation {
    Conversation {
        id: session.id.clone(),
        participants: sole_participant(session.character_id),
        transcript: session.transcript,
        metadata: session.metadata,
        ..Conversation::default()
    }
}

/// The endpoint as versions 1 and 2 of the document wrote it.
///
/// It had a vendor `kind` and a `model`. Both are read here and dropped on the
/// way in: the kind never decided anything worth keeping, and the model becomes
/// the selected one. Reading it through `EndpointConfig` instead would fail,
/// because an old file is exactly the kind of document this build has to keep
/// opening without losing the user's conversations.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyEndpointConfig {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    credential_ref: Option<String>,
    #[serde(default)]
    extra: serde_json::Map<String, serde_json::Value>,
}

impl LegacyEndpointConfig {
    fn into_endpoint(self) -> EndpointConfig {
        EndpointConfig {
            id: self.id,
            name: self.name,
            base_url: self.base_url,
            selected_model: (!self.model.trim().is_empty()).then_some(self.model),
            credential_ref: self.credential_ref,
            overrides: self.extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use sujiu_core::{ContextKind, Persona, PromptProfile};

    use super::*;
    use crate::storage::MemoryStorage;

    fn sample_library() -> Library {
        Library {
            conversations: vec![Conversation::new("conversation-1")],
            characters: vec![Character::new("character-lin", "Lin")],
            personas: vec![Persona::new("persona-me", "Me")],
            world_books: vec![WorldBook::new("lore", "Lore")],
            prompt_profiles: vec![PromptProfile::new("profile-1", "Default")],
            global_worldbook_ids: vec!["lore".into()],
        }
    }

    fn source(id: &str) -> ContextSource {
        ContextSource {
            id: id.to_string(),
            kind: ContextKind::Other,
            name: id.to_string(),
            description: String::new(),
            scope: Default::default(),
            mutable: false,
            record_count: 0,
            metadata: Default::default(),
        }
    }

    fn record(uri: &str, source_id: &str, kind: ContextKind) -> ContextRecord {
        ContextRecord {
            uri: uri.to_string(),
            source_id: source_id.to_string(),
            kind,
            title: String::new(),
            content: "content".to_string(),
            keywords: Vec::new(),
            tags: Vec::new(),
            priority: 0,
            timestamp_ms: None,
            scope: Default::default(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn every_entity_is_stored_under_its_own_kind_and_a_conversation_owns_a_directory() {
        let storage = MemoryStorage::new();
        save(&storage, &sample_library(), &LibraryIndex::default()).unwrap();

        assert!(storage.load(LIBRARY_FILE).is_some());
        assert!(storage
            .load("conversations/conversation-1/conversation.json")
            .is_some());
        assert!(storage.load("characters/character-lin.json").is_some());
        assert!(storage.load("personas/persona-me.json").is_some());
        assert!(storage.load("worldbooks/lore.json").is_some());
        assert!(storage.load("prompt_profiles/profile-1.json").is_some());
    }

    #[test]
    fn a_stored_library_comes_back_as_the_same_domain_entities() {
        let storage = MemoryStorage::new();
        let library = sample_library();
        save(&storage, &library, &LibraryIndex::default()).unwrap();

        let loaded = load(&storage).expect("a readable store");
        assert_eq!(loaded.library, library);
    }

    /// The projection must not become the place a world book is kept. If it were
    /// written as well, deleting the world-book document would leave its lore
    /// searchable and the split would be cosmetic.
    #[test]
    fn a_projected_source_is_not_written_as_if_it_were_stored_data() {
        let storage = MemoryStorage::new();
        let index = LibraryIndex {
            sources: vec![source("worldbook:lore"), source("story")],
            records: vec![
                record(
                    "sujiu://context/worldbook:lore/one",
                    "worldbook:lore",
                    ContextKind::WorldLore,
                ),
                record(
                    "sujiu://context/story/one",
                    "story",
                    ContextKind::StoryEvent,
                ),
            ],
            ..LibraryIndex::default()
        };

        save(&storage, &sample_library(), &index).unwrap();
        let loaded = load(&storage).expect("a readable store");

        assert_eq!(loaded.index.sources.len(), 1);
        assert_eq!(loaded.index.sources[0].id, "story");
        assert_eq!(loaded.index.records.len(), 1);
        assert_eq!(loaded.index.records[0].source_id, "story");
    }

    #[test]
    fn a_version_three_document_becomes_a_conversation_and_keeps_its_tool_calls() {
        let document = serde_json::json!({
            "version": 3,
            "characters": [{
                "schemaVersion": 1,
                "id": "character-lin",
                "name": "Lin",
                "world_book": {
                    "name": "Lore",
                    "entries": [{"id": "tower", "content": "north", "keys": ["tower"]}]
                }
            }],
            "sessions": [{
                "id": "session-1",
                "characterId": "character-lin",
                "transcript": {
                    "turns": [{
                        "id": "turn-1",
                        "user": "hello",
                        "steps": [{
                            "tool_calls": [{
                                "id": "call-1",
                                "name": "search_context",
                                "title": "Search",
                                "arguments": {},
                                "result": {
                                    "state": "completed",
                                    "content": [{"type": "text", "text": "found"}]
                                }
                            }]
                        }]
                    }]
                }
            }],
            "records": [{
                "uri": "sujiu://context/story/one",
                "source_id": "story",
                "kind": "story_event",
                "content": "an event",
                "scope": {"session_id": "session-1"}
            }]
        })
        .to_string();

        let migrated = migrate_legacy_document(&document).expect("a migration");

        let conversation = &migrated.library.conversations[0];
        assert_eq!(
            conversation.id, "session-1",
            "the id a platform already stored survives the move"
        );
        assert_eq!(conversation.participants.len(), 1);
        assert_eq!(conversation.participants[0].character_id, "character-lin");
        assert_eq!(conversation.transcript.turns.len(), 1);
        assert_eq!(
            conversation.transcript.turns[0].steps[0]
                .tool_calls
                .first()
                .map(|call| call.name.as_str()),
            Some("search_context"),
            "a real transcript is not rebuilt from text"
        );

        // The card keeps its lore, as an independent book the card refers to.
        assert_eq!(migrated.library.world_books.len(), 1);
        let book = &migrated.library.world_books[0];
        assert_eq!(book.id, "character-lin-world-book");
        assert_eq!(book.entries[0].content, "north");
        assert_eq!(
            migrated.library.characters[0].worldbook_ids,
            vec![book.id.clone()],
            "the card's defaults have to name the book it used to carry"
        );

        // A record written against a session still names its conversation.
        assert_eq!(
            migrated.index.records[0].scope.conversation_id.as_deref(),
            Some("session-1")
        );
    }

    #[test]
    fn a_version_two_document_keeps_its_endpoint_under_the_old_name() {
        let document = serde_json::json!({
            "version": 2,
            "characters": [],
            "sessions": [],
            "providerConfig": {
                "id": "endpoint",
                "baseUrl": "https://example.test/v1",
                "model": "m1"
            }
        })
        .to_string();

        let migrated = migrate_legacy_document(&document).expect("a migration");
        assert_eq!(
            migrated.index.endpoint.expect("the endpoint").base_url,
            "https://example.test/v1"
        );
    }

    #[test]
    fn a_version_one_document_keeps_its_messages_as_turns() {
        let document = serde_json::json!({
            "characters": [{"id": "character-lin", "name": "Lin"}],
            "sessions": [{
                "id": "session-1",
                "characterId": "character-lin",
                "messages": [
                    {"id": "m1", "role": "user", "content": "hello"},
                    {"id": "m2", "role": "assistant", "content": "hi"}
                ]
            }],
            "providerConfig": {
                "id": "endpoint",
                "baseUrl": "https://example.test/v1",
                "model": "m1"
            }
        })
        .to_string();

        let migrated = migrate_legacy_document(&document).expect("a migration");
        let conversation = &migrated.library.conversations[0];

        assert_eq!(conversation.transcript.turns.len(), 1);
        assert_eq!(conversation.transcript.turns[0].user, "hello");
        assert_eq!(
            conversation.transcript.turns[0].steps[0].text.as_deref(),
            Some("hi")
        );
        assert_eq!(
            migrated.index.endpoint.expect("the endpoint").base_url,
            "https://example.test/v1"
        );
    }

    #[test]
    fn a_document_that_is_not_json_is_not_a_migration() {
        assert!(migrate_legacy_document("not a document").is_none());
    }

    /// A document from a newer build is not something this one may rewrite.
    #[test]
    fn a_document_from_a_newer_version_is_left_alone() {
        let document = serde_json::json!({ "version": 99, "sessions": [] }).to_string();
        assert!(migrate_legacy_document(&document).is_none());
    }
}
