use sujiu_core::Character;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("format is not supported yet: {0}")]
    Unsupported(&'static str),

    #[error("invalid external data: {0}")]
    Invalid(String),
}

pub trait CharacterCodec {
    fn decode(&self, input: &[u8]) -> Result<Character, CodecError>;
    fn encode(&self, character: &Character) -> Result<Vec<u8>, CodecError>;
}

/// Namespace reserved for SillyTavern / Character Card codecs.
///
/// External fields that Sujiu does not understand should be preserved through
/// Character::extensions instead of being silently discarded.
pub mod sillytavern {
    pub const CHARACTER_CARD_V1: &str = "character_card_v1";
    pub const CHARACTER_CARD_V2: &str = "character_card_v2";
    pub const WORLD_BOOK: &str = "sillytavern_world_book";
}
