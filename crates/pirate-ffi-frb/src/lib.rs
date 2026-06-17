#![allow(unexpected_cfgs)]
// flutter_rust_bridge uses cfg flags like `frb_expand` during codegen / macro expansion.
// On stable Rust, `unexpected_cfgs` warns about unknown cfg names; we allow them in this crate.

// Flutter Rust Bridge FFI surface
//
// Minimal, precise API for Flutter integration.
//
// ## FRB Code Generation
//
// This crate uses flutter_rust_bridge for FFI. To regenerate bindings:
//
// ```bash
// flutter_rust_bridge_codegen generate --config-file flutter_rust_bridge.yaml
// ```
//
// Generated files (see `flutter_rust_bridge.yaml`):
// - `src/frb_generated.rs` - Rust FFI glue
// - `app/lib/core/ffi/generated/*.dart` - Dart bindings

// Note: We allow unsafe code only for the FRB-generated module.
// All hand-written code in this crate avoids unsafe.

mod frb_generated; /* AUTO INJECTED BY flutter_rust_bridge. This line may not be accurate, and you can change it according to your needs. */

pub mod api;
pub mod background;
pub mod models;
pub mod streams;

pub use api::*;
pub use models::*;

#[no_mangle]
pub extern "C" fn pirate_debug_log_set_enabled(enabled: u8) {
    let _ = crate::api::set_debug_logging_enabled(enabled != 0);
}

#[no_mangle]
pub extern "C" fn pirate_debug_log_is_enabled() -> u8 {
    if pirate_core::debug_log::is_enabled() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn pirate_debug_log_clear() {
    let _ = crate::api::clear_debug_logs();
}
