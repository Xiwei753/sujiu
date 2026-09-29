pub mod character;
pub mod context;
pub mod prompt;
pub mod provider;
pub mod session;
pub mod worldbook;

pub use character::Character;
pub use context::{ContextKind, ContextRecord, ContextScope, ContextSource};
pub use prompt::{
    PromptCompiler, PromptPlan, PromptSegment, PromptSource, DEFAULT_APP_SYSTEM_PROMPT,
};
pub use provider::{ProviderConfig, ProviderKind};
pub use session::{ChatMessage, ChatRole, Session};
pub use worldbook::{WorldBook, WorldBookEntry, WorldBookPosition};

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
