//! UniFFI binding layer: the app-facing contract Android and any other
//! non-JavaScript platform see.
//!
//! # Why this crate exists separately
//!
//! `sujiu-runtime` is the runtime: it owns conversations, prompt compilation,
//! providers, tools and storage, and it depends on nothing that knows a language
//! binding exists. That is deliberate. A binding generator is a build-time
//! dependency with a large tree and opinions about namespace and error shape,
//! and a runtime that depends on it cannot be used from a plain Rust binary
//! without carrying that along.
//!
//! So this crate is the only place where a UniFFI macro appears. It owns:
//!
//! - the exported object, which is a translation layer and holds no state the
//!   runtime does not already hold;
//! - the records, which are the app-facing domain model and are written once here
//!   rather than hand-copied into Kotlin;
//! - the error enum, which keeps the runtime's own error type a Rust detail.
//!
//! # What must never appear here
//!
//! Provider wire formats. An OpenAI request body, an Anthropic message block, an
//! HTTP status code or an internal store document is not app-facing data, and a
//! platform that learns any of them will eventually make a conversation decision
//! from one. What crosses instead is Sujiu's own model: conversations,
//! characters, personas, world books, prompt profiles, endpoint discovery,
//! normalized turn events, diagnostics.
//!
//! # Relationship to the HarmonyOS binding
//!
//! `sujiu-napi` exports the same runtime to N-API. Both crates translate; neither
//! owns the contract's meaning, and both are generated from the same Rust types,
//! so adding a field is a change in `sujiu-runtime` and nowhere else.

uniffi::setup_scaffolding!("sujiu");

mod error;
mod records;
mod runtime;

pub use error::UniError;
pub use records::*;
pub use runtime::{create, SujiuApp, TurnEventListener, TurnEventRecord};
