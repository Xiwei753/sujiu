use serde::{Deserialize, Serialize};

use crate::{
    model::ModelMessage, Character, ChatRole, Persona, PromptPosition, PromptProfile, Transcript,
    WorldBook, WorldBookEntry, WorldBookPosition,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptSource {
    AppSystem,
    /// A prompt profile's own system prompt, which replaces the app one.
    ProfileSystem,
    /// A prompt profile's user-side instruction, sent as a user message.
    ProfileUser,
    /// One of a prompt profile's own extra segments, at the role and position
    /// that segment declares.
    ProfileSegment,
    CharacterSystem,
    CharacterDefinition,
    /// Who the user is, as the system side of the conversation states it.
    Persona,
    /// What the persona asks of the model, in the user's own voice.
    PersonaUser,
    FormatRules,
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
/// The world-book entries inside `prefix` and `suffix` are also the ones that
/// fit the request's budget. Anything that matched and did not fit is listed in
/// `dropped_world_book_entries`, so a request that quietly sent less lore than
/// the last one can be told apart from a request that sent all of it.
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
    /// World-book entries that matched this turn and did not fit its budget.
    ///
    /// Reported rather than dropped silently, because a lore entry the runtime
    /// chose not to send is a fact about this request, not an absence. Every
    /// entry here is still one `search_context` call away — the budget decides
    /// what is *pushed* into a prompt, never what *exists* in the book.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped_world_book_entries: Vec<DroppedWorldBookEntry>,
}

/// One world-book entry left out of a request, and which limit left it out.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DroppedWorldBookEntry {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Whether the book's author marked this entry as always-on.
    ///
    /// Worth keeping: a dropped keyed entry is a budget decision, while a
    /// dropped constant entry means the book asks for more than the budget
    /// allows, which is a fact about the book.
    #[serde(default)]
    pub constant: bool,
    pub reason: WorldBookDropReason,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorldBookDropReason {
    /// Too many entries already fit.
    EntryLimit,
    /// The character budget was spent first.
    CharLimit,
}

/// How much of a world book one request's prompt may carry.
///
/// World books are injected directly now, which makes "how much" the runtime's
/// problem: a book that matches two hundred entries cannot all go into every
/// prompt, and a runtime that either sends everything or sends nothing is not
/// making a decision. These are client-side limits on what is pushed into a
/// request — not a claim about model attention.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorldBookBudget {
    /// How many entries may be injected in one request.
    pub max_entries: usize,
    /// How many characters of entry text may be injected in one request.
    pub max_chars: usize,
}

impl Default for WorldBookBudget {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_WORLD_BOOK_ENTRIES,
            max_chars: DEFAULT_MAX_WORLD_BOOK_CHARS,
        }
    }
}

pub const DEFAULT_MAX_WORLD_BOOK_ENTRIES: usize = 24;
pub const DEFAULT_MAX_WORLD_BOOK_CHARS: usize = 6_000;

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

        // A changed prompt block is a break, whatever shape the change has. It
        // used to be measured as "fewer shared messages than the previous block
        // was long", which cannot see a block that merely grew: everything the
        // previous request sent is still a prefix of this one, and the block is
        // still different. Comparing the blocks is the direct question.
        if self.prefix != previous.prefix {
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

        // One block ran out of segments before the other, so the difference is
        // at the edge and the extra segments are what changed. A world-book
        // entry that appeared at the end of the block is still a world-book
        // break, and looking only at the zipped pairs would call it a prompt
        // block break instead.
        self.prefix
            .iter()
            .skip(previous.prefix.len())
            .chain(previous.prefix.iter().skip(self.prefix.len()))
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

/// Everything one turn's prompt is built from.
///
/// This is the boundary the prompt pipeline is assembled at, and it is a
/// *context*, not a model: the caller has already resolved which persona,
/// characters, world books and prompt profile the conversation uses, so the
/// compiler never has to decide what belongs in the prompt.
///
/// Which matters because of who does the deciding. Everything here is either
/// injected directly or filtered programmatically — a persona, a character
/// card and the world's lore must not cost a tool call and an extra model round
/// to obtain. Tool calls stay for what genuinely has to be fetched on demand:
/// long history, large memory, external documents, anything whose relevance
/// cannot be known without reading it.
#[derive(Clone, Debug)]
pub struct PromptContext<'a> {
    /// The app-level prompt. A prompt profile's own system prompt replaces it.
    pub app_system_prompt: Option<&'a str>,
    pub prompt_profile: Option<&'a PromptProfile>,
    /// The persona the user brings to this conversation.
    pub persona: Option<&'a Persona>,
    /// Participants in conversation order. Zero is valid, one is ordinary, and
    /// several is a group or a table.
    pub participants: Vec<crate::Participant>,
    /// The character cards those participants refer to, in the same order.
    ///
    /// Kept beside `participants` rather than joined so a participant naming a
    /// card that is not installed is dropped instead of becoming an empty one.
    pub characters: Vec<&'a Character>,
    /// Every world book bound to this conversation, already resolved.
    pub world_books: Vec<&'a WorldBook>,
    /// How much of the world books may go into this request. Defaults to
    /// [`WorldBookBudget::default`]; a caller that knows its books are small,
    /// or that has a context window to fill, sets it explicitly.
    pub world_book_budget: WorldBookBudget,
    /// The transcript of everything already said, passed through unchanged.
    pub history: &'a Transcript,
    pub user_input: &'a str,
}

impl<'a> PromptContext<'a> {
    /// A context for the common single-character case.
    ///
    /// Exists so callers that genuinely have one character do not have to build
    /// a `Library` to say so. It is not the shape the model is compiled for —
    /// see [`PromptContext`] — it is a shortcut into it.
    pub fn single(
        character: &'a Character,
        world_books: &'a [WorldBook],
        history: &'a Transcript,
        user_input: &'a str,
    ) -> Self {
        Self {
            app_system_prompt: None,
            prompt_profile: None,
            persona: None,
            participants: vec![crate::Participant::character(character.id.clone())],
            characters: vec![character],
            world_books: world_books.iter().collect(),
            world_book_budget: WorldBookBudget::default(),
            history,
            user_input,
        }
    }

    /// Set the world-book budget for this request.
    pub fn with_world_book_budget(mut self, budget: WorldBookBudget) -> Self {
        self.world_book_budget = budget;
        self
    }

    /// The name a participant is presented under.
    fn participant_name<'p>(
        participant: &'p crate::Participant,
        character: &'p Character,
    ) -> Option<&'p str> {
        participant
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .or_else(|| {
                let name = character.name.trim();
                (!name.is_empty()).then_some(name)
            })
    }
}

pub struct PromptCompiler;

