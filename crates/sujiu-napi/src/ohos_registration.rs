//! OpenHarmony module registration.
//!
//! Node loads a native addon by looking for `napi_register_module_v1`. The
//! OpenHarmony engine does not: `libace_napi.z.so` exports `napi_module_register`
//! and expects a `napi_module` descriptor to be registered from a library
//! constructor. napi-rs only emits the Node entry point, so without this shim
//! the module loads and every export reads as undefined.
//!
//! The descriptor's register function forwards into napi-rs, so there is still
//! exactly one implementation of the module surface.

use std::os::raw::c_char;

use napi::sys::{napi_env, napi_module, napi_value};

/// The module name ArkTS imports the library by.
const MODULE_NAME: &[u8] = b"sujiu_napi\0";

unsafe extern "C" {
    fn napi_module_register(module: *mut napi_module);

    /// napi-rs's own entry point. It is `#[no_mangle]` but lives in a private
    /// module, so it is declared here rather than imported.
    fn napi_register_module_v1(env: napi_env, exports: napi_value) -> napi_value;
}

unsafe extern "C" fn register_with_napi_rs(env: napi_env, exports: napi_value) -> napi_value {
    napi_register_module_v1(env, exports)
}

static mut SUJIU_NAPI_MODULE: napi_module = napi_module {
    nm_version: 1,
    nm_flags: 0,
    nm_filename: std::ptr::null(),
    nm_register_func: Some(register_with_napi_rs),
    nm_modname: MODULE_NAME.as_ptr() as *const c_char,
    nm_priv: std::ptr::null_mut(),
    reserved: [std::ptr::null_mut(); 4],
};

#[cfg(target_env = "ohos")]
#[used]
#[unsafe(link_section = ".init_array")]
static REGISTER_SUJIU_NAPI_MODULE: extern "C" fn() = {
    extern "C" fn constructor() {
        unsafe { napi_module_register(std::ptr::addr_of_mut!(SUJIU_NAPI_MODULE)) };
    }
    constructor
};
