//! Fuzz test for mnemonic parsing
//!
//! Ensures mnemonic parser handles arbitrary input gracefully

#![no_main]

use libfuzzer_sys::fuzz_target;
use pirate_core::keys::ExtendedSpendingKey;

fuzz_target!(|data: &[u8]| {
    // Convert bytes to string
    if let Ok(s) = std::str::from_utf8(data) {
        // Should never panic, only return Err for invalid input
        let _ = ExtendedSpendingKey::from_mnemonic(s, "");
        
        // Also test with random passphrase
        if data.len() > 10 {
            if let Ok(pass) = std::str::from_utf8(&data[..10]) {
                let _ = ExtendedSpendingKey::from_mnemonic(s, pass);
            }
        }
    }
});

