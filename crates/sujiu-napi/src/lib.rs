//! HarmonyOS/Android napi binding for the shared Sujiu runtime.
//!
//! The generated module is imported from ArkTS as `libsujiu_napi.so`. It is
//! intentionally a thin translation layer: conversation semantics stay in the
//! Rust runtime, and the platform bridge above it never learns provider wire
//! formats.

#![deny(clippy::all)]

mod bridge;
#[cfg(target_env = "ohos")]
mod ohos_registration;

pub use bridge::{
    CharacterSummaryDto, ContextSourceDto, ConversationSnapshotDto, MessageDto, ModelSummaryDto,
    ProviderConfigDto, SendTurnTask, SessionSummaryDto, SujiuRuntimeBridge, ToolCallDto,
    TurnRequestDto,
};

#[napi_derive::napi]
pub fn core_version() -> String {
    sujiu_runtime::CORE_VERSION.to_string()
}
