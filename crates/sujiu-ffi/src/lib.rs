use std::ffi::{c_char, CStr, CString};

use serde::{Deserialize, Serialize};
use sujiu_core::{Character, ChatMessage, PromptCompiler, PromptPlan, CORE_VERSION};

#[derive(Debug, Deserialize)]
struct CompilePromptInput {
    #[serde(default)]
    app_system_prompt: Option<String>,
    character: Character,
    #[serde(default)]
    history: Vec<ChatMessage>,
    user_input: String,
}

#[derive(Debug, Serialize)]
struct ApiEnvelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

impl<T> ApiEnvelope<T> {
    fn ok(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

pub fn compile_prompt_json(input: &str) -> String {
    let parsed = match serde_json::from_str::<CompilePromptInput>(input) {
        Ok(parsed) => parsed,
        Err(error) => {
            return serde_json::to_string(&ApiEnvelope::<PromptPlan>::error(error.to_string()))
                .expect("error envelope is serializable");
        }
    };

    let plan = PromptCompiler::compile(
        parsed.app_system_prompt.as_deref(),
        &parsed.character,
        &parsed.history,
        &parsed.user_input,
    );

    serde_json::to_string(&ApiEnvelope::ok(plan)).expect("prompt plan is serializable")
}

#[no_mangle]
pub extern "C" fn sujiu_core_version() -> *mut c_char {
    into_c_string(CORE_VERSION.to_owned())
}

#[no_mangle]
pub unsafe extern "C" fn sujiu_compile_prompt_json(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return into_c_string(
            serde_json::to_string(&ApiEnvelope::<PromptPlan>::error("input pointer is null"))
                .expect("error envelope is serializable"),
        );
    }

    let input = match unsafe { CStr::from_ptr(input) }.to_str() {
        Ok(input) => input,
        Err(error) => {
            return into_c_string(
                serde_json::to_string(&ApiEnvelope::<PromptPlan>::error(error.to_string()))
                    .expect("error envelope is serializable"),
            );
        }
    };

    into_c_string(compile_prompt_json(input))
}

#[no_mangle]
pub unsafe extern "C" fn sujiu_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(unsafe { CString::from_raw(value) });
    }
}

fn into_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .expect("JSON and version strings must not contain interior NUL bytes")
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_json_returns_error_envelope() {
        let result = compile_prompt_json("{");
        assert!(result.contains("\"ok\":false"));
    }
}
