use async_trait::async_trait;
use thiserror::Error;

use crate::types::{AssistantTurn, ProviderRequest};

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("provider returned HTTP {status}: {body}")]
    Http { status: u16, body: String },

    #[error("provider response was invalid: {0}")]
    InvalidResponse(String),
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, request: ProviderRequest) -> Result<AssistantTurn, ProviderError>;
}
