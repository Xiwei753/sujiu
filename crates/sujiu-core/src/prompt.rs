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
/// A provider caches the longest stable prefix of a request, so the honest way
/// to talk about caching is to say which region is allowed to change:
///
/// - `prefix` is session-stable: the app system prompt, the character's system
///   prompt, its definition and its example dialogue. Nothing in it is derived
///   from what was said, so it is byte-identical across every turn.
/// - `injections` is turn-local: world-book entries the current turn
///   triggered. They are recomputed from editable data on every turn, so a
///   different turn may activate a different set.
/// - `history` is the transcript exactly as it was already sent, including
///   every tool call and result. It only ever grows at the tail.
/// - `suffix` is the new content for this request: the near-history world-book
///   entries, the post-history instruction and the current user input.
///
/// A turn-local world-book injection is the one place where a new request can
/// differ from the previous one before its history. That is a real cache break
/// and it is stated as one rather than hidden: history itself is never rewritten
/// to make a request look tidy.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptPlan {
    /// Session-stable instruction block. Send this first, unchanged.
    pub prefix: Vec<PromptSegment>,
    /// Turn-local system content derived from editable data.
    pub injections: Vec<PromptSegment>,
    /// The transcript as previously sent, in order.
    pub history: Vec<ModelMessage>,
    /// New content for this request, after the history.
    pub suffix: Vec<PromptSegment>,
}

/// How one request continues the previous request's cache prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheContinuity {
    /// The previous request's bytes are still a prefix of this one.
    Unchanged,
    /// The stable prefix still matches, but this turn triggered different
    /// world-book content, so the provider's cache ends at that boundary.
    BrokeAtTurnLocalInjection,
    /// The stable prefix itself differs, so nothing before the new content can
    /// be reused. This means the character or app prompt was edited.
    BrokeAtStablePrefix,
}

impl PromptPlan {
    /// Every segment in wire order, for callers that render a plan as text.
    pub fn segments(&self) -> Vec<PromptSegment> {
        let mut segments = self.prefix.clone();
        segments.extend(self.injections.clone());
        segments.extend(history_segments(&self.history));
        segments.extend(self.suffix.clone());
        segments
    }

    /// The request messages, in wire order.
    pub fn model_messages(&self) -> Vec<ModelMessage> {
        let mut messages: Vec<ModelMessage> = self
            .prefix
            .iter()
            .chain(self.injections.iter())
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

    /// Compare this request with the previous one, so a caller (or a test) can
    /// tell a healthy append from a cache break instead of assuming.
    pub fn cache_continuity_with(&self, previous: &PromptPlan) -> CacheContinuity {
        if self.prefix != previous.prefix {
            return CacheContinuity::BrokeAtStablePrefix;
        }

        if self.injections != previous.injections {
            return CacheContinuity::BrokeAtTurnLocalInjection;
        }

        // The new request must not have rewritten anything the model already
        // read; it may only have appended to it.
        if !self.history.starts_with(&previous.history) {
            return CacheContinuity::BrokeAtStablePrefix;
        }

        CacheContinuity::Unchanged
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

        push_non_empty(
            &mut prefix,
            ChatRole::System,
            &character.example_dialogue,
            PromptSource::ExampleDialogue,
            40,
        );

        // World-book entries are derived from editable data against what was
        // just said, so they are not session-stable: the same session can
        // activate a different entry next turn. They therefore sit after the
        // stable prefix rather than inside it, which keeps the cacheable block
        // byte-identical and turns a changed entry into one declared cache
        // break instead of a silent rewrite.
        //
        // The BeforeCharacter/AfterCharacter distinction keeps its order between
        // the entries themselves; what it no longer does is straddle the
        // character definition, because that split is exactly what made the
        // prefix unstable.
        let mut injections = Vec::new();

        push_world_book(
            &mut injections,
            &active_entries,
            WorldBookPosition::BeforeCharacter,
        );

        push_world_book(
            &mut injections,
            &active_entries,
            WorldBookPosition::AfterCharacter,
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

        // Near-history entries and the post-history instruction stay after the
        // history, where they still read as "just before the user's turn".
        // Neither is replayed from the transcript, and neither has to be:
        // both are re-derived from the character and the world book every turn,
        // so they carry conversation state only as long as that data is
        // unchanged. Anything the model must remember lives in the transcript.
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
            injections,
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

    /// A character with everything that can vary per turn: a keyed world-book
    /// entry, a near-history entry and a post-history instruction.
    fn turning_character() -> Character {
        let mut character = lore_character();
        character.system_prompt = "You are Aerin.".into();
        character.description = "A tower keeper.".into();
        character.example_dialogue = "USER: hello\nAERIN: welcome".into();
        character.post_history_instructions = "Never speak for the user.".into();
        let world_book = character.world_book.as_mut().expect("world book");
        world_book.entries.push(WorldBookEntry {
            id: "harbor".into(),
            name: "Harbor".into(),
            content: "The harbor freezes every winter.".into(),
            keys: vec!["harbor".into()],
            enabled: true,
            constant: true,
            priority: 5,
            position: WorldBookPosition::NearHistory,
            extensions: Default::default(),
        });
        character
    }

    #[test]
    fn the_stable_prefix_survives_a_turn_that_triggers_new_world_book_content() {
        let character = turning_character();
        let quiet = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "we stay in the forest",
        );
        let lore = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "take me to the Black Tower",
        );

        // The keyed entry is turn-local, so it is not in the stable block.
        assert!(quiet.injections.is_empty());
        assert!(lore
            .injections
            .iter()
            .any(|segment| segment.content.contains("north of the capital")));
        // What the provider can cache is untouched, and the change is reported
        // as the one break it is instead of being hidden.
        assert_eq!(quiet.prefix, lore.prefix);
        assert_eq!(
            lore.cache_continuity_with(&quiet),
            CacheContinuity::BrokeAtTurnLocalInjection
        );
    }

    #[test]
    fn a_world_book_entry_that_stops_being_triggered_is_also_a_declared_break() {
        let character = turning_character();
        let lore = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "take me to the Black Tower",
        );
        let quiet = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "we stay in the forest",
        );

