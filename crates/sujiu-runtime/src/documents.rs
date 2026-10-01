//! How domain entities are written to a platform's storage.
//!
//! The layout is by domain, and a conversation owns its own directory — inside
//! the generation that is currently live:
//!
//! ```text
//! sujiu-library.json                              manifest, names one generation
//! generations/gen-00000007/conversations/<conversation-id>/conversation.json
//! generations/gen-00000007/characters/<character-id>.json
//! generations/gen-00000007/personas/<persona-id>.json
//! generations/gen-00000007/worldbooks/<world-book-id>.json
//! generations/gen-00000007/prompt_profiles/<profile-id>.json
//! ```
//!
//! The direction matters. It used to be `characters/<id>/chats/...`, which made
//! a conversation a child of one card. Characters, personas, world books and
//! prompt profiles are independent resources that a conversation *binds* by id,
//! so nothing is stored under a character's directory, and a conversation is the
//! unit a transcript is filed under.
//!
//! A generation is what makes the switch atomic. The manifest is the only
//! document a load starts from, so it is written last and it names one
//! generation: a save that fails half way leaves the previous generation
//! complete and still named, and the next save simply writes over the
//! half-written one. Writing the entity documents in place would instead leave
//! the old manifest pointing at a mixture of new and old documents, which is a
//! store that parses and is wrong.
//!
//! Two things live outside the five domain collections and are said so here:
//! the endpoint configuration and the context store. Neither is a domain
//! entity, and the context store is a projection (see `sujiu_ai::project_library`),
//! so persisting it must never be what keeps a character or a world book alive.
//! Projected sources are therefore filtered back out on the way out and rebuilt
//! from the domain documents on the way in.
//!
//! The `<id>` in each path above is a **logical** id, and it is never spliced in
//! as written. Every one of them goes through [`path_segment`] on the way in and
//! on the way out, because an id can arrive from an imported card, a world book
//! or a migrated document, and a value that chose its own directory could
//! otherwise leave the generation it belongs to. The manifest keeps the logical
//! id, so nothing above this layer has to know the mapping exists.

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
    /// Which generation the entity documents live in.
    ///
    /// `0` means the documents sit at the top level, which is how the first
    /// library build wrote them. It is still readable, because a store someone
    /// already has is not a store this build may refuse.
    #[serde(default)]
    pub generation: u64,
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
pub const LIBRARY_VERSION: u32 = 2;

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

/// The directory every generation is written under.
pub const GENERATION_PREFIX: &str = "generations";

/// One generation's directory, as a storage key.
///
/// Zero-padded so a directory listing sorts in write order, which matters to
/// whoever is reading this store with `ls` at three in the morning.
pub fn generation_key(generation: u64) -> String {
    format!("{GENERATION_PREFIX}/gen-{generation:08}")
}

/// The longest a mapped segment may be before a hash name is used instead.
///
/// Well under the 255-byte limit every mainstream filesystem puts on one path
/// component, and comfortably above any id Sujiu mints itself. It exists because
/// a percent-escaped segment is up to three times the id it came from, so an id
/// long enough to be legal as a name can still be too long once mapped.
const MAX_SEGMENT_BYTES: usize = 120;

/// The name an id with no content is stored under.
///
/// The marker is `~` because no mapped segment can ever contain one: `~` is
/// escaped to `%7E`, and nothing in the safe set produces a bare `~`. So this
/// cannot be the image of any other id, which is what keeps the mapping
/// collision-free.
const EMPTY_SEGMENT: &str = "~empty";

/// The prefix a mapped segment that had to be shortened carries.
const HASHED_SEGMENT_PREFIX: &str = "~hash-";

