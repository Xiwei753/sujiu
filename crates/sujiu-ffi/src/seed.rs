//! Deterministic seed data for a fresh runtime.
//!
//! Sujiu has no persistence layer yet. Until one exists the runtime starts from
//! this catalog so every platform sees the same real domain values: characters
//! with a persona, sessions with transcripts, and context sources backed by
//! actual records.

use serde_json::{Map, Value};
use sujiu_core::{
    AssistantStep, Character, ContextKind, ContextRecord, ContextScope, ContextSource, Session,
    Transcript, Turn,
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
) -> Character {
    Character {
        schema_version: 1,
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
        world_book: None,
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

fn session(
    id: &str,
    character_id: &str,
    title: &str,
    updated_at_ms: i64,
    turns: Vec<Turn>,
) -> Session {
    Session {
        id: id.to_string(),
        character_id: Some(character_id.to_string()),
        transcript: Transcript {
            turns,
            compacted: None,
        },
        metadata: metadata(&[
            ("title", Value::from(title)),
            ("updatedAtMs", Value::from(updated_at_ms)),
        ]),
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

/// The catalog a new runtime starts from.
pub fn seed() -> Seed {
    let characters = vec![
        character(
            "character-lin",
            "Lin",
            "A night-shift radio operator who answers calls nobody else picks up.",
            "Dry, observant, tired but never careless.",
            "A small coastal radio station, 3am.",
        ),
        character(
            "character-wen",
            "Wen",
            "An archivist who remembers the order of every shelf in a library that is slowly flooding.",
            "Warm, precise, quietly funny.",
            "The lower archive of a municipal library.",
        ),
        character(
            "character-shen",
            "Shen",
            "A travelling engineer who repairs machines nobody else can reach.",
            "Blunt, patient, allergic to small talk.",
            "A mountain pass, halfway up.",
        ),
    ];

    let sessions = vec![
        session(
            "session-1",
            "character-lin",
            "The blinking light",
            NOW_MS,
            vec![turn(
                "turn-1",
                "You said the frequency was dead. Why is the light on the console blinking?",
                &["Because the console and I are arguing about who owns the night shift. The blinking is a caller who has not decided to speak yet."],
                NOW_MS - 60_000,
            )],
        ),
        session(
            "session-2",
            "character-wen",
            "Flood order",
            NOW_MS - 86_400_000,
            vec![turn(
                "turn-1",
                "Which shelves move first?",
                &[],
                NOW_MS - 86_400_000,
            )],
        ),
        session(
            "session-3",
            "character-shen",
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
            description: "Imported character card fields used for prompt compilation."
                .to_string()
                .to_string(),
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
            description: "Scene summaries extracted from earlier sessions.".to_string(),
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
            "An abandoned session. Shen was asked to inspect a stalled cable winch and never replied.",
            &["shen", "winch", "cable"],
            NOW_MS - 200_000_000,
        ),
    ];

    Seed {
        characters,
        sessions,
        sources,
        records,
    }
}
