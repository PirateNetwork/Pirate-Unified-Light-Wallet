//! Fuzz test for address decoding
//!
//! Ensures address parser handles malformed input gracefully

#![no_main]

use libfuzzer_sys::fuzz_target;
use pirate_core::keys::PaymentAddress;

fuzz_target!(|data: &[u8]| {
    // Convert bytes to string
    if let Ok(s) = std::str::from_utf8(data) {
        // Should never panic, only return Err for invalid input
        let _ = PaymentAddress::decode(s);
    }
});

