use serde::{Deserialize, Serialize};

use crate::{Character, ChatMessage, ChatRole, WorldBookEntry, WorldBookPosition};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptSource {
    AppSystem,
    CharacterSystem,
    CharacterDefinition,
    WorldBook,
    ExampleDialogue,
    ChatHistory,
    PostHistoryInstruction,
    CurrentUserInput,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PromptSegment {
    pub role: ChatRole,
    pub content: String,
    pub source: PromptSource,

    /// Client-side retention/ordering priority. This is not a model attention weight.
    pub priority: i32,
}

/// Instruction applied to every conversation.
///
/// This is model semantics, not user-visible copy, so it lives in the shared
/// runtime instead of a platform bridge. It is English by design: it is read by
/// the model, never rendered, so it must not follow the interface language.
pub const DEFAULT_APP_SYSTEM_PROMPT: &str = "You are Sujiu, an AI role-play client. Stay in character, and answer in the language the user writes in.";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptPlan {
    pub segments: Vec<PromptSegment>,
}

pub struct PromptCompiler;

impl PromptCompiler {
    pub fn compile(
        app_system_prompt: Option<&str>,
        character: &Character,
        history: &[ChatMessage],
        user_input: &str,
    ) -> PromptPlan {
        let scan_text = history
            .iter()
            .map(|message| message.content.as_str())
            .chain(std::iter::once(user_input))
            .collect::<Vec<_>>()
            .join("\n");

        let mut active_entries = character
            .world_book
            .as_ref()
            .map(|book| {
                book.entries
                    .iter()
                    .filter(|entry| entry.is_active_for(&scan_text))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        active_entries.sort_by_key(|entry| entry.priority);

        let mut segments = Vec::new();

        push_non_empty(
            &mut segments,
            ChatRole::System,
            app_system_prompt.unwrap_or_default(),
            PromptSource::AppSystem,
            100,
        );

        push_world_book(
            &mut segments,
            &active_entries,
            WorldBookPosition::BeforeCharacter,
        );

        push_non_empty(
            &mut segments,
            ChatRole::System,
            &character.system_prompt,
            PromptSource::CharacterSystem,
            95,
        );

        let definition = character_definition(character);
        push_non_empty(
            &mut segments,
            ChatRole::System,
            &definition,
            PromptSource::CharacterDefinition,
            90,
        );

        push_world_book(
            &mut segments,
            &active_entries,
            WorldBookPosition::AfterCharacter,
        );

        push_non_empty(
            &mut segments,
            ChatRole::System,
            &character.example_dialogue,
            PromptSource::ExampleDialogue,
            40,
        );

        for message in history {
            if !message.content.trim().is_empty() {
                segments.push(PromptSegment {
                    role: message.role,
                    content: message.content.clone(),
                    source: PromptSource::ChatHistory,
                    priority: 70,
                });
            }
        }

        push_world_book(
            &mut segments,
            &active_entries,
            WorldBookPosition::NearHistory,
        );

        push_non_empty(
            &mut segments,
            ChatRole::System,
            &character.post_history_instructions,
            PromptSource::PostHistoryInstruction,
            98,
        );

        push_non_empty(
            &mut segments,
            ChatRole::User,
            user_input,
            PromptSource::CurrentUserInput,
            1000,
        );

        PromptPlan { segments }
    }
}

fn character_definition(character: &Character) -> String {
    let mut lines = Vec::new();

    if !character.name.trim().is_empty() {
        lines.push(format!("Character: {}", character.name.trim()));
    }
    if !character.description.trim().is_empty() {
        lines.push(format!("Description:\n{}", character.description.trim()));
    }
    if !character.personality.trim().is_empty() {
        lines.push(format!("Personality:\n{}", character.personality.trim()));
    }
    if !character.scenario.trim().is_empty() {
        lines.push(format!("Scenario:\n{}", character.scenario.trim()));
    }

    lines.join("\n\n")
}

fn push_world_book(
    output: &mut Vec<PromptSegment>,
    entries: &[&WorldBookEntry],
    position: WorldBookPosition,
) {
    for entry in entries.iter().filter(|entry| entry.position == position) {
        push_non_empty(
            output,
            ChatRole::System,
            &entry.content,
            PromptSource::WorldBook,
            60 + entry.priority,
        );
    }
}

fn push_non_empty(
    output: &mut Vec<PromptSegment>,
    role: ChatRole,
    content: &str,
    source: PromptSource,
    priority: i32,
) {
    let content = content.trim();
    if !content.is_empty() {
        output.push(PromptSegment {
            role,
            content: content.to_owned(),
            source,
            priority,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorldBook, WorldBookEntry};

    #[test]
    fn triggered_world_book_entry_enters_prompt() {
        let mut character = Character {
            name: "Aerin".into(),
            ..Character::default()
        };
        character.world_book = Some(WorldBook {
            name: "World".into(),
            entries: vec![WorldBookEntry {
                id: "black-tower".into(),
                name: "Black Tower".into(),
                content: "The Black Tower stands north of the capital.".into(),
                keys: vec!["Black Tower".into()],
                enabled: true,
                constant: false,
                priority: 10,
                position: WorldBookPosition::AfterCharacter,
                extensions: Default::default(),
            }],
            extensions: Default::default(),
        });

        let plan = PromptCompiler::compile(None, &character, &[], "Take me to the Black Tower.");

        assert!(plan.segments.iter().any(|segment| {
            segment.source == PromptSource::WorldBook
                && segment.content.contains("north of the capital")
        }));
    }

    #[test]
    fn unrelated_world_book_entry_stays_out() {
        let mut character = Character::default();
        character.world_book = Some(WorldBook {
            name: "World".into(),
            entries: vec![WorldBookEntry {
                id: "harbor".into(),
                name: "Harbor".into(),
                content: "The harbor is closed at night.".into(),
                keys: vec!["harbor".into()],
                enabled: true,
                constant: false,
                priority: 0,
                position: WorldBookPosition::AfterCharacter,
                extensions: Default::default(),
            }],
            extensions: Default::default(),
        });

        let plan = PromptCompiler::compile(None, &character, &[], "We stay in the forest.");

        assert!(!plan
            .segments
            .iter()
            .any(|segment| segment.source == PromptSource::WorldBook));
    }
}
