//! Deterministic seed data for a fresh runtime.
//!
//! A new runtime starts from this library so every platform sees the same real
//! domain values: characters, personas, world books and a prompt profile as
//! entities of their own, conversations that bind them, transcripts, and
//! context sources backed by actual records.
//!
//! The seed deliberately includes a conversation with more than one
//! participant, because "the chat has exactly one character in it" used to be
//! the only shape the data model could hold, and seed data that never shows the
//! other shapes is how that assumption comes back.

use serde_json::{Map, Value};
use sujiu_core::{
    AssistantStep, Character, ContextKind, ContextRecord, ContextScope, ContextSource,
    Conversation, Library, Participant, Persona, PromptProfile, Transcript, Turn, WorldBook,
    WorldBookEntry, WorldBookPosition,
};

use crate::runtime::Seed;

const NOW_MS: i64 = 1_760_000_000_000;

fn metadata(entries: &[(&str, Value)]) -> Map<String, Value> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn character(
    id: &str,
    name: &str,
    description: &str,
    personality: &str,
    scenario: &str,
    worldbook_ids: &[&str],
) -> Character {
    Character {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        personality: personality.to_string(),
        scenario: scenario.to_string(),
        first_message: String::new(),
        alternate_greetings: Vec::new(),
        example_dialogue: String::new(),
        system_prompt: format!("You are {name}. Stay in character."),
        post_history_instructions: String::new(),
        worldbook_ids: worldbook_ids.iter().map(|id| (*id).to_string()).collect(),
        extensions: Map::new(),
        ..Character::default()
    }
}

fn lore_entry(
    id: &str,
    name: &str,
    content: &str,
    keys: &[&str],
    constant: bool,
    priority: i32,
) -> WorldBookEntry {
    WorldBookEntry {
        id: id.to_string(),
        name: name.to_string(),
        content: content.to_string(),
        keys: keys.iter().map(|key| (*key).to_string()).collect(),
        constant,
        priority,
        enabled: true,
        position: WorldBookPosition::default(),
        extensions: Map::new(),
    }
}

/// One recorded turn: what was asked, and every assistant step that answered.
///
/// The seed records steps rather than a single answer, so seeded data has the
/// same shape a real tool-using turn produces.
fn turn(id: &str, user: &str, answers: &[&str], at_ms: i64) -> Turn {
    let mut turn = Turn::new(id, user);
    turn.created_at_ms = Some(at_ms);
    turn.steps = answers
        .iter()
        .map(|answer| AssistantStep::text_only(*answer))
        .collect();
    turn
}

fn conversation(
    id: &str,
    participants: Vec<Participant>,
    persona_id: Option<&str>,
    worldbook_ids: &[&str],
    prompt_profile_id: Option<&str>,
    title: &str,
    updated_at_ms: i64,
    turns: Vec<Turn>,
) -> Conversation {
    Conversation {
        id: id.to_string(),
        participants,
        persona_id: persona_id.map(str::to_owned),
        worldbook_ids: worldbook_ids.iter().map(|id| (*id).to_string()).collect(),
        prompt_profile_id: prompt_profile_id.map(str::to_owned),
        transcript: Transcript {
            turns,
            compacted: None,
        },
        metadata: metadata(&[
            ("title", Value::from(title)),
            ("updatedAtMs", Value::from(updated_at_ms)),
        ]),
        ..Conversation::default()
    }
}

fn record(
    uri: &str,
    source_id: &str,
    kind: ContextKind,
    title: &str,
    content: &str,
    keywords: &[&str],
    at_ms: i64,
) -> ContextRecord {
    ContextRecord {
        uri: uri.to_string(),
        source_id: source_id.to_string(),
        kind,
        title: title.to_string(),
        content: content.to_string(),
        keywords: keywords.iter().map(|value| (*value).to_string()).collect(),
        tags: Vec::new(),
        priority: 0,
        timestamp_ms: Some(at_ms),
        scope: ContextScope::default(),
        metadata: Map::new(),
    }
}

