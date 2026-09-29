use serde::{Deserialize, Serialize};

use crate::{
    model::ModelMessage, Character, ChatRole, Transcript, WorldBookEntry, WorldBookPosition,
};

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

/// A compiled prompt, split so its cache behaviour is visible.
///
/// The three parts exist because a provider caches the longest stable prefix of
/// a request. `prefix` is the instruction block and must stay byte-identical
/// across turns of a session; `history` is the transcript exactly as it was
/// already sent, including every tool call and result; `suffix` is the new
/// content for this request. A turn appends to `history` and never rewrites
/// what came before it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptPlan {
    /// Stable instruction block. Send this first, unchanged.
    pub prefix: Vec<PromptSegment>,
    /// The transcript as previously sent, in order.
    pub history: Vec<ModelMessage>,
    /// New content for this request.
    pub suffix: Vec<PromptSegment>,
}

impl PromptPlan {
    /// Every segment in wire order, for callers that render a plan as text.
    pub fn segments(&self) -> Vec<PromptSegment> {
        let mut segments = self.prefix.clone();
        segments.extend(history_segments(&self.history));
        segments.extend(self.suffix.clone());
        segments
    }

    /// The request messages, in wire order.
    pub fn model_messages(&self) -> Vec<ModelMessage> {
        let mut messages: Vec<ModelMessage> = self
            .prefix
            .iter()
            .map(|segment| ModelMessage::Text {
                role: segment.role.into(),
                content: segment.content.clone(),
            })
            .collect();

        messages.extend(self.history.iter().cloned());
        messages.extend(self.suffix.iter().map(|segment| ModelMessage::Text {
            role: segment.role.into(),
            content: segment.content.clone(),
        }));

        messages
    }
}

/// Flatten history into display segments.
///
/// Tool steps are shown as text here because this is a rendering view, not the
/// request. The request keeps the structured form through
/// [`PromptPlan::model_messages`].
fn history_segments(history: &[ModelMessage]) -> Vec<PromptSegment> {
    let mut segments = Vec::new();

    for message in history {
        match message {
            ModelMessage::Text { role, content } => {
                if !content.trim().is_empty() {
                    segments.push(PromptSegment {
                        role: (*role).into(),
                        content: content.clone(),
                        source: PromptSource::ChatHistory,
                        priority: 70,
                    });
                }
            }
            ModelMessage::AssistantToolCalls { content, calls } => {
                if let Some(content) = content.as_ref().filter(|c| !c.trim().is_empty()) {
                    segments.push(PromptSegment {
                        role: ChatRole::Assistant,
                        content: content.clone(),
                        source: PromptSource::ChatHistory,
                        priority: 70,
                    });
                }

                for call in calls {
                    segments.push(PromptSegment {
                        role: ChatRole::Assistant,
                        content: format!("[tool call] {} {}", call.name, call.id),
                        source: PromptSource::ChatHistory,
                        priority: 70,
                    });
                }
            }
            ModelMessage::ToolResult {
                call_id, content, ..
            } => segments.push(PromptSegment {
                role: ChatRole::Assistant,
                content: format!("[tool result] {call_id}: {content}"),
                source: PromptSource::ChatHistory,
                priority: 70,
            }),
        }
    }

    segments
}

pub struct PromptCompiler;

impl PromptCompiler {
    /// Compile one request.
    ///
    /// `history` is the transcript of everything already said in this session.
    /// It is passed through unchanged, so the request this returns shares a
    /// prefix with the previous one and the provider can reuse its cache.
    pub fn compile(
        app_system_prompt: Option<&str>,
        character: &Character,
        history: &Transcript,
        user_input: &str,
    ) -> PromptPlan {
        let scan_text = format!("{}\n{}", history.scan_text(), user_input);

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

        let mut prefix = Vec::new();

        push_non_empty(
            &mut prefix,
            ChatRole::System,
            app_system_prompt.unwrap_or_default(),
            PromptSource::AppSystem,
            100,
        );

        push_world_book(
            &mut prefix,
            &active_entries,
            WorldBookPosition::BeforeCharacter,
        );

        push_non_empty(
            &mut prefix,
            ChatRole::System,
            &character.system_prompt,
            PromptSource::CharacterSystem,
            95,
        );

        let definition = character_definition(character);
        push_non_empty(
            &mut prefix,
            ChatRole::System,
            &definition,
            PromptSource::CharacterDefinition,
            90,
        );

        push_world_book(
            &mut prefix,
            &active_entries,
            WorldBookPosition::AfterCharacter,
        );

        push_non_empty(
            &mut prefix,
            ChatRole::System,
            &character.example_dialogue,
            PromptSource::ExampleDialogue,
            40,
        );

        // The transcript goes through exactly as stored: every assistant step,
        // tool call and tool result the model saw last time is still here.
        //
        // If the session was compacted, the summary leads the history. The turns
        // it stands in for stay retrievable, but the model is told they are a
        // summary rather than being handed a gap it never saw close.
        let mut history_messages = Vec::new();
        if let Some(compacted) = history.compacted.as_ref() {
            if !compacted.summary.trim().is_empty() {
                history_messages.push(ModelMessage::user(format!(
                    "Summary of the earlier conversation:\n{}",
                    compacted.summary.trim()
                )));
            }
        }
        history_messages.extend(crate::transcript::model_messages_from_transcript(history));
        let history = history_messages;

        let mut suffix = Vec::new();

        push_world_book(&mut suffix, &active_entries, WorldBookPosition::NearHistory);

        push_non_empty(
            &mut suffix,
            ChatRole::System,
            &character.post_history_instructions,
            PromptSource::PostHistoryInstruction,
            98,
        );

        push_non_empty(
            &mut suffix,
            ChatRole::User,
            user_input,
            PromptSource::CurrentUserInput,
            1000,
        );

        PromptPlan {
            prefix,
            history,
            suffix,
        }
    }