/// Map a domain id onto the one path segment that stands for it on disk.
///
/// An id is a **logical** id. It is whatever a card, a world book, a migrated
/// document or a platform named the entity, which means it can contain `/`, a
/// backslash, `..`, or nothing at all. Splicing such a value straight into a key
/// would let it decide the directory a document lands in — escaping the
/// generation it belongs to, or overwriting a sibling, or the manifest itself.
/// So every id goes through here, on the way in and on the way out, and the
/// manifest stores the logical id unchanged.
///
/// The rule, in one sentence: letters, digits, `.`, `_` and `-` survive; every
/// other byte becomes `%` plus two uppercase hex digits; `.` and `..` are
/// escaped whole; an empty id and an over-long id get a marked name of their own.
///
/// Three properties are load-bearing, and the tests hold each of them:
///
/// - **Round trip.** Both sides call this one function, so `load` looks in
///   exactly the place `save` wrote, without having to decode anything.
/// - **No collisions.** The output alphabet is the safe set plus well-formed
///   `%XX` escapes, and `%` is itself escaped. A safe id can only be produced by
///   itself, and two different unsafe ids cannot collapse onto one name. The
///   marked names use `~`, which the output alphabet cannot contain.
/// - **Stability for ordinary ids.** `session-1` and `character-lin` map to
///   themselves. A store written by an earlier build keeps resolving, so this
///   rule needs no migration of its own.
///
/// A hash name is a fallback, not a security boundary: the id it stands for is in
/// the manifest, and the 64-bit FNV-1a below is a stable name, chosen because it
/// is fixed by this function and cannot drift between runs the way a
/// deliberately-unseeded hasher can.
pub fn path_segment(id: &str) -> String {
    if id.is_empty() {
        return EMPTY_SEGMENT.to_owned();
    }

    let escaped = escape(id);
    if escaped == "." || escaped == ".." {
        return escape_dots(id);
    }
    if escaped.len() > MAX_SEGMENT_BYTES {
        return format!("{HASHED_SEGMENT_PREFIX}{:016x}", fnv1a64(id.as_bytes()));
    }
    escaped
}

fn escape(id: &str) -> String {
    let mut escaped = String::with_capacity(id.len());
    for byte in id.as_bytes() {
        if is_safe(*byte) {
            escaped.push(*byte as char);
        } else {
            escaped.push('%');
            escaped.push_str(&format!("{byte:02X}"));
        }
    }
    escaped
}

/// The same mapping with `.` escaped too, so a segment is never `.` or `..`.
fn escape_dots(id: &str) -> String {
    let mut escaped = String::with_capacity(id.len());
    for byte in id.as_bytes() {
        match byte {
            b'%' => escaped.push_str("%25"),
            b'.' => escaped.push_str("%2E"),
            byte if is_safe(*byte) => escaped.push(*byte as char),
            byte => {
                escaped.push('%');
                escaped.push_str(&format!("{byte:02X}"));
            }
        }
    }
    escaped
}

fn is_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

/// FNV-1a, 64-bit. Not a security hash: it names a segment deterministically.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// An entity document's key inside a generation.
///
/// Generation `0` is the top-level layout the first library build wrote, and it
/// is read from where it was written rather than migrated on sight.
fn entity_key(generation: u64, key: &str) -> String {
    if generation == 0 {
        key.to_owned()
    } else {
        format!("{}/{key}", generation_key(generation))
    }
}

/// Every key that names a domain document goes through `path_segment`.
///
/// None of these take a raw id, and that is the point: a path built from a value
/// the domain did not mint is a path the domain does not get to choose.
pub fn conversation_key(id: &str) -> String {
    format!("conversations/{}/conversation.json", path_segment(id))
}

pub fn character_key(id: &str) -> String {
    format!("characters/{}.json", path_segment(id))
}

pub fn persona_key(id: &str) -> String {
    format!("personas/{}.json", path_segment(id))
}

pub fn world_book_key(id: &str) -> String {
    format!("worldbooks/{}.json", path_segment(id))
}

pub fn prompt_profile_key(id: &str) -> String {
    format!("prompt_profiles/{}.json", path_segment(id))
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
        library.conversations.push(read_entity(
            storage,
            &entity_key(index.generation, &conversation_key(id)),
        )?);
    }
    for id in &index.character_ids {
        library.characters.push(read_entity(
            storage,
            &entity_key(index.generation, &character_key(id)),
        )?);
    }
    for id in &index.persona_ids {
        library.personas.push(read_entity(
            storage,
            &entity_key(index.generation, &persona_key(id)),
        )?);
    }
    for id in &index.world_book_ids {
        library.world_books.push(read_entity(
            storage,
            &entity_key(index.generation, &world_book_key(id)),
        )?);
    }
    for id in &index.prompt_profile_ids {
        library.prompt_profiles.push(read_entity(
            storage,
            &entity_key(index.generation, &prompt_profile_key(id)),
        )?);
    }

    Some(LoadedLibrary { library, index })
}

