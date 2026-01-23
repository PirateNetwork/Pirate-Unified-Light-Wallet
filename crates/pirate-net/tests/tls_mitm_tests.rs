//! TLS pinning MITM prevention tests
//!
//! Tests proving certificate pinning protects against man-in-the-middle attacks.

use pirate_net::{CertificatePin, TlsPinning};

fn make_pin(prefix: &str, fill: char) -> String {
    let mut pin = prefix.to_string();
    while pin.len() < 44 {
        pin.push(fill);
    }
    pin.truncate(44);
    pin
}

#[test]
fn test_pin_enforcement_blocks_mismatch() {
    let mut pinning = TlsPinning::new(true); // Enforce

    // Add pin for test.com
    let pin = CertificatePin::new(
        "test.com".to_string(),
        make_pin("PIN_A_", 'A'),
        "Test pin".to_string(),
    );

    pinning.add_pin(pin.clone()).unwrap();

    // Correct pin should pass
    assert!(pinning.verify("test.com", &pin.spki_sha256).is_ok());

    // Wrong pin should FAIL (MITM detected!)
    let wrong_pin = make_pin("PIN_B_", 'B');
    let result = pinning.verify("test.com", &wrong_pin);

    assert!(
        result.is_err(),
        "Wrong pin should be rejected when enforced"
    );
    assert!(result.unwrap_err().to_string().contains("pin mismatch"));
}

#[test]
fn test_pin_warning_mode_allows_mismatch() {
    let mut pinning = TlsPinning::new(false); // Warn only

    let pin = CertificatePin::new(
        "test.com".to_string(),
        make_pin("PIN_A_", 'A'),
        "Test pin".to_string(),
    );

    pinning.add_pin(pin).unwrap();

    // Wrong pin should PASS with warning (not enforced)
    let wrong_pin = make_pin("PIN_B_", 'B');
    let result = pinning.verify("test.com", &wrong_pin);

    assert!(result.is_ok(), "Wrong pin should pass when not enforced");
}

#[test]
fn test_multiple_pins_any_match() {
    let mut pinning = TlsPinning::new(true);

    // Add two pins for same host (e.g., primary + backup cert)
    let pin1 = CertificatePin::new(
        "example.com".to_string(),
        make_pin("PIN_A_", 'A'),
        "Primary cert".to_string(),
    );

    let pin2 = CertificatePin::new(
        "example.com".to_string(),
        make_pin("PIN_B_", 'B'),
        "Backup cert".to_string(),
    );

    pinning.add_pin(pin1.clone()).unwrap();
    pinning.add_pin(pin2.clone()).unwrap();

    // Either pin should pass
    assert!(pinning.verify("example.com", &pin1.spki_sha256).is_ok());
    assert!(pinning.verify("example.com", &pin2.spki_sha256).is_ok());

    // Different pin should fail
    assert!(pinning
        .verify("example.com", &make_pin("PIN_C_", 'C'))
        .is_err());
}

#[test]
fn test_no_pins_allows_any_cert() {
    let pinning = TlsPinning::new(true);

    // No pins configured for test.com
    // Any cert should pass
    let result = pinning.verify("test.com", &make_pin("ANY_", 'A'));

    assert!(result.is_ok(), "Should pass when no pins configured");
}

#[test]
fn test_pin_validation() {
    // Valid pin (44 chars)
    let valid = CertificatePin::new(
        "test.com".to_string(),
        make_pin("PIN_A_", 'A'),
        "Valid".to_string(),
    );
    assert!(valid.validate().is_ok());

    // Invalid pin (too short)
    let invalid = CertificatePin::new(
        "test.com".to_string(),
        "TOO_SHORT".to_string(),
        "Invalid".to_string(),
    );
    assert!(invalid.validate().is_err());

    // Invalid pin (too long)
    let invalid2 = CertificatePin::new(
        "test.com".to_string(),
        "A".repeat(45),
        "Invalid".to_string(),
    );
    assert!(invalid2.validate().is_err());
}

#[test]
fn test_pin_export_import() {
    let mut pinning1 = TlsPinning::new(true);

    let pin = CertificatePin::new(
        "secure.example.com".to_string(),
        make_pin("SECUREPIN_", 'A'),
        "Secure server".to_string(),
    );

    pinning1.add_pin(pin.clone()).unwrap();

    // Export pins
    let exported = pinning1.export().unwrap();
    assert!(exported.contains("secure.example.com"));
    assert!(exported.contains("SECUREPIN"));

    // Import into new instance
    let mut pinning2 = TlsPinning::new(true);
    pinning2.import(&exported).unwrap();

    // Verify imported pins work
    assert!(pinning2
        .verify("secure.example.com", &pin.spki_sha256)
        .is_ok());
}

#[test]
fn test_pin_rotation() {
    let mut pinning = TlsPinning::new(true);

    // Old pin
    let old_pin = CertificatePin::new(
        "example.com".to_string(),
        make_pin("OLDPIN_", 'A'),
        "Old cert".to_string(),
    );

    pinning.add_pin(old_pin.clone()).unwrap();

    // Old pin works
    assert!(pinning.verify("example.com", &old_pin.spki_sha256).is_ok());

    // Add new pin (for rotation period)
    let new_pin = CertificatePin::new(
        "example.com".to_string(),
        make_pin("NEWPIN_", 'B'),
        "New cert".to_string(),
    );

    pinning.add_pin(new_pin.clone()).unwrap();

    // Both pins should work during rotation
    assert!(pinning.verify("example.com", &old_pin.spki_sha256).is_ok());
    assert!(pinning.verify("example.com", &new_pin.spki_sha256).is_ok());

    // Remove old pin
    pinning.remove_pins("example.com");
    pinning.add_pin(new_pin.clone()).unwrap();

    // Only new pin works now
    assert!(pinning.verify("example.com", &new_pin.spki_sha256).is_ok());
    assert!(pinning.verify("example.com", &old_pin.spki_sha256).is_err());
}

#[test]
fn test_default_pins_loaded() {
    let pinning = TlsPinning::default();

    // Defaults are intentionally empty until gRPC cert extraction is available.
    let pins = pinning.get_pins("lightd.pirate.black");
    assert!(pins.is_empty(), "Pins should be empty while disabled");
}

#[test]
fn test_mitm_scenario_simulation() {
    let mut pinning = TlsPinning::new(true);

    // Legitimate server pin
    let legit_pin = CertificatePin::new(
        "bank.example.com".to_string(),
        make_pin("LEGIT_", 'A'),
        "Bank's real certificate".to_string(),
    );

    pinning.add_pin(legit_pin.clone()).unwrap();

    // Attacker's fake certificate
    let attacker_pin = make_pin("ATTACKER_", 'B');

    // Attack should be detected and blocked
    let result = pinning.verify("bank.example.com", &attacker_pin);

    assert!(result.is_err(), "MITM attack should be detected");
    assert!(result.unwrap_err().to_string().contains("mismatch"));

    // Legitimate cert should still work
    assert!(pinning
        .verify("bank.example.com", &legit_pin.spki_sha256)
        .is_ok());
}
