//! Errors crossing the binding.
//!
//! The runtime's own error enum stays a Rust detail. What a platform needs from
//! a failure is a case it can switch on and a sentence it can show, and those two
//! are not always the same string, so they are separate fields here.
//!
//! The variants are deliberately not merged. "That conversation is gone" and
//! "the thing you were editing is gone" look identical if both are reported as a
//! missing id, and a platform cannot tell which screen it should return to.

use sujiu_ai::ContextStoreError;
use sujiu_runtime::runtime::TurnError;

/// One failure the app-facing API can report.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum UniError {
    #[error("conversation_not_found: {0}")]
    ConversationNotFound(String),

    #[error("entity_not_found: {0}")]
    EntityNotFound(String),

    #[error("no_provider_configured")]
    NoProviderConfigured,

    #[error("no_usable_protocol: {0}")]
    NoUsableProtocol(String),

    #[error("missing_credential")]
    MissingCredential,

    #[error("{0}")]
    ContextStore(String),

    #[error("storage: {0}")]
    Storage(String),
}

impl From<TurnError> for UniError {
    fn from(error: TurnError) -> Self {
        match error {
            TurnError::SessionNotFound(id) => Self::ConversationNotFound(id),
            TurnError::EntityNotFound(id) => Self::EntityNotFound(id),
            TurnError::NoProviderConfigured => Self::NoProviderConfigured,
            TurnError::NoUsableProtocol(reason) => Self::NoUsableProtocol(reason),
            TurnError::MissingCredential => Self::MissingCredential,
            TurnError::ContextStore(inner) => Self::ContextStore(inner.to_string()),
            TurnError::Storage(detail) => Self::Storage(detail),
        }
    }
}

impl From<ContextStoreError> for UniError {
    fn from(error: ContextStoreError) -> Self {
        Self::ContextStore(error.to_string())
    }
}