fn read_entity<T: serde::de::DeserializeOwned>(storage: &dyn AppStorage, key: &str) -> Option<T> {
    let document = storage.load(key)?;
    serde_json::from_str(&document).ok()
}

/// Write every entity into a new generation, then the manifest that names it.
///
/// The order is the whole point. Every entity document is written into a
/// generation the manifest does not mention yet, and the manifest is written
/// last, so the store is only ever switched by one atomic document write. A
/// failure before that point leaves the previous generation complete and still
/// named: the next load reads exactly what it read before, rather than a mixture
/// of new and old documents. A failure is returned, because a save that cannot
/// finish must not be followed by a manifest claiming that it did.
///
/// The generation that was live is retired only after the new manifest is safely
/// in place, and a failure to retire it is ignored: an extra directory costs
/// disk, while a retired generation the manifest still names costs data.
pub fn save(
    storage: &dyn AppStorage,
    library: &Library,
    index: &LibraryIndex,
) -> std::io::Result<u64> {
    let previous = read_generation(storage);

    let mut index = index.clone();
    index.version = LIBRARY_VERSION;
    // The next generation is one past the live one. A half-written generation
    // from an interrupted save is written over rather than skipped, so an
    // interrupted save does not leak a directory per attempt.
    index.generation = previous + 1;
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

    let generation = index.generation;

    for item in &library.conversations {
        write_entity(
            storage,
            &entity_key(generation, &conversation_key(&item.id)),
            item,
        )?;
    }
    for item in &library.characters {
        write_entity(
            storage,
            &entity_key(generation, &character_key(&item.id)),
            item,
        )?;
    }
    for item in &library.personas {
        write_entity(
            storage,
            &entity_key(generation, &persona_key(&item.id)),
            item,
        )?;
    }
    for item in &library.world_books {
        write_entity(
            storage,
            &entity_key(generation, &world_book_key(&item.id)),
            item,
        )?;
    }
    for item in &library.prompt_profiles {
        write_entity(
            storage,
            &entity_key(generation, &prompt_profile_key(&item.id)),
            item,
        )?;
    }

    let document = serde_json::to_string(&index).map_err(invalid_data)?;
    storage.save(LIBRARY_FILE, &document)?;

    if previous > 0 {
        let _ = storage.remove(&generation_key(previous));
    }

    Ok(generation)
}

/// The generation the manifest currently names, or zero when there is none.
///
/// A manifest that cannot be parsed counts as no generation: the next save then
/// starts at one and writes a complete store beside whatever is there, and the
/// unreadable manifest keeps the store protected until the user resolves it.
fn read_generation(storage: &dyn AppStorage) -> u64 {
    storage
        .load(LIBRARY_FILE)
        .and_then(|manifest| serde_json::from_str::<LibraryIndex>(&manifest).ok())
        .map(|index| index.generation)
        .unwrap_or(0)
}

