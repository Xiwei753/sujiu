use serde::{Deserialize, Serialize};

use crate::{
    Character, Conversation, Participant, Persona, PromptContext, PromptProfile, Transcript,
    WorldBook,
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

    pub fn world_book(&self, id: &str) -> Option<&WorldBook> {
        self.world_books.iter().find(|item| item.id == id)
    }

    pub fn prompt_profile(&self, id: &str) -> Option<&PromptProfile> {
        self.prompt_profiles.iter().find(|item| item.id == id)
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
        history,
        user_input,
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