impl PromptCompiler {
    /// Compile one request.
    ///
    /// The transcript goes through exactly as stored: every assistant step, tool
    /// call and tool result the model saw last time is still here, so the
    /// request this returns shares a prefix with the previous one and the
    /// provider can reuse its cache.
    pub fn compile(context: &PromptContext<'_>) -> PromptPlan {
        let scan_text = format!("{}\n{}", context.history.scan_text(), context.user_input);

        // World-book selection happens here, in the runtime, by keyword and
        // position. The model is never asked whether a lore entry applies, and
        // never has to call a tool to find one: it is told, and what it is told
        // is the entries the scan actually hit.
        let mut active_entries = active_entries(context.world_books.iter().copied(), &scan_text);
        active_entries.sort_by_key(|entry| entry.priority);

        // ... and then the budget decides how much of what matched may actually
        // be sent. Entries that matched but did not fit are reported on the plan
        // rather than quietly left out: they are still in the book, still
        // projected into the context store, and one search away from this turn.
        let (active_entries, dropped_world_book_entries) =
            fit_world_book_budget(&active_entries, context.world_book_budget);

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

        // A bound profile's system prompt replaces the app one rather than
        // stacking under it: two system prompts saying different things about
        // the same job is a worse prompt than either alone.
        match context
            .prompt_profile
            .map(|profile| profile.system_prompt.as_str())
            .filter(|prompt| !prompt.trim().is_empty())
        {
            Some(profile_prompt) => push_non_empty(
                &mut prefix,
                ChatRole::System,
                profile_prompt,
                PromptSource::ProfileSystem,
                100,
            ),
            None => push_non_empty(
                &mut prefix,
                ChatRole::System,
                context.app_system_prompt.unwrap_or_default(),
                PromptSource::AppSystem,
                100,
            ),
        }

        push_world_book(
            &mut prefix,
            &active_entries,
            WorldBookPosition::BeforeCharacter,
        );

        // A group gets every participant's card, not just one. A narrator is
        // labelled as such, because a table's game master and an ordinary
        // character are not the same kind of participant and the model has to be
        // able to tell.
        for (participant, character) in context.participants.iter().zip(context.characters.iter()) {
            push_non_empty(
                &mut prefix,
                ChatRole::System,
                &character.system_prompt,
                PromptSource::CharacterSystem,
                95,
            );

            let definition = character_definition(
                PromptContext::participant_name(participant, character),
                participant,
                character,
            );
            push_non_empty(
                &mut prefix,
                ChatRole::System,
                &definition,
                PromptSource::CharacterDefinition,
                90,
            );
        }

        // A persona is two things said by two different parties, and they are
        // sent as two messages. Who the user is belongs to the system's account
        // of the conversation; what the user asks of the model is the user
        // speaking, and a system message that speaks for the user is a message
        // that misattributes it.
        if let Some(persona) = context.persona {
            push_non_empty(
                &mut prefix,
                ChatRole::System,
                &persona.system_text(),
                PromptSource::Persona,
                92,
            );
            push_non_empty(
                &mut prefix,
                ChatRole::User,
                persona.user_text(),
                PromptSource::PersonaUser,
                91,
            );
        }

        push_world_book(
            &mut prefix,
            &active_entries,
            WorldBookPosition::AfterCharacter,
        );

        for character in &context.characters {
            push_non_empty(
                &mut prefix,
                ChatRole::System,
                &character.example_dialogue,
                PromptSource::ExampleDialogue,
                40,
            );
        }

        push_non_empty(
            &mut prefix,
            ChatRole::System,
            &context
                .prompt_profile
                .map(|profile| profile.format_rules.as_str())
                .unwrap_or_default(),
            PromptSource::FormatRules,
            80,
        );

        // The profile's own user-side instruction, as a user message. Kept out of
        // the system prompt on purpose: it is what the user asked for, and the
        // system prompt is not the user.
        push_non_empty(
            &mut prefix,
            ChatRole::User,
            &context
                .prompt_profile
                .map(|profile| profile.user_prompt.as_str())
                .unwrap_or_default(),
            PromptSource::ProfileUser,
            78,
        );

        // Extra segments keep the role and position they declare, in the order
        // the profile lists them. Anything that belongs after the transcript is
        // pushed there instead, so "where does this go" is answered by the
        // profile rather than by a convention nobody can see.
        for profile in context.prompt_profile.into_iter() {
            for segment in &profile.segments {
                if segment.position == PromptPosition::Prefix {
                    push_non_empty(
                        &mut prefix,
                        segment.role,
                        &segment.content,
                        PromptSource::ProfileSegment,
                        75,
                    );
                }
            }
        }

        // If the session was compacted, the summary leads the history. The turns
        // it stands in for stay retrievable, but the model is told they are a
        // summary rather than being handed a gap it never saw close.
        let mut history_messages = Vec::new();
        if let Some(compacted) = context.history.compacted.as_ref() {
            if !compacted.summary.trim().is_empty() {
                history_messages.push(ModelMessage::user(format!(
                    "Summary of the earlier conversation:\n{}",
                    compacted.summary.trim()
                )));
            }
        }
        history_messages.extend(crate::transcript::model_messages_from_transcript(
            context.history,
        ));
        let history = history_messages;

        let mut suffix = Vec::new();

        // Near-history entries and the post-history instruction stay after the
        // history, where they still read as "just before the user's turn".
        // Neither is replayed from the transcript, and neither has to be: both
        // are re-derived from the conversation's bindings every turn, so they
        // carry conversation state only as long as that data is unchanged.
        // Anything the model must remember lives in the transcript.
        //
        // This is also why a request is not always append-only: last turn's copy
        // of this tail moves down behind the assistant's answer. The prompt block
        // and the transcript are untouched, so the provider keeps all of them
        // and re-reads only the tail.
        push_world_book(&mut suffix, &active_entries, WorldBookPosition::NearHistory);

        let mut post_history = String::new();
        for character in &context.characters {
            if !character.post_history_instructions.trim().is_empty() {
                post_history.push_str(character.post_history_instructions.trim());
                post_history.push('\n');
            }
        }
        if let Some(profile) = context.prompt_profile {
            if !profile.post_history_instructions.trim().is_empty() {
                post_history.push_str(profile.post_history_instructions.trim());
                post_history.push('\n');
            }
        }

        push_non_empty(
            &mut suffix,
            ChatRole::System,
            &post_history,
            PromptSource::PostHistoryInstruction,
            98,
        );

        for profile in context.prompt_profile.into_iter() {
            for segment in &profile.segments {
                if segment.position == PromptPosition::PostHistory {
                    push_non_empty(
                        &mut suffix,
                        segment.role,
                        &segment.content,
                        PromptSource::ProfileSegment,
                        97,
                    );
                }
            }
        }

        push_non_empty(
            &mut suffix,
            ChatRole::User,
            context.user_input,
            PromptSource::CurrentUserInput,
            1000,
        );

        PromptPlan {
            prefix,
            history,
            suffix,
            dropped_world_book_entries,
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

/// Decide what may be sent, and report what was left behind.
///
/// Constant entries go first: the book's author marked them as always-on, and an
/// instruction that is conditional on a keyword scan is not what they asked for.
/// They are still bounded — a book whose constant entries alone exceed the
/// budget is a book asking for more than a prompt can carry, and the plan says
/// so through `constant: true` rather than quietly sending less than promised.
///
/// Keyed hits then compete by priority, lowest number first, which is the same
/// order the prompt already used. An entry that is too long for what remains of
/// the character budget is skipped rather than truncated: half a lore entry is
/// a fact the model cannot use, and a partial one costs the same as a whole one.
///
/// The kept entries are returned in their original priority order regardless of
/// which pass chose them. Choosing *what* to send must not silently relocate the
/// entries that were always going to be sent: an entry's position in the prompt
/// is part of its meaning, and a cache break caused by a budget is already
/// declared as one.
fn fit_world_book_budget<'a>(
    entries: &[&'a WorldBookEntry],
    budget: WorldBookBudget,
) -> (Vec<&'a WorldBookEntry>, Vec<DroppedWorldBookEntry>) {
    let mut chosen = vec![false; entries.len()];
    let mut dropped: Vec<DroppedWorldBookEntry> = Vec::new();
    let mut count = 0usize;
    let mut chars = 0usize;

    for constant in [true, false] {
        for (index, entry) in entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.constant == constant)
        {
            let length = entry.content.chars().count();
            let over_entries = count >= budget.max_entries;
            let over_chars = chars.saturating_add(length) > budget.max_chars;
            if over_entries || over_chars {
                dropped.push(DroppedWorldBookEntry {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    constant,
                    reason: if over_entries {
                        WorldBookDropReason::EntryLimit
                    } else {
                        WorldBookDropReason::CharLimit
                    },
                });
                continue;
            }
            chosen[index] = true;
            count += 1;
            chars += length;
        }
    }

    let kept = entries
        .iter()
        .zip(chosen)
        .filter(|(_, keep)| *keep)
        .map(|(entry, _)| *entry)
        .collect();

    (kept, dropped)
}

/// The entries of every bound world book that apply to what has been said.
///
/// Bound first, then filtered. The filter is the runtime's decision — keyword,
/// enabled, constant — not the model's, because the alternative is paying a
/// model round trip and a tool call to learn which lore the prompt was always
/// going to contain.
fn active_entries<'a>(
    books: impl Iterator<Item = &'a WorldBook>,
    scan_text: &str,
) -> Vec<&'a WorldBookEntry> {
    books
        .flat_map(|book| book.entries.iter())
        .filter(|entry| entry.is_active_for(scan_text))
        .collect()
}

