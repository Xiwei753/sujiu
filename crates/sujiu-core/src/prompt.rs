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
/// A provider caches the longest stable prefix of a request. Rather than claim
/// the request is append-only, this type says which part of it is allowed to
/// move, and [`PromptPlan::cache_continuity_with`] compares two real requests
/// against each other:
///
/// - `prefix` is the prompt block: the app system prompt, the world's
///   before-character entries, the character's system prompt and definition,
///   its after-character entries and its example dialogue. Each piece sits
///   where its meaning says it belongs, so a world-book entry never moves just
///   to make the block easier to cache. Constant entries never change the
///   block; keyed entries can, and that is reported as a world-book break.
/// - `history` is the transcript exactly as it was already sent, including
///   every tool call and result. It only ever grows at the tail, and only
///   compaction ever rewrites it.
/// - `suffix` is turn-local content derived from the current input: the
///   near-history world-book entries, the post-history instruction and the new
///   user message.
///
/// The suffix is why a request is not always append-only. Last turn's
/// near-history entry, post-history instruction and user message sat at the very
/// end; once the assistant has answered, the new turn's copy of them moves down
/// behind that answer. Everything before them is still shared, so the provider
/// loses only that tail — but it is a real break and it is reported as one.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptPlan {
    /// The prompt block, in semantic order. Send this first, unchanged.
    pub prefix: Vec<PromptSegment>,
    /// The transcript as previously sent, in order.
    pub history: Vec<ModelMessage>,
    /// Turn-local content for this request, after the history.
    pub suffix: Vec<PromptSegment>,
}

