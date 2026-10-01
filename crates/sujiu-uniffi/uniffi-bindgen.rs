//! The binding generator.
//!
//! Built only when the `bindgen` feature is on, so an ordinary build of the
//! runtime never pulls the generator in. `scripts/generate-bindings.sh` runs it
//! and CI runs the drift check against whatever it writes.

fn main() {
    uniffi::uniffi_bindgen_main()
}