/// The library a new runtime starts from.
pub fn seed() -> Seed {
    let world_books = vec![WorldBook {
        id: "world-book-coast".to_string(),
        name: "The northern coast".to_string(),
        entries: vec![
            lore_entry(
                "coast-constant",
                "Tone",
                "The coast is cold, quiet and out of season. Weather is never small here.",
                &[],
                true,
                10,
            ),
            lore_entry(
                "coast-station",
                "The coastal station",
                "KRS-9 transmits from a concrete hut on the northern breakwater. Its console has one blinking status light that nobody has been able to replace since the winter storm.",
                &["station", "console", "radio", "breakwater"],
                false,
                50,
            ),
            lore_entry(
                "coast-archive",
                "The lower archive",
                "Water reaches the lowest shelves first. Wen salvages what she can and files everything else by hand.",
                &["archive", "library", "flood"],
                false,
                40,
            ),
        ],
        extensions: Map::new(),
        ..WorldBook::default()
    }];

    let personas = vec![Persona {
        id: "persona-insomniac".to_string(),
        name: "The insomniac".to_string(),
        description:
            "A night-shift listener who is always tired and never quite as bored as they sound."
                .to_string(),
        user_prompt: "Keep replies short. Do not summarise the scene back at me.".to_string(),
        ..Persona::default()
    }];

    let prompt_profiles = vec![PromptProfile {
        id: "profile-roleplay".to_string(),
        name: "Roleplay".to_string(),
        format_rules: "Write in third person for narration and first person for speech. Never speak for the user's character."
            .to_string(),
        post_history_instructions:
            "Stay in the moment. Do not step outside the scene to explain yourself.".to_string(),
        ..PromptProfile::default()
    }];

    let characters = vec![
        character(
            "character-lin",
            "Lin",
            "A night-shift radio operator who answers calls nobody else picks up.",
            "Dry, observant, tired but never careless.",
            "A small coastal radio station, 3am.",
            &["world-book-coast"],
        ),
        character(
            "character-wen",
            "Wen",
            "An archivist who remembers the order of every shelf in a library that is slowly flooding.",
            "Warm, precise, quietly funny.",
            "The lower archive of a municipal library.",
            &[],
        ),
        character(
            "character-shen",
            "Shen",
            "A travelling engineer who repairs machines nobody else can reach.",
            "Blunt, patient, allergic to small talk.",
            "A mountain pass, halfway up.",
            &[],
        ),
    ];

    let conversations = vec![
        conversation(
            "session-1",
            vec![Participant::character("character-lin")],
            Some("persona-insomniac"),
            &[],
            Some("profile-roleplay"),
            "The blinking light",
            NOW_MS,
            vec![turn(
                "turn-1",
                "You said the frequency was dead. Why is the light on the console blinking?",
                &["Because the console and I are arguing about who owns the night shift. The blinking is a caller who has not decided to speak yet."],
                NOW_MS - 60_000,
            )],
        ),
        conversation(
            "session-2",
            vec![Participant::character("character-wen")],
            None,
            &["world-book-coast"],
            None,
            "Flood order",
            NOW_MS - 86_400_000,
            vec![turn(
                "turn-1",
                "Which shelves move first?",
                &[],
                NOW_MS - 86_400_000,
            )],
        ),
        // Two characters and a narrator in one conversation: the shape a
        // group chat or a tabletop session needs, which the old single-character
        // session could not hold at all.
        conversation(
            "session-3",
            vec![
                Participant::narrator("character-shen"),
                Participant::character("character-lin"),
            ],
            None,
            &[],
            None,
            "Halfway up",
            NOW_MS - 200_000_000,
            Vec::new(),
        ),
    ];

    let sources = vec![
        ContextSource {
            id: "source-character".to_string(),
            kind: ContextKind::Persona,
            name: "Character settings".to_string(),
            description: "Imported character card fields used for prompt compilation.".to_string(),
            scope: ContextScope::default(),
            mutable: true,
            record_count: characters.len(),
            metadata: Map::new(),
        },
        ContextSource {
            id: "source-lore".to_string(),
            kind: ContextKind::WorldLore,
            name: "World book".to_string(),
            description: "World book entries triggered by keywords.".to_string(),
            scope: ContextScope::default(),
            mutable: true,
            record_count: 3,
            metadata: Map::new(),
        },
        ContextSource {
            id: "source-story".to_string(),
            kind: ContextKind::StoryEvent,
            name: "Plot memory".to_string(),
            description: "Scene summaries extracted from earlier conversations."
                .to_string()
                .to_string(),
            scope: ContextScope::default(),
            mutable: true,
            record_count: 2,
            metadata: Map::new(),
        },
        ContextSource {
            id: "source-history".to_string(),
            kind: ContextKind::ChatHistory,
            name: "Older chats".to_string(),
            description: "Compacted transcripts of past conversations.".to_string(),
            scope: ContextScope::default(),
            mutable: false,
            record_count: 2,
            metadata: Map::new(),
        },
    ];

    let records = vec![
        record(
            "sujiu://context/lore/station",
            "source-lore",
            ContextKind::WorldLore,
            "The coastal station",
            "KRS-9 transmits from a concrete hut on the northern breakwater. Its console has one blinking status light that nobody has been able to replace since the winter storm.",
            &["station", "console", "radio", "breakwater"],
            NOW_MS - 900_000,
        ),
        record(
            "sujiu://context/lore/nightshift",
            "source-lore",
            ContextKind::WorldLore,
            "The night shift",
            "The station runs unattended after midnight. Lin keeps the shift by choice and has never explained why.",
            &["night", "shift", "lin"],
            NOW_MS - 800_000,
        ),
        record(
            "sujiu://context/lore/archive",
            "source-lore",
            ContextKind::WorldLore,
            "The lower archive",
            "Water reaches the lowest shelves first. Wen salvages what she can and files everything else by hand.",
            &["archive", "library", "wen", "flood"],
            NOW_MS - 700_000,
        ),
        record(
            "sujiu://context/story/blinking-caller",
            "source-story",
            ContextKind::StoryEvent,
            "The caller who did not speak",
            "A caller held the line open for forty seconds and hung up without saying anything. Lin logged it as a false positive.",
            &["caller", "blinking", "false positive"],
            NOW_MS - 60_000,
        ),
        record(
            "sujiu://context/story/archive-rescue",
            "source-story",
            ContextKind::StoryEvent,
            "Moving the shelves",
            "Wen and two volunteers moved eleven boxes of municipal records out of the archive before the water reached them.",
            &["shelves", "archive", "records"],
            NOW_MS - 86_400_000,
        ),
        record(
            "sujiu://context/history/session-2",
            "source-history",
            ContextKind::ChatHistory,
            "Flood order",
            "User asked which shelves move first. Wen answered the lowest ones, then listed what she would save first.",
            &["shelves", "flood", "wen"],
            NOW_MS - 86_400_000,
        ),
        record(
            "sujiu://context/history/session-3",
            "source-history",
            ContextKind::ChatHistory,
            "Halfway up",
            "An abandoned conversation. Shen was asked to inspect a stalled cable winch and never replied.",
            &["shen", "winch", "cable"],
            NOW_MS - 200_000_000,
        ),
    ];

    Seed {
        library: Library {
            conversations,
            characters,
            personas,
            world_books,
            prompt_profiles,
            global_worldbook_ids: Vec::new(),
        },
        sources,
        records,
    }
}