/// How one request continues the previous request's cache prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheContinuity {
    /// The previous request's messages are still a prefix of this one, so the
    /// provider reuses its cache and only reads the new tail.
    Unchanged,
    /// The prompt block and the transcript still match. The break is the
    /// previous request's turn-local tail, which this turn pushed down behind
    /// the assistant's answer. The shared part is still cacheable.
    BrokeAtTurnLocalTail,
    /// The prompt block's world-book entries activated a different set. A
    /// constant entry can never cause this; a keyed one can.
    BrokeAtWorldBook,
    /// The app prompt or the character changed, so the prompt block is
    /// different from the first message on.
    BrokeAtPromptBlock,
    /// The transcript was rewritten instead of extended. Only compaction does
    /// this, and it is allowed to.
    BrokeAtHistory,
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

    /// How many leading messages this request and `previous` have in common.
    ///
    /// That count is what a provider can still read from cache, so it is worth
    /// reporting directly instead of inferring it from regions.
    pub fn shared_prefix_messages(&self, previous: &PromptPlan) -> usize {
        previous
            .model_messages()
            .iter()
            .zip(self.model_messages().iter())
            .take_while(|(before, after)| before == after)
            .count()
    }

    /// Compare this request with the previous one and report what happened to
    /// the cache.
    ///
    /// The judgement is made on the two real message sequences, not on the
    /// internal regions: the regions on their own cannot see that last turn's
    /// suffix has been pushed down behind this turn's answer, which is a real
    /// break that a region-only comparison reports as a healthy append.
    pub fn cache_continuity_with(&self, previous: &PromptPlan) -> CacheContinuity {
        if self
            .model_messages()
            .starts_with(&previous.model_messages())
        {
            return CacheContinuity::Unchanged;
        }

        // Everything before this index is the prompt block. A shared count equal
        // to the block length means the block survived intact and the break sits
        // at its edge, which is the history's or the turn-local tail's business
        // rather than the block's.
        let block_len = previous.prefix.len();

        if self.shared_prefix_messages(previous) < block_len {
            return if self.broke_on_the_world_book(previous) {
                CacheContinuity::BrokeAtWorldBook
            } else {
                CacheContinuity::BrokeAtPromptBlock
            };
        }

        if !self.history.starts_with(&previous.history) {
            return CacheContinuity::BrokeAtHistory;
        }

        // The block and the transcript are intact, so the only thing that moved
        // is what follows them: the ordinary cost of a turn-local tail.
        CacheContinuity::BrokeAtTurnLocalTail
    }

    /// Whether the first difference between two prompt blocks is a world-book
    /// entry appearing or disappearing rather than an edited character.
    fn broke_on_the_world_book(&self, previous: &PromptPlan) -> bool {
        for (new, old) in self.prefix.iter().zip(previous.prefix.iter()) {
            if new != old {
                return new.source == PromptSource::WorldBook
                    || old.source == PromptSource::WorldBook;
            }
        }

        // One block ran out of segments before the other.
        self.prefix
            .iter()
            .chain(previous.prefix.iter())
            .any(|segment| segment.source == PromptSource::WorldBook)
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
            // The reasoning sidecar is deliberately left out: this is the flat
            // text view the world book is scanned against, and private
            // reasoning is not something a keyword should match on.
            ModelMessage::Assistant { content, calls, .. } => {
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

        // The prompt block is assembled in semantic order, which is also the
        // order its meaning requires: a before-character entry really does come
        // before the character and an after-character entry really does follow
        // the definition. Relocating them to make the block easier to cache
        // would trade prompt correctness for a cache hit, which is the wrong
        // trade — a cache miss is a performance cost, not a semantic one.
        //
        // A constant entry is active on every turn, so it never changes this
        // block. A keyed entry can, and when it does the difference is reported
        // as a world-book break instead of being hidden.
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

        // Near-history entries and the post-history instruction stay after the
        // history, where they still read as "just before the user's turn".
        // Neither is replayed from the transcript, and neither has to be: both
        // are re-derived from the character and the world book every turn, so
        // they carry conversation state only as long as that data is unchanged.
        // Anything the model must remember lives in the transcript.
        //
        // This is also why a request is not always append-only: last turn's copy
        // of this tail moves down behind the assistant's answer. The prompt block
        // and the transcript are untouched, so the provider keeps all of them
        // and re-reads only the tail.
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

    /// The compacted summary is part of what the model can see, so it has to be
    /// part of what the world book is scanned against. Otherwise a long
    /// conversation silently changes which lore entries trigger the moment it
    /// gets compacted, and the summary is the only place the old detail lives.
    #[test]
    fn a_compacted_summary_keeps_triggering_lore_that_only_it_still_mentions() {
        let mut transcript = Transcript::default();

        // Only the OLD turn mentions the tower. If the retained turn mentioned
        // it too, the keyword would still match the live history and the test
        // would pass whether or not the summary is scanned.
        let mut old = Turn::new("turn-0", "Where do we go from here?");
        old.steps.push(crate::transcript::AssistantStep {
            text: Some("We should visit the Black Tower before dawn.".into()),
            ..crate::transcript::AssistantStep::default()
        });
        transcript.push(old);

        let mut kept = Turn::new("turn-1", "And then?");
        kept.steps.push(crate::transcript::AssistantStep {
            text: Some("The road is long.".into()),
            ..crate::transcript::AssistantStep::default()
        });
        transcript.push(kept);

        transcript.compact(1, |_, _| {
            "Earlier the pair agreed to visit the Black Tower before dawn.".to_string()
        });

        // The only turn that mentioned the tower is now only in the summary.
        assert_eq!(transcript.turns.len(), 1);
        assert!(!transcript.turns[0].steps[0]
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("Black Tower"));
        assert!(transcript.scan_text().contains("Black Tower"));

        let plan = PromptCompiler::compile(None, &lore_character(), &transcript, "Carry on.");

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
            |message| matches!(message, ModelMessage::Assistant { calls, .. }
                if calls[0].id == "call-1")
        ));
        assert!(messages.iter().any(
            |message| matches!(message, ModelMessage::ToolResult { call_id, .. } if call_id == "call-1")
        ));
        assert!(messages.iter().any(
            |message| matches!(message, ModelMessage::Assistant { content: Some(content), calls, .. }
                if content == "A caller held the line open." && calls.is_empty())
        ));
        // Ordering is the protocol ordering.
        let call_at = messages
            .iter()
            .position(|m| matches!(m, ModelMessage::Assistant { calls, .. } if !calls.is_empty()))
            .unwrap();
        let answer_at = messages
            .iter()
            .position(|m| matches!(m, ModelMessage::Assistant { calls, .. } if calls.is_empty()))
            .unwrap();
        let result_at = messages
            .iter()
            .position(|m| matches!(m, ModelMessage::ToolResult { .. }))
            .unwrap();
        assert!(call_at < result_at);
        assert!(result_at < answer_at);
    }

    /// A character with a keyed world-book entry, a constant near-history entry
    /// and a post-history instruction: everything that can vary per turn.
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

    /// A character with nothing that varies per turn: no near-history entry
    /// and no post-history instruction, so the only thing a next turn adds to a
    /// request is the answer and the new input.
    fn steady_character() -> Character {
        let mut character = turning_character();
        character.post_history_instructions = String::new();
        let world_book = character.world_book.as_mut().expect("world book");
        // Near-history content belongs to the turn-local tail, so a character
        // without any of it is what makes "the request only appends"
        // expressible at all.
        world_book.entries[1].enabled = false;
        character
    }

    fn index_of(plan: &PromptPlan, needle: &str) -> usize {
        plan.prefix
            .iter()
            .position(|segment| segment.content.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} must be in the prompt block: {:?}", plan.prefix))
    }

    #[test]
    fn a_world_book_entry_keeps_the_position_its_meaning_asks_for() {
        let mut character = steady_character();
        let world_book = character.world_book.as_mut().expect("world book");
        world_book.entries.push(WorldBookEntry {
            id: "dawn".into(),
            name: "Dawn".into(),
            content: "Dawn comes late in winter.".into(),
            keys: vec![],
            enabled: true,
            constant: true,
            priority: 1,
            position: WorldBookPosition::BeforeCharacter,
            extensions: Default::default(),
        });

        let plan = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &Transcript::default(),
            "Take me to the Black Tower.",
        );

        // Before-character really is before the character, and the
        // after-character entry really is after the definition. Relocating
        // either to win a cache hit would change what the prompt means.
        assert!(index_of(&plan, "Dawn comes late") < index_of(&plan, "You are Aerin."));
        assert!(index_of(&plan, "A tower keeper") < index_of(&plan, "north of the capital"));
        assert!(index_of(&plan, "north of the capital") < index_of(&plan, "AERIN: welcome"));
    }

    #[test]
    fn a_turn_that_triggers_new_world_book_content_declares_a_world_book_break() {
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

        // The entry keeps its semantic position, so the prompt block really
        // does differ, and the report says so instead of calling it a healthy
        // append.
        assert!(quiet
            .prefix
            .iter()
            .all(|segment| !segment.content.contains("north of the capital")));
        assert!(lore
            .prefix
            .iter()
            .any(|segment| segment.content.contains("north of the capital")));
        assert_eq!(
            lore.cache_continuity_with(&quiet),
            CacheContinuity::BrokeAtWorldBook
        );
        assert_eq!(
            quiet.cache_continuity_with(&lore),
            CacheContinuity::BrokeAtWorldBook
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

        assert_eq!(
            quiet.cache_continuity_with(&lore),
            CacheContinuity::BrokeAtWorldBook
        );
    }

    #[test]
    fn near_history_and_post_history_content_never_enter_the_prompt_block() {
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
    fn editing_the_character_is_reported_as_a_break_in_the_prompt_block() {
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
            CacheContinuity::BrokeAtPromptBlock
        );
    }

    /// A request with a turn-local tail is not a prefix of the next one, and
    /// the runtime has to say so rather than report a clean append.
    #[test]
    fn a_turn_local_tail_is_reported_as_a_break_in_the_tail_only() {
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
            CacheContinuity::BrokeAtTurnLocalTail
        );
        // The claim is checked against the real messages rather than against
        // the internal regions, which is what makes it worth anything.
        assert!(
            !second.model_messages().starts_with(&first.model_messages()),
            "a pushed-down tail is not an append"
        );
        // Everything the provider can keep is still shared: the prompt block and
        // the whole transcript the first request carried.
        assert_eq!(first.prefix, second.prefix);
        assert_eq!(second.shared_prefix_messages(&first), first.prefix.len());
    }

    /// With no turn-local tail, the next request really is the previous one plus
    /// an answer and a new user message, so the whole cache survives.
    #[test]
    fn without_a_turn_local_tail_the_next_request_only_appends() {
        let character = steady_character();
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
            "and the cold?",
        );

        assert!(second.model_messages().starts_with(&first.model_messages()));
        assert_eq!(
            second.cache_continuity_with(&first),
            CacheContinuity::Unchanged
        );
        assert_eq!(
            second.shared_prefix_messages(&first),
            first.model_messages().len()
        );
        assert_eq!(
            second.model_messages().last(),
            Some(&ModelMessage::user("and the cold?"))
        );
    }

    /// Compaction is the one thing allowed to rewrite the transcript, and it
    /// has to be reported rather than mistaken for a prompt problem.
    #[test]
    fn compaction_is_reported_as_a_history_break() {
        let character = steady_character();
        let mut transcript = Transcript::default();
        for index in 0..3 {
            let mut turn = Turn::new(&format!("turn-{index}"), &format!("question {index}"));
            turn.steps
                .push(AssistantStep::text_only(&format!("answer {index}")));
            transcript.push(turn);
        }

        // Before compaction the session still carries all three turns.
        let mut uncompacted = transcript.clone();
        uncompacted.turns.retain(|turn| turn.id == "turn-2");
        let before = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &uncompacted,
            "and now?",
        );

        assert!(transcript.compact(1, |turns, _| { format!("{} earlier turns", turns.len()) }));

        let after = PromptCompiler::compile(
            Some("stable system prompt"),
            &character,
            &transcript,
            "and now?",
        );

        assert_eq!(after.prefix, before.prefix);
        assert_eq!(
            after.cache_continuity_with(&before),
            CacheContinuity::BrokeAtHistory
        );
    }

    #[test]
    fn the_prompt_block_is_identical_across_turns_of_a_session() {
        let character = steady_character();
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

        let Some(ModelMessage::Assistant { calls, .. }) = messages.get(1) else {
            panic!("the assistant step must stay a tool call");
        };
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].name, "search_context");
    }
}