        assert_eq!(quiet.prefix, lore.prefix);
        assert_eq!(
            quiet.cache_continuity_with(&lore),
            CacheContinuity::BrokeAtTurnLocalInjection
        );
    }

    #[test]
    fn near_history_and_post_history_content_never_enter_the_stable_prefix() {
        let plan = PromptCompiler::compile(
            Some("stable system prompt"),
            &turning_character(),
            &Transcript::default(),
            "we stay in the forest",
        );

        assert!(!plan
            .prefix
            .iter()
            .any(|segment| segment.content.contains("freezes every winter")));
        assert!(plan
            .suffix
            .iter()
            .any(|segment| segment.content.contains("freezes every winter")));
        assert!(plan
            .suffix
            .iter()
            .any(|segment| segment.content.contains("Never speak for the user.")));
    }

    #[test]
    fn editing_the_character_is_reported_as_a_break_in_the_stable_prefix() {
        let mut edited = turning_character();
        edited.system_prompt = "You are Aerin the second.".into();

        let before = PromptCompiler::compile(
            Some("stable system prompt"),
            &turning_character(),
            &Transcript::default(),
            "hello",
        );
        let after = PromptCompiler::compile(
            Some("stable system prompt"),
            &edited,
            &Transcript::default(),
            "hello",
        );

        assert_eq!(
            after.cache_continuity_with(&before),
            CacheContinuity::BrokeAtStablePrefix
        );
    }

    #[test]
    fn an_ordinary_next_turn_only_appends_to_what_was_already_sent() {
        let character = turning_character();
        let mut transcript = Transcript::default();
        let mut turn = Turn::new("turn-1", "who keeps the tower?");
        turn.steps.push(AssistantStep::text_only("Aerin does."));
        transcript.push(turn);

        let first = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "who keeps the tower?",
        );
        let second = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &transcript,
            "and the harbor?",
        );

        assert_eq!(
            second.cache_continuity_with(&first),
            CacheContinuity::Unchanged
        );
        // The previous history is still a prefix of the new one: nothing the
        // model already read was rewritten.
        assert!(second.history.starts_with(&first.history));
        assert_eq!(
            second.model_messages().last(),
            Some(&ModelMessage::user("and the harbor?"))
        );
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