/// One participant's card, as the model reads it.
///
/// `name` is the participant's name rather than the card's, because a table
/// gives an actor a title the card never had, and the prompt has to use the one
/// the conversation is using.
fn character_definition(
    name: Option<&str>,
    participant: &crate::Participant,
    character: &Character,
) -> String {
    let mut lines = Vec::new();

    if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
        lines.push(format!("Character: {}", name.trim()));
    }
    if participant.role != crate::ParticipantRole::Character {
        lines.push(format!(
            "Role: {}",
            match participant.role {
                crate::ParticipantRole::Narrator => "narrator or game master",
                crate::ParticipantRole::Character => "character",
            }
        ));
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
    use crate::{
        AssistantStep, Conversation, Library, Participant, ToolResultRecord, Turn, WorldBook,
        WorldBookEntry,
    };
    use serde_json::json;

    fn lore_book() -> WorldBook {
        WorldBook {
            id: "world".into(),
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
            ..WorldBook::default()
        }
    }

    fn lore_character() -> Character {
        Character {
            id: "character-aerin".into(),
            name: "Aerin".into(),
            worldbook_ids: vec!["world".into()],
            ..Character::default()
        }
    }

    /// The ordinary shape: one character, the world book it defaults to, no
    /// persona and no profile. Built through `Library` so the tests exercise the
    /// same resolution the runtime uses, rather than a shortcut that could pass
    /// while the real binding path was broken.
    fn compile(
        library: &Library,
        conversation: &crate::Conversation,
        app_system_prompt: Option<&str>,
        user_input: &str,
    ) -> PromptPlan {
        let context = library.prompt_context(conversation, app_system_prompt, user_input);
        PromptCompiler::compile(&context)
    }

    fn solo_library() -> (Library, crate::Conversation) {
        let library = Library {
            characters: vec![lore_character()],
            world_books: vec![lore_book()],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![Participant::character("character-aerin")],
            ..Conversation::new("conversation-1")
        };
        (library, conversation)
    }

    #[test]
    fn triggered_world_book_entry_enters_prompt() {
        let (library, conversation) = solo_library();
        let plan = compile(&library, &conversation, None, "Take me to the Black Tower.");

        assert!(plan.segments().iter().any(|segment| {
            segment.source == PromptSource::WorldBook
                && segment.content.contains("north of the capital")
        }));
    }

    /// A conversation with nobody in it still produces a request.
    ///
    /// "How many characters does a chat need?" is a question the old model could
    /// only answer with "one", because a session had a `character_id`. Zero is a
    /// state the user reaches by starting a chat before choosing anybody, and it
    /// has to assemble.
    #[test]
    fn a_conversation_with_no_participants_still_compiles() {
        let library = Library::default();
        let conversation = Conversation::new("conversation-empty");

        let plan = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            "hello",
        );

        assert!(plan
            .prefix
            .iter()
            .any(|segment| segment.content == "stable system prompt"));
        assert_eq!(
            plan.model_messages().last(),
            Some(&ModelMessage::user("hello"))
        );
    }

    /// Two characters in one conversation means both cards reach the model.
    /// Group chat and tabletop are not a special mode; they are the same shape
    /// with two entries in a list.
    #[test]
    fn every_participant_reaches_the_prompt() {
        let library = Library {
            characters: vec![
                Character {
                    id: "gm".into(),
                    name: "Warden Ilsa".into(),
                    description: "Runs the table.".into(),
                    ..Character::default()
                },
                Character {
                    id: "npc".into(),
                    name: "Bram".into(),
                    description: "A smuggler.".into(),
                    ..Character::default()
                },
            ],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![Participant::narrator("gm"), Participant::character("npc")],
            ..Conversation::new("conversation-table")
        };

        let plan = compile(&library, &conversation, None, "roll initiative");

        assert!(plan.prefix.iter().any(|segment| {
            segment.source == PromptSource::CharacterDefinition
                && segment.content.contains("Runs the table.")
        }));
        assert!(plan.prefix.iter().any(|segment| {
            segment.source == PromptSource::CharacterDefinition
                && segment.content.contains("A smuggler.")
        }));
        // The narrator is labelled, because a game master and an ordinary
        // character are not the same kind of participant and the model has to be
        // able to tell them apart.
        assert!(plan.prefix.iter().any(|segment| {
            segment.source == PromptSource::CharacterDefinition
                && segment.content.contains("narrator or game master")
        }));
    }

    /// A participant naming a card that is not installed is skipped, not turned
    /// into an empty character. A missing card is a state the user can fix; a
    /// nameless participant in the prompt is a bug they cannot.
    #[test]
    fn a_participant_whose_card_is_missing_is_skipped() {
        let library = Library {
            characters: vec![lore_character()],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![
                Participant::character("character-aerin"),
                Participant::character("character-absent"),
            ],
            ..Conversation::new("conversation-1")
        };

        let context = library.prompt_context(&conversation, None, "hello");

        assert_eq!(context.characters.len(), 1);
        assert_eq!(context.participants.len(), 1);
    }

    /// The persona is injected by the runtime, every turn, without the model
    /// having to decide it wants to know who it is talking to.
    #[test]
    fn the_persona_is_injected_directly() {
        let library = Library {
            characters: vec![lore_character()],
            personas: vec![Persona {
                name: "Wren".into(),
                description: "A cartographer who has never seen the sea.".into(),
                ..Persona::new("persona-wren", "Wren")
            }],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![Participant::character("character-aerin")],
            persona_id: Some("persona-wren".into()),
            ..Conversation::new("conversation-1")
        };

        let plan = compile(&library, &conversation, None, "hello");

        let persona = plan
            .prefix
            .iter()
            .find(|segment| segment.source == PromptSource::Persona)
            .expect("the persona belongs in the prompt block");

        assert!(persona.content.contains("Wren"));
        assert!(persona.content.contains("never seen the sea"));
    }

    /// A bound prompt profile replaces the app prompt instead of stacking under
    /// it. Two system prompts disagreeing about the same job is a worse prompt
    /// than either one alone.
    #[test]
    fn a_bound_profile_replaces_the_app_system_prompt() {
        let library = Library {
            characters: vec![lore_character()],
            prompt_profiles: vec![PromptProfile {
                system_prompt: "You narrate a cold northern port.".into(),
                format_rules: "Answer in at most three sentences.".into(),
                ..PromptProfile::new("profile-cold", "Cold port")
            }],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![Participant::character("character-aerin")],
            prompt_profile_id: Some("profile-cold".into()),
            ..Conversation::new("conversation-1")
        };

        let plan = compile(&library, &conversation, Some("the app prompt"), "hello");

        let system = plan.prefix[0].clone();
        assert_eq!(system.source, PromptSource::ProfileSystem);
        assert_eq!(system.content, "You narrate a cold northern port.");
        assert!(!plan
            .segments()
            .iter()
            .any(|segment| segment.content == "the app prompt"));
        assert!(plan
            .prefix
            .iter()
            .any(|segment| segment.source == PromptSource::FormatRules
                && segment.content.contains("three sentences")));
    }

    /// Two world books bound at once both reach the prompt. A world book used to
    /// be a character field, so "this conversation uses two books" was not
    /// expressible at all.
    #[test]
    fn several_bound_world_books_all_reach_the_prompt() {
        let second = WorldBook {
            id: "harbors".into(),
            name: "Harbors".into(),
            entries: vec![WorldBookEntry {
                id: "salt-harbor".into(),
                name: "Salt Harbor".into(),
                content: "Salt Harbor trades in lamp oil.".into(),
                keys: vec!["Salt Harbor".into()],
                enabled: true,
                constant: false,
                priority: 20,
                position: WorldBookPosition::AfterCharacter,
                extensions: Default::default(),
            }],
            ..WorldBook::default()
        };
        let library = Library {
            world_books: vec![lore_book(), second],
            characters: vec![lore_character()],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![Participant::character("character-aerin")],
            worldbook_ids: vec!["harbors".into()],
            ..Conversation::new("conversation-1")
        };

        let plan = compile(
            &library,
            &conversation,
            None,
            "Take me past the Black Tower to Salt Harbor.",
        );

        assert!(plan.segments().iter().any(|segment| {
            segment.source == PromptSource::WorldBook
                && segment.content.contains("north of the capital")
        }));
        assert!(plan.segments().iter().any(|segment| {
            segment.source == PromptSource::WorldBook
                && segment.content.contains("trades in lamp oil")
        }));
    }

    /// The compacted summary is part of what the model can see, so it has to be
    /// part of what the world book is scanned against. Otherwise a long
    /// conversation silently changes which lore entries trigger the moment it
    /// gets compacted, and the summary is the only place the old detail lives.
    #[test]
    fn a_compacted_summary_keeps_triggering_lore_that_only_it_still_mentions() {
        let (library, mut conversation) = solo_library();

        // Only the OLD turn mentions the tower. If the retained turn mentioned
        // it too, the keyword would still match the live history and the test
        // would pass whether or not the summary is scanned.
        let mut old = Turn::new("turn-0", "Where do we go from here?");
        old.steps.push(AssistantStep {
            text: Some("We should visit the Black Tower before dawn.".into()),
            ..AssistantStep::default()
        });
        conversation.transcript.push(old);

        let mut kept = Turn::new("turn-1", "And then?");
        kept.steps.push(AssistantStep {
            text: Some("The road is long.".into()),
            ..AssistantStep::default()
        });
        conversation.transcript.push(kept);

        conversation.transcript.compact(1, |_, _| {
            "Earlier the pair agreed to visit the Black Tower before dawn.".to_string()
        });

        // The only turn that mentioned the tower is now only in the summary.
        assert_eq!(conversation.transcript.turns.len(), 1);
        assert!(!conversation.transcript.turns[0].steps[0]
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("Black Tower"));
        assert!(conversation.scan_text().contains("Black Tower"));

        let plan = compile(&library, &conversation, None, "Carry on.");

        assert!(plan.segments().iter().any(|segment| {
            segment.source == PromptSource::WorldBook
                && segment.content.contains("north of the capital")
        }));
    }

    #[test]
    fn unrelated_world_book_entry_stays_out() {
        let (library, conversation) = solo_library();
        let plan = compile(&library, &conversation, None, "We stay in the forest.");

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

        let conversation = crate::Conversation {
            transcript: {
                let mut transcript = Transcript::default();
                transcript.push(turn);
                transcript
            },
            ..Conversation::new("conversation-1")
        };

        let library = Library::default();
        let plan = compile(&library, &conversation, None, "and who called?");

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

    /// A conversation with a keyed world-book entry, a constant near-history
    /// entry and a post-history instruction: everything that can vary per turn.
    fn turning_conversation() -> (Library, crate::Conversation) {
        let (mut library, conversation) = solo_library();

        library.characters = vec![Character {
            system_prompt: "You are Aerin.".into(),
            description: "A tower keeper.".into(),
            example_dialogue: "USER: hello\nAERIN: welcome".into(),
            post_history_instructions: "Never speak for the user.".into(),
            ..lore_character()
        }];

        library.world_books[0].entries.push(WorldBookEntry {
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

        (library, conversation)
    }

    /// A conversation with nothing that varies per turn: no near-history entry
    /// and no post-history instruction, so the only thing a next turn adds to a
    /// request is the answer and the new input.
    fn steady_conversation() -> (Library, crate::Conversation) {
        let (mut library, mut conversation) = turning_conversation();
        library.characters[0].post_history_instructions = String::new();
        library.world_books[0].entries[1].enabled = false;
        conversation.id = "conversation-steady".into();
        (library, conversation)
    }

    fn index_of(plan: &PromptPlan, needle: &str) -> usize {
        plan.prefix
            .iter()
            .position(|segment| segment.content.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} must be in the prompt block: {:?}", plan.prefix))
    }

    #[test]
    fn a_world_book_entry_keeps_the_position_its_meaning_asks_for() {
        let (mut library, conversation) = steady_conversation();
        library.world_books[0].entries.push(WorldBookEntry {
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

        let plan = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
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
        let (library, conversation) = turning_conversation();
        let quiet = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            "we stay in the forest",
        );
        let lore = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
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
        let (library, conversation) = turning_conversation();
        let lore = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            "take me to the Black Tower",
        );
        let quiet = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            "we stay in the forest",
        );

        assert_eq!(
            quiet.cache_continuity_with(&lore),
            CacheContinuity::BrokeAtWorldBook
        );
    }

    #[test]
    fn near_history_and_post_history_content_never_enter_the_prompt_block() {
        let (library, conversation) = turning_conversation();
        let plan = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
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
        let (mut library, conversation) = turning_conversation();
        let before = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            "hello",
        );

        library.characters[0].system_prompt = "You are Aerin the second.".into();
        let after = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
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
        let (library, mut conversation) = turning_conversation();

        let first = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            "who keeps the tower?",
        );

        let mut turn = Turn::new("turn-1", "who keeps the tower?");
        turn.steps.push(AssistantStep::text_only("Aerin does."));
        conversation.transcript.push(turn);

        let second = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
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
        let (library, mut conversation) = steady_conversation();
        let first_input = "who keeps the tower?";

        let first = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            first_input,
        );

        let mut turn = Turn::new("turn-1", first_input);
        turn.steps.push(AssistantStep::text_only("Aerin does."));
        conversation.transcript.push(turn);

        let second = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
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
        let (library, mut conversation) = steady_conversation();
        for index in 0..3 {
            let mut turn = Turn::new(&format!("turn-{index}"), &format!("question {index}"));
            turn.steps
                .push(AssistantStep::text_only(&format!("answer {index}")));
            conversation.transcript.push(turn);
        }

        // Before compaction the conversation still carries all three turns.
        let mut uncompacted = conversation.clone();
        uncompacted
            .transcript
            .turns
            .retain(|turn| turn.id == "turn-2");
        let before = compile(
            &library,
            &uncompacted,
            Some("stable system prompt"),
            "and now?",
        );

        assert!(conversation
            .transcript
            .compact(1, |turns, _| format!("{} earlier turns", turns.len())));

        let after = compile(
            &library,
            &conversation,
            Some("stable system prompt"),
            "and now?",
        );

        assert_eq!(after.prefix, before.prefix);
        assert_eq!(
            after.cache_continuity_with(&before),
            CacheContinuity::BrokeAtHistory
        );
    }

    #[test]
    fn the_prompt_block_is_identical_across_turns_of_a_conversation() {
        let (library, conversation) = steady_conversation();
        let first = compile(&library, &conversation, Some("stable system prompt"), "one");
        let second = compile(&library, &conversation, Some("stable system prompt"), "two");

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

#[cfg(test)]
mod binding_tests {
    use super::*;
    use crate::{Conversation, Library, Participant, ParticipantRole, WorldBook};

    fn library() -> Library {
        Library {
            characters: vec![
                Character::new("card", "Aerin"),
                Character::new("other", "Bram"),
            ],
            world_books: vec![WorldBook::new("lore", "Lore")],
            personas: vec![Persona::new("persona", "Wren")],
            prompt_profiles: vec![PromptProfile::new("profile", "Profile")],
            global_worldbook_ids: vec!["lore".into()],
            conversations: Vec::new(),
        }
    }

    /// A world book bound globally and by the conversation is one book, not two.
    /// Injecting it twice would be visible to the model as duplicated lore.
    #[test]
    fn a_world_book_bound_twice_is_still_injected_once() {
        let library = library();
        let conversation = crate::Conversation {
            worldbook_ids: vec!["lore".into()],
            ..Conversation::new("conversation-1")
        };

        assert_eq!(library.world_books_of(&conversation).len(), 1);
    }

    /// A character's default world book applies to a conversation that did not
    /// name it, and a conversation's own binding applies to a conversation whose
    /// characters name nothing. Both directions, because both are the same
    /// resolution with a different origin.
    #[test]
    fn character_defaults_and_conversation_bindings_both_apply() {
        let mut library = library();
        library.characters[0].worldbook_ids = vec!["lore".into()];

        let by_default = crate::Conversation {
            participants: vec![Participant::character("card")],
            ..Conversation::new("conversation-1")
        };
        // The character names it, and the global binding agrees.
        assert_eq!(library.world_books_of(&by_default).len(), 1);

        let by_conversation = crate::Conversation {
            worldbook_ids: vec!["lore".into()],
            participants: vec![Participant::character("other")],
            ..Conversation::new("conversation-2")
        };
        // The conversation names it and the character does not.
        assert_eq!(library.world_books_of(&by_conversation).len(), 1);
    }

    /// Explicit bindings come first, so that when two books disagree the one the
    /// user chose for this conversation is the one the model reads earlier.
    #[test]
    fn an_explicit_binding_leads_the_world_books_of_a_conversation() {
        let mut library = library();
        library.world_books.push(WorldBook::new("extra", "Extra"));
        library.global_worldbook_ids = vec!["lore".into()];

        let conversation = crate::Conversation {
            worldbook_ids: vec!["extra".into()],
            ..Conversation::new("conversation-1")
        };

        let ids: Vec<&str> = library
            .world_books_of(&conversation)
            .iter()
            .map(|book| book.id.as_str())
            .collect();

        assert_eq!(ids, vec!["extra", "lore"]);
    }

    /// A participant role survives a round trip, because "this character is the
    /// game master" is stored data and not something a caller re-derives.
    #[test]
    fn a_participant_role_survives_serialization() {
        let participant = Participant {
            character_id: "gm".into(),
            role: ParticipantRole::Narrator,
            display_name: Some("Warden Ilsa".into()),
        };

        let restored: Participant =
            serde_json::from_str(&serde_json::to_string(&participant).unwrap()).unwrap();

        assert_eq!(restored, participant);
        assert_eq!(restored.role, ParticipantRole::Narrator);
    }

    /// A conversation with exactly one participant can be asked who it is. A
    /// conversation with two cannot, and answers `None` rather than picking
    /// one — there is no primary participant, and pretending otherwise is how
    /// group chat ends up being routed through a single character again.
    #[test]
    fn only_a_single_participant_conversation_has_a_sole_participant() {
        let solo = crate::Conversation {
            participants: vec![Participant::character("a")],
            ..Conversation::new("conversation-1")
        };
        let group = crate::Conversation {
            participants: vec![Participant::character("a"), Participant::narrator("b")],
            ..Conversation::new("conversation-2")
        };
        let empty = Conversation::new("conversation-3");

        assert_eq!(
            solo.sole_participant().map(|p| p.character_id.as_str()),
            Some("a")
        );
        assert!(group.sole_participant().is_none());
        assert!(empty.sole_participant().is_none());
    }

    /// An id generator that collides is not an id generator.
    #[test]
    fn a_new_conversation_id_cannot_collide() {
        let mut library = Library::default();
        library
            .conversations
            .push(Conversation::new("conversation-2"));

        let id = library.next_conversation_id();

        assert!(id != "conversation-2");
        assert!(!library.conversations.iter().any(|item| item.id == id));
    }
}

/// A fixed instruction has to be sent from the side of the conversation it
/// belongs to, or the model is being told a falsehood about who is speaking.
mod role_tests {
    use super::*;
    use crate::{Library, ModelRole, Persona, PromptProfile, PromptProfileSegment};
    use serde_json::json;

    /// A segment with only the two fields every segment has.
    fn empty_segment() -> PromptProfileSegment {
        PromptProfileSegment {
            id: String::new(),
            name: String::new(),
            content: String::new(),
            role: ChatRole::System,
            position: PromptPosition::Prefix,
            extensions: Default::default(),
        }
    }

    fn library_with_profile(
        profile: PromptProfile,
        persona: Option<Persona>,
    ) -> (Library, crate::Conversation) {
        let mut library = Library {
            characters: vec![Character {
                id: "character-aerin".into(),
                name: "Aerin".into(),
                ..Character::default()
            }],
            prompt_profiles: vec![profile],
            ..Library::default()
        };
        let mut conversation = crate::Conversation {
            participants: vec![crate::Participant::character("character-aerin")],
            prompt_profile_id: Some("profile".into()),
            ..crate::Conversation::new("conversation-1")
        };
        if let Some(persona) = persona {
            library.personas.push(persona.clone());
            conversation.persona_id = Some(persona.id.clone());
        }
        (library, conversation)
    }

    /// A field named `user_prompt` that arrives as system text is not a user
    /// prompt; it is a system prompt that claims to be the user. The message the
    /// provider receives is the thing that counts, so the assertion is on the
    /// request rather than on the segment.
    #[test]
    fn a_profile_user_prompt_is_sent_as_a_user_message() {
        let (library, conversation) = library_with_profile(
            PromptProfile {
                system_prompt: "You narrate a cold northern port.".into(),
                user_prompt: "Keep Aerin in character even when I push.".into(),
                ..PromptProfile::new("profile", "Port")
            },
            None,
        );

        let plan = PromptCompiler::compile(&library.prompt_context(&conversation, None, "hello"));

        let user_turn = plan
            .prefix
            .iter()
            .find(|segment| segment.source == PromptSource::ProfileUser)
            .expect("the user prompt is a segment of its own");
        assert_eq!(user_turn.role, ChatRole::User);
        assert!(!user_turn.content.contains("cold northern port"));

        let speaks_as_the_user = plan.model_messages().iter().any(|message| {
            matches!(
                message,
                ModelMessage::Text { role: ModelRole::User, content } if content.contains("even when I push")
            )
        });
        assert!(
            speaks_as_the_user,
            "the request has to carry the instruction from the user's side"
        );
    }

    /// A persona is two statements by two different parties: who the user is,
    /// which is the system's account, and what the user asks of the model, which
    /// is the user speaking. Merging them was what made the persona's user side
    /// unreadable.
    #[test]
    fn a_persona_speaks_from_both_sides_of_the_conversation() {
        let persona = Persona {
            description: "A cartographer who has never seen the sea.".into(),
            user_prompt: "I would rather be led than pushed.".into(),
            ..Persona::new("persona-wren", "Wren")
        };
        let (library, conversation) =
            library_with_profile(PromptProfile::new("profile", "Port"), Some(persona.clone()));

        let plan = PromptCompiler::compile(&library.prompt_context(&conversation, None, "hello"));

        let system_side = plan
            .prefix
            .iter()
            .find(|segment| segment.source == PromptSource::Persona)
            .expect("who the user is belongs in the system prompt");
        assert_eq!(system_side.role, ChatRole::System);
        assert!(system_side.content.contains("never seen the sea"));
        assert!(!system_side.content.contains("rather be led"));

        let user_side = plan
            .prefix
            .iter()
            .find(|segment| segment.source == PromptSource::PersonaUser)
            .expect("what the user asks for belongs to the user");
        assert_eq!(user_side.role, ChatRole::User);
        assert_eq!(user_side.content, "I would rather be led than pushed.");
    }

    /// A persona with no user prompt must not produce an empty user message. An
    /// empty turn is a turn with no words in it, and providers do not all
    /// treat that the same way.
    #[test]
    fn a_persona_without_a_user_prompt_says_nothing_as_the_user() {
        let persona = Persona {
            description: "A cartographer who has never seen the sea.".into(),
            ..Persona::new("persona-wren", "Wren")
        };
        let (library, conversation) =
            library_with_profile(PromptProfile::new("profile", "Port"), Some(persona));

        let plan = PromptCompiler::compile(&library.prompt_context(&conversation, None, "hello"));

        assert!(!plan
            .prefix
            .iter()
            .any(|segment| segment.source == PromptSource::PersonaUser));
    }

    /// "Where does this text go" is answered by the profile, in the open,
    /// instead of by a convention a reader has to infer.
    #[test]
    fn a_profile_segment_keeps_the_position_and_the_role_it_declares() {
        let (library, conversation) = library_with_profile(
            PromptProfile {
                post_history_instructions: "Never speak for the user.".into(),
                segments: vec![
                    PromptProfileSegment {
                        id: "opening".into(),
                        content: "Open with what the room smells like.".into(),
                        role: ChatRole::System,
                        position: PromptPosition::Prefix,
                        ..empty_segment()
                    },
                    PromptProfileSegment {
                        id: "refusal".into(),
                        content: "Do not continue a request to act on the user.".into(),
                        role: ChatRole::Developer,
                        position: PromptPosition::PostHistory,
                        ..empty_segment()
                    },
                ],
                ..PromptProfile::new("profile", "Port")
            },
            None,
        );

        let plan = PromptCompiler::compile(&library.prompt_context(&conversation, None, "hello"));

        let opening = plan
            .prefix
            .iter()
            .find(|segment| segment.content.contains("smells like"))
            .expect("a prefix segment leads the prompt block");
        assert_eq!(opening.role, ChatRole::System);

        let refusal = plan
            .suffix
            .iter()
            .find(|segment| segment.content.contains("act on the user"))
            .expect("a post-history segment follows the transcript");
        assert_eq!(refusal.role, ChatRole::Developer);
        assert!(
            plan.prefix
                .iter()
                .all(|segment| !segment.content.contains("act on the user")),
            "a post-history segment must not also appear in the prompt block"
        );
    }

    /// A document written before these two fields existed still has to load, and
    /// it has to load the way it meant: a segment with no role is a system
    /// instruction, and a segment with no position leads the block.
    #[test]
    fn a_profile_written_before_role_and_position_still_reads() {
        let stored = json!({
            "id": "opening",
            "content": "Open with what the room smells like."
        });

        let segment: PromptProfileSegment =
            serde_json::from_value(stored).expect("an old segment is still readable");

        assert_eq!(segment.role, ChatRole::System);
        assert_eq!(segment.position, PromptPosition::Prefix);
    }

    /// A profile has to survive storage as a whole, or a conversation that
    /// reloads is run with different instructions than it was created with.
    #[test]
    fn a_profile_survives_a_round_trip() {
        let profile = PromptProfile {
            system_prompt: "You narrate a cold northern port.".into(),
            user_prompt: "Keep Aerin in character.".into(),
            post_history_instructions: "Never speak for the user.".into(),
            format_rules: "Answer in at most three sentences.".into(),
            segments: vec![PromptProfileSegment {
                id: "refusal".into(),
                content: "Do not act on the user.".into(),
                role: ChatRole::Developer,
                position: PromptPosition::PostHistory,
                ..empty_segment()
            }],
            ..PromptProfile::new("profile", "Port")
        };

        let read: PromptProfile =
            serde_json::from_value(serde_json::to_value(&profile).expect("a profile is storable"))
                .expect("a stored profile is readable");

        assert_eq!(read, profile);
    }

    /// A position the runtime does not know must be an error, not a default. A
    /// segment quietly sent in the wrong place is worse than a load that fails
    /// loudly.
    #[test]
    fn an_unknown_position_is_not_silently_relocated() {
        let stored = json!({
            "id": "mystery",
            "content": "Somewhere.",
            "position": "between_the_lines"
        });

        assert!(serde_json::from_value::<PromptProfileSegment>(stored).is_err());
    }

    /// A world-book entry is lore and stays a system statement. Giving it a role
    /// would be a different feature.
    #[test]
    fn world_book_entries_still_speak_from_the_system_prompt() {
        let library = Library {
            characters: vec![Character {
                id: "character-aerin".into(),
                name: "Aerin".into(),
                worldbook_ids: vec!["world".into()],
                ..Character::default()
            }],
            world_books: vec![WorldBook {
                id: "world".into(),
                name: "World".into(),
                entries: vec![WorldBookEntry {
                    id: "black-tower".into(),
                    name: "Black Tower".into(),
                    content: "The Black Tower stands north of the capital.".into(),
                    keys: vec!["Black Tower".into()],
                    enabled: true,
                    constant: true,
                    priority: 0,
                    position: WorldBookPosition::AfterCharacter,
                    extensions: Default::default(),
                }],
                ..WorldBook::default()
            }],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![crate::Participant::character("character-aerin")],
            ..crate::Conversation::new("conversation-1")
        };

        let plan = PromptCompiler::compile(&library.prompt_context(&conversation, None, "hello"));

        let lore = plan
            .prefix
            .iter()
            .find(|segment| segment.source == PromptSource::WorldBook)
            .expect("a constant entry is active on every turn");
        assert_eq!(lore.role, ChatRole::System);
    }
}

/// How much of a world book one request may carry, and what it says when it
/// cannot carry all of it.
mod budget_tests {
    use super::*;
    use crate::{Library, WorldBookEntry};

    fn entry(id: &str, priority: i32, constant: bool, chars: usize) -> WorldBookEntry {
        WorldBookEntry {
            id: id.into(),
            name: id.into(),
            // The id leads the text so a test can say which entry was sent, and
            // the filler decides how much of the character budget it spends.
            content: format!("{id}:{}", "x".repeat(chars)),
            keys: if constant {
                Vec::new()
            } else {
                vec!["port".into()]
            },
            enabled: true,
            constant,
            priority,
            position: WorldBookPosition::AfterCharacter,
            extensions: Default::default(),
        }
    }

    fn library_with(entries: Vec<WorldBookEntry>) -> (Library, crate::Conversation) {
        let library = Library {
            characters: vec![Character {
                id: "character-aerin".into(),
                name: "Aerin".into(),
                worldbook_ids: vec!["world".into()],
                ..Character::default()
            }],
            world_books: vec![WorldBook {
                id: "world".into(),
                name: "World".into(),
                entries,
                ..WorldBook::default()
            }],
            ..Library::default()
        };
        let conversation = crate::Conversation {
            participants: vec![crate::Participant::character("character-aerin")],
            ..crate::Conversation::new("conversation-1")
        };
        (library, conversation)
    }

    fn plan_with_budget(
        library: &Library,
        conversation: &crate::Conversation,
        budget: WorldBookBudget,
    ) -> PromptPlan {
        let context = library
            .prompt_context(conversation, None, "take me to the port")
            .with_world_book_budget(budget);
        PromptCompiler::compile(&context)
    }

    /// The ids of the entries this request actually carried, in prompt order.
    fn sent_ids(plan: &PromptPlan) -> Vec<String> {
        plan.prefix
            .iter()
            .filter(|segment| segment.source == PromptSource::WorldBook)
            .filter_map(|segment| segment.content.split(':').next().map(String::from))
            .collect()
    }

    fn is_sent(plan: &PromptPlan, id: &str) -> bool {
        sent_ids(plan).iter().any(|sent| sent == id)
    }

    /// The character length of everything a request carried from the world book.
    fn sent_chars(plan: &PromptPlan) -> usize {
        plan.prefix
            .iter()
            .filter(|segment| segment.source == PromptSource::WorldBook)
            .map(|segment| segment.content.chars().count())
            .sum()
    }

    /// The runtime is now the thing that pushes lore into every request, so
    /// "how much" has to be a decision it makes. Without a cap, a book that
    /// matches two hundred entries ships two hundred entries every turn.
    #[test]
    fn too_many_matching_entries_leave_out_the_low_priority_ones() {
        let (library, conversation) = library_with(vec![
            entry("first", 10, false, 10),
            entry("second", 20, false, 10),
            entry("third", 30, false, 10),
        ]);

        let plan = plan_with_budget(
            &library,
            &conversation,
            WorldBookBudget {
                max_entries: 2,
                max_chars: 1_000,
            },
        );

        assert_eq!(sent_ids(&plan).len(), 2);
        assert!(is_sent(&plan, "first"));
        assert!(is_sent(&plan, "second"));
        assert!(!is_sent(&plan, "third"));
        assert_eq!(plan.dropped_world_book_entries.len(), 1);
        assert_eq!(plan.dropped_world_book_entries[0].id, "third");
        assert_eq!(
            plan.dropped_world_book_entries[0].reason,
            WorldBookDropReason::EntryLimit
        );
    }

    /// A character budget is spent by whichever entries have priority, and the
    /// plan has to say which limit did it: an entry cap and a character cap are
    /// different problems for whoever wrote the book.
    #[test]
    fn an_entry_too_long_for_what_is_left_is_skipped_rather_than_truncated() {
        let (library, conversation) = library_with(vec![
            entry("long", 10, false, 500),
            entry("short", 20, false, 10),
        ]);

        let plan = plan_with_budget(
            &library,
            &conversation,
            WorldBookBudget {
                max_entries: 10,
                max_chars: 100,
            },
        );

        assert!(!is_sent(&plan, "long"));
        assert!(is_sent(&plan, "short"));
        assert_eq!(plan.dropped_world_book_entries.len(), 1);
        assert_eq!(plan.dropped_world_book_entries[0].id, "long");
        assert_eq!(
            plan.dropped_world_book_entries[0].reason,
            WorldBookDropReason::CharLimit
        );
        // A partial entry is text the model cannot use, and it costs the same as
        // a whole one, so nothing that was sent is cut short.
        assert_eq!(sent_chars(&plan), "short:xxxxxxxxxx".chars().count());
    }

    /// A constant entry is not conditional on anything, so being left out
    /// because of a budget is a fact about the book and not a budget decision.
    /// The two are reported differently for that reason.
    #[test]
    fn a_constant_entry_outranks_a_keyed_one_however_low_its_priority() {
        let (library, conversation) = library_with(vec![
            entry("constant", 900, true, 10),
            entry("keyed", 1, false, 10),
        ]);

        let plan = plan_with_budget(
            &library,
            &conversation,
            WorldBookBudget {
                max_entries: 1,
                max_chars: 1_000,
            },
        );

        assert!(is_sent(&plan, "constant"));
        assert!(!is_sent(&plan, "keyed"));
        assert_eq!(plan.dropped_world_book_entries[0].id, "keyed");
        assert!(!plan.dropped_world_book_entries[0].constant);
    }

    /// A book whose constant entries alone exceed the budget cannot be
    /// honoured. That is worth saying out loud instead of sending less than the
    /// book promised.
    #[test]
    fn a_dropped_constant_entry_is_reported_as_one() {
        let (library, conversation) = library_with(vec![
            entry("always-one", 1, true, 10),
            entry("always-two", 2, true, 10),
        ]);

        let plan = plan_with_budget(
            &library,
            &conversation,
            WorldBookBudget {
                max_entries: 1,
                max_chars: 1_000,
            },
        );

        assert!(is_sent(&plan, "always-one"));
        assert_eq!(plan.dropped_world_book_entries.len(), 1);
        assert!(plan.dropped_world_book_entries[0].constant);
    }

    /// Deciding what to send must not move what was always going to be sent. An
    /// entry's position in the prompt is part of its meaning.
    #[test]
    fn the_budget_keeps_its_order_and_only_changes_membership() {
        let (library, conversation) = library_with(vec![
            entry("late-but-constant", 500, true, 10),
            entry("early-keyed", 1, false, 10),
            entry("later-keyed", 2, false, 10),
        ]);

        let plan = plan_with_budget(
            &library,
            &conversation,
            WorldBookBudget {
                max_entries: 2,
                max_chars: 1_000,
            },
        );

        assert_eq!(
            sent_ids(&plan),
            vec!["early-keyed".to_string(), "late-but-constant".to_string()]
        );
    }

    /// Nothing dropped is nothing to report, and a plan that always carries a
    /// list of names has a list that always has to be checked.
    #[test]
    fn an_ordinary_request_reports_nothing_dropped() {
        let (library, conversation) = library_with(vec![entry("only", 10, true, 10)]);

        let plan = plan_with_budget(&library, &conversation, WorldBookBudget::default());

        assert!(plan.dropped_world_book_entries.is_empty());
        assert!(
            serde_json::to_value(&plan)
                .expect("a plan is storable")
                .get("dropped_world_book_entries")
                .is_none(),
            "an empty list should not be written at all"
        );
    }

    /// A plan stored before the budget existed still has to load. The list is
    /// about what a request left behind, and a plan that cannot be read is worse
    /// than one that reports nothing dropped.
    #[test]
    fn a_plan_written_before_the_budget_still_reads() {
        let stored = serde_json::json!({
            "prefix": [],
            "history": [],
            "suffix": []
        });

        let plan: PromptPlan =
            serde_json::from_value(stored).expect("an old plan is still readable");

        assert!(plan.dropped_world_book_entries.is_empty());
    }

    /// Changing what fits changes the prompt block, so it has to be reported the
    /// same way any other world-book change is: a declared break, not a hidden
    /// one.
    #[test]
    fn a_budget_that_changed_between_two_turns_is_a_declared_break() {
        let (library, conversation) = library_with(vec![
            entry("first", 10, false, 10),
            entry("second", 20, false, 10),
        ]);

        let roomy = plan_with_budget(
            &library,
            &conversation,
            WorldBookBudget {
                max_entries: 5,
                max_chars: 1_000,
            },
        );
        let tight = plan_with_budget(
            &library,
            &conversation,
            WorldBookBudget {
                max_entries: 1,
                max_chars: 1_000,
            },
        );

        assert_eq!(
            roomy.cache_continuity_with(&tight),
            CacheContinuity::BrokeAtWorldBook
        );
    }

    /// The defaults have to be a real limit, not a number that only looks
    /// careful.
    #[test]
    fn the_default_budget_bounds_an_enormous_book() {
        let entries: Vec<WorldBookEntry> = (0..200)
            .map(|index| entry(&format!("entry-{index}"), index, false, 200))
            .collect();
        let (library, conversation) = library_with(entries);

        let plan = plan_with_budget(&library, &conversation, WorldBookBudget::default());

        assert!(sent_ids(&plan).len() <= DEFAULT_MAX_WORLD_BOOK_ENTRIES);
        assert!(!plan.dropped_world_book_entries.is_empty());
    }
}