    /// The next history for a session, given the messages just sent.
    ///
    /// This is what keeps the transcript append-only: the previous history is
    /// extended with the messages this turn produced instead of being rebuilt.
    pub fn extend_history(
        history: &[ModelMessage],
        produced: &[ModelMessage],
    ) -> Vec<ModelMessage> {
        let mut extended = history.to_vec();
        extended.extend(produced.iter().cloned());
        extended
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
    use crate::{AssistantStep, ToolResultRecord, Turn, WorldBook, WorldBookEntry};
    use serde_json::json;

    fn lore_character() -> Character {
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
        character
    }

    #[test]
    fn triggered_world_book_entry_enters_prompt() {
        let plan = PromptCompiler::compile(
            None,
            &lore_character(),
            &Transcript::default(),
            "Take me to the Black Tower.",
        );

        assert!(plan.segments().iter().any(|segment| {
            segment.source == PromptSource::WorldBook
                && segment.content.contains("north of the capital")
        }));
    }

    #[test]
    fn unrelated_world_book_entry_stays_out() {
        let plan = PromptCompiler::compile(
            None,
            &lore_character(),
            &Transcript::default(),
            "We stay in the forest.",
        );

        assert!(!plan
            .segments()
            .iter()
            .any(|segment| segment.source == PromptSource::WorldBook));
    }

    #[test]
    fn tool_steps_from_an_earlier_turn_stay_in_the_next_request() {
        let mut turn = Turn::new("turn-1", "what is in the lore?");
        turn.steps.push(AssistantStep {
            tool_calls: vec![crate::ToolCallRecord {
                id: "call-1".into(),
                name: "search_context".into(),
                title: None,
                arguments: json!({"query": "lore"}),
                result: ToolResultRecord::default(),
            }],
            ..AssistantStep::default()
        });
        turn.steps
            .push(AssistantStep::text_only("A caller held the line open."));

        let mut history = Transcript::default();
        history.push(turn);

        let plan =
            PromptCompiler::compile(None, &Character::default(), &history, "and who called?");

        let messages = plan.model_messages();

        // The intermediate call and its result are in the next request, not
        // just the final answer.
        assert!(messages.iter().any(
            |message| matches!(message, ModelMessage::AssistantToolCalls { calls, .. }
                if calls[0].id == "call-1")
        ));
        assert!(messages.iter().any(
            |message| matches!(message, ModelMessage::ToolResult { call_id, .. } if call_id == "call-1")
        ));
        assert!(messages.iter().any(
            |message| matches!(message, ModelMessage::Text { content, .. } if content == "A caller held the line open.")
        ));
        // Ordering is the protocol ordering.
        let call_at = messages
            .iter()
            .position(|m| matches!(m, ModelMessage::AssistantToolCalls { .. }))
            .unwrap();
        let result_at = messages
            .iter()
            .position(|m| matches!(m, ModelMessage::ToolResult { .. }))
            .unwrap();
        assert!(call_at < result_at);
    }

    #[test]
    fn the_prefix_is_identical_across_turns_of_a_session() {
        let character = lore_character();
        let first = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "one",
        );
        let second = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "two",
        );

        assert_eq!(first.prefix, second.prefix);
    }

    #[test]
    fn extending_history_keeps_what_came_before() {
        let produced = vec![ModelMessage::assistant("answer")];
        let extended = PromptCompiler::extend_history(&[ModelMessage::user("question")], &produced);

        assert_eq!(extended.len(), 2);
        assert_eq!(extended[0], ModelMessage::user("question"));
    }

    #[test]
    fn a_tool_call_survives_a_transcript_round_trip_with_its_id() {
        let mut turn = Turn::new("turn-1", "hello");
        turn.steps.push(AssistantStep {
            tool_calls: vec![crate::ToolCallRecord {
                id: "call-1".into(),
                name: "search_context".into(),
                title: None,
                arguments: json!({}),
                result: ToolResultRecord::default(),
            }],
            ..AssistantStep::default()
        });

        let json = serde_json::to_string(&turn).unwrap();
        let restored: Turn = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.steps[0].tool_calls[0].id, "call-1");
        let messages = crate::transcript::model_messages_from_transcript(&Transcript {
            turns: vec![restored],
            compacted: None,
        });

        let Some(ModelMessage::AssistantToolCalls { calls, .. }) = messages.get(1) else {
            panic!("the assistant step must stay a tool call");
        };
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].name, "search_context");
    }
}
