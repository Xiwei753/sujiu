use serde::{Deserialize, Serialize};

use crate::{
    Character, Conversation, Participant, Persona, PromptContext, PromptProfile, Transcript,
    WorldBook, WorldBookBudget,
};

/// Everything the app stores, as domain entities.
///
/// This is the source of truth. `ContextSource`/`ContextRecord` are a *projection*
/// of it for search and tool reads — convenient, but not where a character, a
/// persona or a world book lives. Keeping the two apart is what lets a world
/// book outlive the card that first carried it.
///
/// The collections are vectors rather than maps because they are serialized
/// directly, and because the order a user arranged their library in is itself
/// worth keeping. Lookups by id are linear, which is the right cost at this
/// size; if that stops being true the map belongs in the in-memory form, not in
/// the stored one.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    #[serde(default)]
    pub conversations: Vec<Conversation>,
    #[serde(default)]
    pub characters: Vec<Character>,
    #[serde(default)]
    pub personas: Vec<Persona>,
    #[serde(default)]
    pub world_books: Vec<WorldBook>,
    #[serde(default)]
    pub prompt_profiles: Vec<PromptProfile>,
    /// World books that apply everywhere, regardless of who is talking.
    #[serde(default)]
    pub global_worldbook_ids: Vec<String>,
}

impl Library {
    pub fn conversation(&self, id: &str) -> Option<&Conversation> {
        self.conversations.iter().find(|item| item.id == id)
    }

    pub fn conversation_mut(&mut self, id: &str) -> Option<&mut Conversation> {
        self.conversations.iter_mut().find(|item| item.id == id)
    }

    pub fn character(&self, id: &str) -> Option<&Character> {
        self.characters.iter().find(|item| item.id == id)
    }

    pub fn character_mut(&mut self, id: &str) -> Option<&mut Character> {
        self.characters.iter_mut().find(|item| item.id == id)
    }

    pub fn persona(&self, id: &str) -> Option<&Persona> {
        self.personas.iter().find(|item| item.id == id)
    }

    pub fn persona_mut(&mut self, id: &str) -> Option<&mut Persona> {
        self.personas.iter_mut().find(|item| item.id == id)
    }

    pub fn world_book(&self, id: &str) -> Option<&WorldBook> {
        self.world_books.iter().find(|item| item.id == id)
    }

    pub fn world_book_mut(&mut self, id: &str) -> Option<&mut WorldBook> {
        self.world_books.iter_mut().find(|item| item.id == id)
    }

    pub fn prompt_profile(&self, id: &str) -> Option<&PromptProfile> {
        self.prompt_profiles.iter().find(|item| item.id == id)
    }

    pub fn prompt_profile_mut(&mut self, id: &str) -> Option<&mut PromptProfile> {
        self.prompt_profiles.iter_mut().find(|item| item.id == id)
    }

    /// Insert or replace by id, keeping the old position if it existed.
    ///
    /// Reports whether the entity was new, which is what a caller writing a
    /// per-entity document needs to know.
    fn upsert_by_id<T, F>(collection: &mut Vec<T>, entity: T, id_of: F) -> bool
    where
        F: Fn(&T) -> &str,
    {
        let id = id_of(&entity).to_string();
        match collection.iter_mut().find(|item| id_of(item) == id) {
            Some(slot) => {
                *slot = entity;
                false
            }
            None => {
                collection.push(entity);
                true
            }
        }
    }

    pub fn upsert_character(&mut self, character: Character) -> bool {
        Self::upsert_by_id(&mut self.characters, character, |item| item.id.as_str())
    }

    pub fn upsert_persona(&mut self, persona: Persona) -> bool {
        Self::upsert_by_id(&mut self.personas, persona, |item| item.id.as_str())
    }

    pub fn upsert_world_book(&mut self, book: WorldBook) -> bool {
        Self::upsert_by_id(&mut self.world_books, book, |item| item.id.as_str())
    }

    pub fn upsert_prompt_profile(&mut self, profile: PromptProfile) -> bool {
        Self::upsert_by_id(&mut self.prompt_profiles, profile, |item| item.id.as_str())
    }