fn write_entity<T: Serialize>(
    storage: &dyn AppStorage,
    key: &str,
    entity: &T,
) -> std::io::Result<()> {
    let document = serde_json::to_string(entity).map_err(invalid_data)?;
    storage.save(key, &document)
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

    /// The generation the manifest currently names, so a test reads the layout
    /// instead of assuming a number. Hard-coding `gen-00000001` would make the
    /// test pass only while nothing else in the runtime had ever saved.
    fn live_generation(storage: &MemoryStorage) -> String {
        let document = storage.load(LIBRARY_FILE).expect("a manifest");
        let index: LibraryIndex = serde_json::from_str(&document).expect("a readable manifest");
        generation_key(index.generation)
    }

    /// Storage that keeps every document in a map and fails on demand.
    ///
    /// The failure is what this test is about, so it is injected rather than
    /// produced by filling a disk, which is neither portable nor deterministic.
    struct FailingStorage {
        inner: MemoryStorage,
        /// 1-based ordinal of the next `save` that fails; `0` never fails.
        fail_on: std::sync::atomic::AtomicUsize,
        saves: std::sync::atomic::AtomicUsize,
    }

    impl FailingStorage {
        fn new() -> Self {
            Self {
                inner: MemoryStorage::new(),
                fail_on: std::sync::atomic::AtomicUsize::new(0),
                saves: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        /// Fail the `nth` save from now, counting the ones in between.
        fn arm(&self, nth: usize) {
            self.fail_on.store(nth, std::sync::atomic::Ordering::SeqCst);
            self.saves.store(0, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl AppStorage for FailingStorage {
        fn load(&self, key: &str) -> Option<String> {
            self.inner.load(key)
        }

        fn save(&self, key: &str, contents: &str) -> std::io::Result<()> {
            let ordinal = self.saves.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if self.fail_on.load(std::sync::atomic::Ordering::SeqCst) == ordinal {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("injected failure writing {key}"),
                ));
            }
            self.inner.save(key, contents)
        }

        fn remove(&self, key: &str) -> std::io::Result<()> {
            self.inner.remove(key)
        }

        fn location(&self) -> Option<String> {
            Some("in-memory".to_string())
        }
    }

    #[test]
    fn every_entity_is_stored_under_its_own_kind_and_a_conversation_owns_a_directory() {
        let storage = MemoryStorage::new();
        save(&storage, &sample_library(), &LibraryIndex::default()).unwrap();
        let generation = live_generation(&storage);

        assert!(storage.load(LIBRARY_FILE).is_some());
        assert!(storage
            .load(&format!(
                "{generation}/conversations/conversation-1/conversation.json"
            ))
            .is_some());
        assert!(storage
            .load(&format!("{generation}/characters/character-lin.json"))
            .is_some());
        assert!(storage
            .load(&format!("{generation}/personas/persona-me.json"))
            .is_some());
        assert!(storage
            .load(&format!("{generation}/worldbooks/lore.json"))
            .is_some());
        assert!(storage
            .load(&format!("{generation}/prompt_profiles/profile-1.json"))
            .is_some());
    }

    /// A save that fails half way must leave the *previous* complete version
    /// readable, not a mixture of two. Overwriting entity files in place and
    /// only then switching the manifest cannot promise that: the manifest would
    /// name documents that are half of one save and half of another.
    ///
    /// The test fails the save at every step in turn — including the manifest
    /// write itself — and requires the same thing each time: the library that
    /// comes back is the one that was complete before.
    #[test]
    fn a_save_that_fails_at_any_step_leaves_the_previous_library_readable() {
        let first = sample_library();
        // Enough saves that the failure ordinal covers the manifest write, not
        // just the entity writes: one generation writes 5 entities + manifest.
        let steps_per_save = first.conversations.len()
            + first.characters.len()
            + first.personas.len()
            + first.world_books.len()
            + first.prompt_profiles.len()
            + 1;

        for fail_on in 1..=steps_per_save {
            let storage = FailingStorage::new();
            save(&storage, &first, &LibraryIndex::default())
                .expect("the first save is the whole library");
            storage.arm(fail_on);

            // A second conversation, so the failing save has something to lose.
            let mut second = first.clone();
            second
                .conversations
                .push(Conversation::new("conversation-2"));

            let error = save(&storage, &second, &LibraryIndex::default())
                .expect_err("this save is supposed to fail");
            assert!(
                error.to_string().contains("injected failure"),
                "the failure must reach the caller, not be swallowed: {error}"
            );

            let loaded = load(&storage).expect("the previous library is still readable");
            assert_eq!(
                loaded.library.conversations.len(),
                1,
                "a failed save (step {fail_on}) must not leave a half-written library behind"
            );
            assert_eq!(loaded.library.conversations[0].id, "conversation-1");
        }
    }

    /// A second save moves to a new generation directory and lets go of the old
    /// one only once the new manifest names the new generation.
    #[test]
    fn each_save_moves_the_library_to_a_new_generation() {
        let storage = MemoryStorage::new();
        assert_eq!(
            save(&storage, &sample_library(), &LibraryIndex::default()).unwrap(),
            1
        );

        let mut grown = sample_library();
        grown
            .conversations
            .push(Conversation::new("conversation-2"));
        assert_eq!(save(&storage, &grown, &LibraryIndex::default()).unwrap(), 2);

        assert!(storage
            .load("conversations/conversation-2/conversation.json")
            .is_none());
        assert!(storage
            .load("generations/gen-00000002/conversations/conversation-2/conversation.json")
            .is_some());
        // The previous generation is only dropped once the new manifest is live.
        assert!(storage.load("generations/gen-00000001").is_none());
        assert_eq!(
            load(&storage)
                .expect("the live generation")
                .library
                .conversations
                .len(),
            2
        );
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

/// An id is a logical id, and only a mapped one may become a path segment.
#[cfg(test)]
mod path_tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::storage::MemoryStorage;

    /// The property every one of these tests leans on: no mapping of these ids
    /// may contain a separator, name a parent directory, or be empty.
    fn assert_inert(segment: &str) {
        assert!(!segment.is_empty(), "an empty segment names the directory");
        assert!(!segment.contains('/'), "`/` in {segment:?}");
        assert!(!segment.contains('\\'), "a backslash in {segment:?}");
        assert!(!segment.contains('\0'), "a NUL in {segment:?}");
        assert_ne!(segment, ".", "`.` is the directory itself");
        assert_ne!(segment, "..", "`..` is the parent directory");
    }

    #[test]
    fn an_ordinary_id_is_its_own_segment() {
        for id in [
            "session-1",
            "character-lin",
            "world-book-coast",
            "profile_roleplay",
            "v2.1",
            "A1",
        ] {
            assert_eq!(path_segment(id), id, "{id} should be left alone");
        }
    }

    #[test]
    fn an_id_that_asks_to_climb_out_is_escaped_instead() {
        // A segment that *is* a parent directory has to be escaped whole.
        assert_eq!(path_segment(".."), "%2E%2E");
        assert_eq!(path_segment("."), "%2E");
        // A `..` in the middle of a name is only a substring: it reaches no
        // directory, because the separator that would make it one is escaped.
        assert_eq!(
            path_segment("../../sujiu-library.json"),
            "..%2F..%2Fsujiu-library.json"
        );
        assert_eq!(path_segment("a/../../b"), "a%2F..%2F..%2Fb");
        for segment in [
            path_segment(".."),
            path_segment("."),
            path_segment("../../sujiu-library.json"),
            path_segment("a/../../b"),
        ] {
            assert_inert(&segment);
        }
    }

    #[test]
    fn an_id_with_a_separator_in_it_cannot_introduce_a_directory() {
        for id in [
            "a/b",
            "a\\b",
            "a/b/c",
            "conversations/x/../../y",
            "\\",
            "//",
        ] {
            assert_inert(&path_segment(id));
        }
        assert_eq!(path_segment("a/b"), "a%2Fb");
        assert_eq!(path_segment("a\\b"), "a%5Cb");
    }

    #[test]
    fn an_empty_id_gets_a_name_of_its_own() {
        assert_eq!(path_segment(""), EMPTY_SEGMENT);
        assert_inert(&path_segment(""));
    }

    #[test]
    fn an_over_long_id_is_named_by_its_hash() {
        let long = "l".repeat(400);
        let segment = path_segment(&long);
        assert_inert(&segment);
        assert!(segment.starts_with(HASHED_SEGMENT_PREFIX));
        assert!(segment.len() <= MAX_SEGMENT_BYTES);
        // The same id always gets the same name, or a save would orphan its own
        // document.
        assert_eq!(segment, path_segment(&long));
        // And two different long ids do not get the same one.
        let other = "l".repeat(399) + "m";
        assert_ne!(segment, path_segment(&other));
    }

    #[test]
    fn the_boundary_between_a_plain_name_and_a_hash_is_where_it_says() {
        // Exactly at the cap the id is still used, one byte over it is hashed.
        let at_cap = "l".repeat(MAX_SEGMENT_BYTES);
        assert_eq!(path_segment(&at_cap), at_cap);
        let over_cap = "l".repeat(MAX_SEGMENT_BYTES + 1);
        assert!(path_segment(&over_cap).starts_with(HASHED_SEGMENT_PREFIX));
    }

    /// The whole reason for the marked names: two different ids may never land
    /// on one file, or a save would silently drop one of them.
    #[test]
    fn two_different_ids_never_land_on_one_document() {
        // The two long ids differ in one byte, and are borrowed so the array can
        // hold them next to the literals.
        let long_a = "l".repeat(400);
        let long_b = format!("{}m", "l".repeat(399));
        let ids = [
            "",
            ".",
            "..",
            "a",
            "a.json",
            "a%2Ejson",
            "a.b",
            "a/b",
            "a%2Fb",
            "a\\b",
            "~empty",
            "~hash-0000000000000000",
            "sujiu-library.json",
            "generations",
            "generations/gen-00000001",
            long_a.as_str(),
            long_b.as_str(),
        ];
        let segments: BTreeSet<String> = ids.iter().copied().map(path_segment).collect();
        assert_eq!(segments.len(), ids.len(), "these ids collide: {segments:?}");
    }

    /// A marked name is unreachable from any id, which is what lets the collision
    /// test above pass without a special case per token.
    #[test]
    fn a_marked_name_cannot_come_from_an_id() {
        // `~` is escaped, so no id produces a bare one and therefore nothing
        // produces either marked name.
        assert_eq!(path_segment("~"), "%7E");
        assert_eq!(path_segment("~empty"), "%7Eempty");
        assert_eq!(
            path_segment("~hash-0000000000000000"),
            "%7Ehash-0000000000000000"
        );
        assert!(path_segment("").contains('~'));
        assert!(path_segment(&"l".repeat(MAX_SEGMENT_BYTES + 1)).contains('~'));
    }

    /// The rule has to hold for every kind of document, not just characters.
    #[test]
    fn every_document_key_goes_through_the_same_mapping() {
        let nasty = "../../escape";
        let keys = [
            conversation_key(nasty),
            character_key(nasty),
            persona_key(nasty),
            world_book_key(nasty),
            prompt_profile_key(nasty),
        ];
        for key in keys {
            // What matters is not that the text is gone but that no component of
            // the key is a relative directory.
            for component in key.split('/') {
                assert_ne!(component, "..", "{key:?} still climbs");
                assert_ne!(component, ".", "{key:?} is a relative path");
            }
            assert_eq!(key.matches("escape").count(), 1, "{key:?}");
        }
        assert_eq!(
            conversation_key("session-1"),
            "conversations/session-1/conversation.json"
        );
        assert_eq!(
            character_key("character-lin"),
            "characters/character-lin.json"
        );
    }

    /// An id is mapped on the way in and on the way out by the same call, so a
    /// document written under a hostile id is found again by the id the manifest
    /// kept.
    #[test]
    fn a_hostile_id_survives_a_save_and_a_load() {
        let storage = MemoryStorage::new();
        let id = "../../../escaped/./..";
        let library = Library {
            conversations: vec![Conversation::new(id)],
            characters: vec![Character::new(id, "Lin")],
            ..Library::default()
        };
        let index = LibraryIndex::default();
        save(&storage, &library, &index).expect("a save");

        let document = storage.load(LIBRARY_FILE).expect("a manifest");
        let index: LibraryIndex = serde_json::from_str(&document).expect("a readable manifest");
        // The manifest keeps the logical id, not the path spelling.
        assert_eq!(index.conversation_ids, vec![id.to_owned()]);
        let inside = format!("{}/", generation_key(index.generation));
        for key in [character_key(id), conversation_key(id)] {
            assert!(storage.load(&format!("{inside}{key}")).is_some(), "{key}");
        }

        let loaded = load(&storage).expect("a readable library");
        assert_eq!(loaded.library.conversations[0].id, id);
        assert_eq!(loaded.library.characters[0].id, id);
    }

    /// An id is escaped, not truncated: the two must not become the same file.
    #[test]
    fn escaping_keeps_two_near_identical_ids_apart() {
        assert_ne!(path_segment("a b"), path_segment("a%20b"));
        assert_ne!(path_segment("a%2Fb"), path_segment("a/b"));
    }
}
