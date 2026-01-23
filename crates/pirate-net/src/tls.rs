//! TLS certificate pinning
//!
//! Provides MITM protection through certificate pinning.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Certificate pin (SHA256 fingerprint of SPKI)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificatePin {
    /// Host this pin applies to
    pub host: String,
    /// SHA256 hash of Subject Public Key Info (SPKI)
    pub spki_sha256: String,
    /// Human-readable description
    pub description: String,
    /// Expiry date (optional)
    pub expires: Option<String>,
}

impl CertificatePin {
    /// Create new certificate pin
    pub fn new(host: String, spki_sha256: String, description: String) -> Self {
        Self {
            host,
            spki_sha256,
            description,
            expires: None,
        }
    }

    /// Validate pin format (base64 SHA256)
    pub fn validate(&self) -> Result<()> {
        if self.spki_sha256.len() != 44 {
            return Err(Error::Tls(format!(
                "Invalid pin format: expected 44 chars, got {}",
                self.spki_sha256.len()
            )));
        }
        Ok(())
    }
}

/// TLS pinning manager
pub struct TlsPinning {
    /// Pins mapped by host
    pins: HashMap<String, Vec<CertificatePin>>,
    /// Enforce pinning (fail on mismatch)
    enforce: bool,
}

impl TlsPinning {
    /// Create new TLS pinning manager
    pub fn new(enforce: bool) -> Self {
        info!("Creating TLS pinning manager (enforce={})", enforce);
        Self {
            pins: HashMap::new(),
            enforce,
        }
    }

    /// Add certificate pin for a host
    pub fn add_pin(&mut self, pin: CertificatePin) -> Result<()> {
        pin.validate()?;

        info!("Adding TLS pin for {}: {}", pin.host, pin.description);

        self.pins.entry(pin.host.clone()).or_default().push(pin);

        Ok(())
    }

    /// Remove all pins for a host
    pub fn remove_pins(&mut self, host: &str) {
        info!("Removing all TLS pins for {}", host);
        self.pins.remove(host);
    }

    /// Get pins for a host
    pub fn get_pins(&self, host: &str) -> Vec<&CertificatePin> {
        self.pins
            .get(host)
            .map(|pins| pins.iter().collect())
            .unwrap_or_default()
    }

    /// Verify certificate against pins
    pub fn verify(&self, host: &str, cert_spki_sha256: &str) -> Result<()> {
        if let Some(pins) = self.pins.get(host) {
            debug!("Verifying {} pins for {}", pins.len(), host);

            // Check if any pin matches
            let matches = pins.iter().any(|pin| pin.spki_sha256 == cert_spki_sha256);

            if !matches {
                let error_msg = format!(
                    "Certificate pin mismatch for {}! Expected one of {:?}, got {}",
                    host,
                    pins.iter().map(|p| &p.spki_sha256).collect::<Vec<_>>(),
                    cert_spki_sha256
                );

                if self.enforce {
                    return Err(Error::Tls(error_msg));
                } else {
                    warn!("{} (not enforced)", error_msg);
                }
            } else {
                debug!("Certificate pin verified for {}", host);
            }
        } else {
            debug!("No pins configured for {}", host);
        }

        Ok(())
    }

    /// Load default pins for known services
    ///
    /// NOTE: Currently disabled because lightwalletd servers use gRPC/HTTP2
    /// and don't present certificates in a way that can be easily extracted.
    /// TLS connections still work, but certificate pinning is disabled.
    ///
    /// When proper certificates become available, add them here.
    pub fn load_defaults(&mut self) -> Result<()> {
        info!("TLS pinning disabled - lightwalletd servers use gRPC/HTTP2 without extractable certificates");

        // TLS pinning is currently disabled because:
        // 1. Servers use gRPC/HTTP2 which doesn't expose certificates to browsers
        // 2. Certificates are not in Certificate Transparency logs
        // 3. OpenSSL s_client cannot extract certificates from gRPC endpoints
        //
        // TLS connections still work - we just don't pin certificates.
        // When proper certificates become available, uncomment and add pins:
        // crate::lightwalletd_pins::LightwalletdPins::load_all(self)?;

        Ok(())
    }

    /// Export pins for backup
    pub fn export(&self) -> Result<String> {
        let all_pins: Vec<&CertificatePin> =
            self.pins.values().flat_map(|pins| pins.iter()).collect();

        serde_json::to_string_pretty(&all_pins)
            .map_err(|e| Error::Tls(format!("Failed to export pins: {}", e)))
    }

    /// Import pins from backup
    pub fn import(&mut self, json: &str) -> Result<()> {
        let pins: Vec<CertificatePin> = serde_json::from_str(json)
            .map_err(|e| Error::Tls(format!("Failed to parse pins: {}", e)))?;

        for pin in pins {
            self.add_pin(pin)?;
        }

        Ok(())
    }

    /// Check if enforcement is enabled
    pub fn is_enforced(&self) -> bool {
        self.enforce
    }

    /// Set enforcement mode
    pub fn set_enforce(&mut self, enforce: bool) {
        info!("Setting TLS pin enforcement: {}", enforce);
        self.enforce = enforce;
    }
}

impl Default for TlsPinning {
    fn default() -> Self {
        let mut pinning = Self::new(true); // Enforce by default
        let _ = pinning.load_defaults(); // Load default pins
        pinning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_certificate_pin_validation() {
        let valid_pin = CertificatePin::new(
            "example.com".to_string(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(), // 44 chars
            "Test".to_string(),
        );
        assert!(valid_pin.validate().is_ok());

        let invalid_pin = CertificatePin::new(
            "example.com".to_string(),
            "TOO_SHORT".to_string(),
            "Test".to_string(),
        );
        assert!(invalid_pin.validate().is_err());
    }

    #[test]
    fn test_tls_pinning_add_verify() {
        let mut pinning = TlsPinning::new(true);

        let pin = CertificatePin::new(
            "test.com".to_string(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
            "Test".to_string(),
        );

        pinning.add_pin(pin.clone()).unwrap();

        // Verify with correct pin
        assert!(pinning.verify("test.com", &pin.spki_sha256).is_ok());

        // Verify with wrong pin (should fail with enforcement)
        assert!(pinning
            .verify("test.com", "WRONG_PIN_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .is_err());
    }

    #[test]
    fn test_tls_pinning_export_import() {
        let mut pinning = TlsPinning::new(true);

        let pin = CertificatePin::new(
            "test.com".to_string(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
            "Test".to_string(),
        );

        pinning.add_pin(pin.clone()).unwrap();

        // Export
        let exported = pinning.export().unwrap();

        // Import into new instance
        let mut pinning2 = TlsPinning::new(true);
        pinning2.import(&exported).unwrap();

        // Verify
        assert!(pinning2.verify("test.com", &pin.spki_sha256).is_ok());
    }

    #[test]
    fn test_enforcement_mode() {
        let mut pinning = TlsPinning::new(false); // Not enforced

        let pin = CertificatePin::new(
            "test.com".to_string(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
            "Test".to_string(),
        );

        pinning.add_pin(pin).unwrap();

        // Wrong pin should succeed when not enforced
        assert!(pinning
            .verify("test.com", "WRONG_PIN_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .is_ok());

        // Enable enforcement
        pinning.set_enforce(true);

        // Now should fail
        assert!(pinning
            .verify("test.com", "WRONG_PIN_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .is_err());
    }
}