    /// A conversation id that cannot collide with an existing one.
    pub fn next_conversation_id(&self) -> String {
        next_id(&self.conversations, "conversation", |item| item.id.as_str())
    }

    /// A character id that cannot collide with an existing one.
    pub fn next_character_id(&self) -> String {
        next_id(&self.characters, "character", |item| item.id.as_str())
    }

    /// A persona id that cannot collide with an existing one.
    pub fn next_persona_id(&self) -> String {
        next_id(&self.personas, "persona", |item| item.id.as_str())
    }

    /// A world book id that cannot collide with an existing one.
    pub fn next_world_book_id(&self) -> String {
        next_id(&self.world_books, "worldbook", |item| item.id.as_str())
    }

    /// A prompt profile id that cannot collide with an existing one.
    pub fn next_prompt_profile_id(&self) -> String {
        next_id(&self.prompt_profiles, "promptprofile", |item| {
            item.id.as_str()
        })
    }

    /// Remove a character, and every reference that would outlive it.
    ///
    /// Deleting a resource has to clean up the bindings that named it, or the
    /// library keeps dangling ids that quietly resolve to nothing at prompt
    /// time. A participant whose card is gone is dropped rather than kept as an
    /// empty slot: the user can re-add the card, and a nameless participant
    /// would speak as one in the meantime.
    pub fn remove_character(&mut self, id: &str) -> bool {
        let before = self.characters.len();
        self.characters.retain(|item| item.id != id);
        if self.characters.len() == before {
            return false;
        }
        for conversation in &mut self.conversations {
            conversation
                .participants
                .retain(|participant| participant.character_id != id);
        }
        true
    }

    /// Remove a persona, clearing it from every conversation that used it.
    pub fn remove_persona(&mut self, id: &str) -> bool {
        let before = self.personas.len();
        self.personas.retain(|item| item.id != id);
        if self.personas.len() == before {
            return false;
        }
        for conversation in &mut self.conversations {
            if conversation.persona_id.as_deref() == Some(id) {
                conversation.persona_id = None;
            }
        }
        true
    }

    /// Remove a world book, clearing every binding that named it.
    ///
    /// World books are shared, so this reaches further than the conversations:
    /// a character's defaults, the persona's defaults and the global list all
    /// name books too, and a book that survives only as a dangling id there is
    /// a reference the user cannot see or fix.
    pub fn remove_world_book(&mut self, id: &str) -> bool {
        let before = self.world_books.len();
        self.world_books.retain(|item| item.id != id);
        if self.world_books.len() == before {
            return false;
        }
        for conversation in &mut self.conversations {
            conversation.worldbook_ids.retain(|item| item != id);
        }
        for character in &mut self.characters {
            character.worldbook_ids.retain(|item| item != id);
        }
        for persona in &mut self.personas {
            persona.worldbook_ids.retain(|item| item != id);
        }
        self.global_worldbook_ids.retain(|item| item != id);
        true
    }

    /// Remove a prompt profile, clearing it from every conversation that used it.
    pub fn remove_prompt_profile(&mut self, id: &str) -> bool {
        let before = self.prompt_profiles.len();
        self.prompt_profiles.retain(|item| item.id != id);
        if self.prompt_profiles.len() == before {
            return false;
        }
        for conversation in &mut self.conversations {
            if conversation.prompt_profile_id.as_deref() == Some(id) {
                conversation.prompt_profile_id = None;
            }
        }
        true
    }

    /// The characters taking part in a conversation, in participant order.
    ///
    /// A participant naming a card that is not installed is skipped rather than
    /// turned into an empty card: a missing card is a real state the user can
    /// fix, and inventing a nameless character in the prompt is not.
    pub fn participants_of(&self, conversation: &Conversation) -> Vec<(Participant, &Character)> {
        conversation
            .participants
            .iter()
            .filter_map(|participant| {
                self.character(&participant.character_id)
                    .map(|character| (participant.clone(), character))
            })
            .collect()
    }

