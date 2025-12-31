//! Fuzz test for memo parsing
//!
//! Ensures memo parser handles arbitrary input gracefully

#![no_main]

use libfuzzer_sys::fuzz_target;
use pirate_core::memo::Memo;

fuzz_target!(|data: &[u8]| {
    // Try to parse as UTF-8 string
    if let Ok(s) = std::str::from_utf8(data) {
        // Should never panic
        let _ = Memo::from_str(s);
    }
    
    // Also try raw bytes (for binary memo parsing)
    let _ = Memo::from_bytes(data);
});

