pub mod character;
pub mod prompt;
pub mod provider;
pub mod session;
pub mod worldbook;

pub use character::Character;
pub use prompt::{PromptCompiler, PromptPlan, PromptSegment, PromptSource};
pub use provider::{ProviderConfig, ProviderKind};
pub use session::{ChatMessage, ChatRole, Session};
pub use worldbook::{WorldBook, WorldBookEntry, WorldBookPosition};

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