    /// The world books in force for a conversation.
    ///
    /// Explicit bindings first, then the participants' and the persona's
    /// defaults, then the global ones. Order is stable across turns on purpose:
    /// this list feeds a prompt block a provider is meant to cache, and a
    /// reordering that changed nothing would break that cache for no reason.
    /// Duplicates are dropped, because a book bound both globally and by the
    /// conversation is one book, not two.
    pub fn world_books_of(&self, conversation: &Conversation) -> Vec<&WorldBook> {
        let mut ids: Vec<&str> = Vec::new();

        fn push<'a>(id: &'a str, ids: &mut Vec<&'a str>) {
            if !id.trim().is_empty() && !ids.iter().any(|seen| *seen == id) {
                ids.push(id);
            }
        }

        for id in &conversation.worldbook_ids {
            push(id, &mut ids);
        }
        for participant in &conversation.participants {
            if let Some(character) = self.character(&participant.character_id) {
                for id in &character.worldbook_ids {
                    push(id, &mut ids);
                }
            }
        }
        if let Some(persona) = conversation
            .persona_id
            .as_ref()
            .and_then(|id| self.persona(id))
        {
            for id in &persona.worldbook_ids {
                push(id, &mut ids);
            }
        }
        for id in &self.global_worldbook_ids {
            push(id, &mut ids);
        }

        ids.into_iter()
            .filter_map(|id| self.world_book(id))
            .collect()
    }

    /// Assemble everything the prompt for this conversation needs.
    ///
    /// This is the boundary the issue asks for: a conversation names what it
    /// uses, and the runtime resolves those names against stored entities.
    /// Nothing above this line has to know how the bindings are stored.
    pub fn prompt_context<'a>(
        &'a self,
        conversation: &'a Conversation,
        app_system_prompt: Option<&'a str>,
        user_input: &'a str,
    ) -> PromptContext<'a> {
        let participants = self.participants_of(conversation);
        let characters: Vec<&Character> = participants.iter().map(|(_, c)| *c).collect();
        let world_books = self.world_books_of(conversation);

        PromptContext {
            app_system_prompt,
            prompt_profile: conversation
                .prompt_profile_id
                .as_ref()
                .and_then(|id| self.prompt_profile(id)),
            persona: conversation
                .persona_id
                .as_ref()
                .and_then(|id| self.persona(id)),
            participants: participants.into_iter().map(|(p, _)| p).collect(),
            characters,
            world_books,
            world_book_budget: WorldBookBudget::default(),
            history: &conversation.transcript,
            user_input,
        }
    }

    /// Compile the prompt for a conversation, or `None` when it is not stored.
    pub fn compile_prompt(
        &self,
        conversation: &Conversation,
        app_system_prompt: Option<&str>,
        user_input: &str,
    ) -> Option<crate::PromptPlan> {
        let context = self.prompt_context(conversation, app_system_prompt, user_input);
        Some(crate::PromptCompiler::compile(&context))
    }
}

/// A transcript with no conversation around it, for callers that compile a
/// prompt from loose parts.
pub fn standalone_context<'a>(
    history: &'a Transcript,
    character: Option<&'a Character>,
    world_books: &'a [WorldBook],
    user_input: &'a str,
) -> PromptContext<'a> {
    let characters: Vec<&Character> = character.into_iter().collect();
    let participants = characters
        .iter()
        .map(|character| Participant::character(character.id.clone()))
        .collect();

    PromptContext {
        app_system_prompt: None,
        prompt_profile: None,
        persona: None,
        participants,
        characters,
        world_books: world_books.iter().collect(),
        world_book_budget: WorldBookBudget::default(),
        history,
        user_input,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Participant, ParticipantRole};

    fn library_with_bindings() -> Library {
        let mut library = Library {
            conversations: vec![Conversation {
                id: "conversation-1".to_string(),
                participants: vec![
                    Participant {
                        character_id: "character-1".to_string(),
                        role: ParticipantRole::Character,
                        display_name: None,
                    },
                    Participant {
                        character_id: "character-2".to_string(),
                        role: ParticipantRole::Narrator,
                        display_name: None,
                    },
                ],
                persona_id: Some("persona-1".to_string()),
                worldbook_ids: vec!["worldbook-1".to_string()],
                prompt_profile_id: Some("promptprofile-1".to_string()),
                ..Conversation::new("conversation-1")
            }],
            characters: vec![
                Character::new("character-1", "Kei"),
                Character::new("character-2", "Narrator"),
            ],
            personas: vec![Persona {
                worldbook_ids: vec!["worldbook-1".to_string()],
                ..Persona::new("persona-1", "Me")
            }],
            world_books: vec![
                WorldBook::new("worldbook-1", "Town"),
                WorldBook::new("worldbook-2", "Spare"),
            ],
            prompt_profiles: vec![PromptProfile::new("promptprofile-1", "Direct")],
            global_worldbook_ids: vec!["worldbook-2".to_string()],
        };
        library.characters[0].worldbook_ids = vec!["worldbook-1".to_string()];
        library
    }

    #[test]
    fn next_ids_stay_within_their_own_kind() {
        let mut library = Library::default();
        assert_eq!(library.next_character_id(), "character-1");
        library.upsert_character(Character::new(library.next_character_id(), "Kei"));
        assert_eq!(library.next_character_id(), "character-2");
        assert_eq!(library.next_persona_id(), "persona-1");
        assert_eq!(library.next_world_book_id(), "worldbook-1");
        assert_eq!(library.next_prompt_profile_id(), "promptprofile-1");
        assert_eq!(library.next_conversation_id(), "conversation-1");
    }

    #[test]
    fn next_id_never_reuses_an_existing_number() {
        let mut library = Library::default();
        library.upsert_character(Character::new("character-1", "Kei"));
        library.upsert_character(Character::new("character-3", "Mira"));
        let next = library.next_character_id();
        assert_ne!(next, "character-1");
        assert_ne!(next, "character-3");
        library.upsert_character(Character::new(next.clone(), "Sora"));
        assert_ne!(library.next_character_id(), next);
    }

    #[test]
    fn removing_a_character_drops_it_from_every_participant_list() {
        let mut library = library_with_bindings();
        assert!(library.remove_character("character-2"));
        let conversation = library.conversation("conversation-1").unwrap();
        assert_eq!(conversation.participants.len(), 1);
        assert_eq!(conversation.participants[0].character_id, "character-1");
        assert!(!library.remove_character("character-2"));
    }

    #[test]
    fn removing_a_world_book_clears_the_bindings_that_named_it() {
        let mut library = library_with_bindings();
        assert!(library.remove_world_book("worldbook-1"));
        let conversation = library.conversation("conversation-1").unwrap();
        assert!(conversation.worldbook_ids.is_empty());
        assert!(library.characters[0].worldbook_ids.is_empty());
        assert!(library.personas[0].worldbook_ids.is_empty());
        // The global book is untouched, so it is the only one still in force.
        let in_force: Vec<&str> = library
            .world_books_of(conversation)
            .iter()
            .map(|book| book.id.as_str())
            .collect();
        assert_eq!(in_force, vec!["worldbook-2"]);
    }

    #[test]
    fn removing_a_world_book_clears_the_global_binding() {
        let mut library = library_with_bindings();
        assert!(library.remove_world_book("worldbook-2"));
        assert!(library.global_worldbook_ids.is_empty());
    }

    #[test]
    fn removing_a_persona_or_prompt_profile_clears_the_conversation_pointer() {
        let mut library = library_with_bindings();
        assert!(library.remove_persona("persona-1"));
        assert!(library
            .conversation("conversation-1")
            .unwrap()
            .persona_id
            .is_none());
        assert!(library.remove_prompt_profile("promptprofile-1"));
        assert!(library
            .conversation("conversation-1")
            .unwrap()
            .prompt_profile_id
            .is_none());
    }
}

/// An id that cannot collide with one already in `existing`.
fn next_id<T, F>(existing: &[T], prefix: &str, id_of: F) -> String
where
    F: Fn(&T) -> &str,
{
    let mut number = existing.len() + 1;
    loop {
        let candidate = format!("{prefix}-{number}");
        if !existing.iter().any(|item| id_of(item) == candidate) {
            return candidate;
        }
        number += 1;
    }
}
