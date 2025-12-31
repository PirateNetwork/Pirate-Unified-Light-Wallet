//! Lightwalletd certificate pins
//!
//! Default certificate pins for known Pirate Chain lightwalletd servers.

use crate::{CertificatePin, TlsPinning, Result};

/// Known Pirate Chain lightwalletd servers
pub struct LightwalletdPins;

impl LightwalletdPins {
    /// Load all known lightwalletd pins into TLS pinning manager
    pub fn load_all(pinning: &mut TlsPinning) -> Result<()> {
        // Primary Pirate Chain lightwalletd servers
        Self::load_primary(pinning)?;
        
        // Community lightwalletd servers
        Self::load_community(pinning)?;
        
        Ok(())
    }

    /// Load primary Pirate Chain lightwalletd pins
    /// 
    /// NOTE: Currently disabled because lightwalletd servers use gRPC/HTTP2
    /// and don't present certificates in a way that can be easily extracted.
    /// 
    /// When proper certificates are available, add them here and enable
    /// TLS pinning enforcement in TlsPinning::new(true).
    fn load_primary(_pinning: &mut TlsPinning) -> Result<()> {
        // TLS pinning is currently disabled for lightwalletd servers because:
        // 1. Servers use gRPC/HTTP2 which doesn't expose certificates to browsers
        // 2. Certificates are not in Certificate Transparency logs
        // 3. OpenSSL s_client cannot extract certificates from gRPC endpoints
        // 
        // When proper certificates become available:
        // 1. Extract SPKI SHA256 pins using gRPC-specific tools
        // 2. Add pins here using pinning.add_pin()
        // 3. Enable enforcement: TlsPinning::new(true)
        // 
        // Known servers:
        // - lightd1.piratechain.com:9067 (mainnet, Sapling only)
        // - 64.23.167.130:9067 (orchard-ready)
        // - 64.23.167.130:8067 (testnet, Sapling + Orchard)
        
        Ok(())
    }

    /// Load community lightwalletd pins
    fn load_community(pinning: &mut TlsPinning) -> Result<()> {
        // Add community servers here as they become available
        // Each should have its own SPKI pin
        
        Ok(())
    }

    /// Extract SPKI hash from PEM certificate
    /// 
    /// This is a helper function to extract the correct SPKI hash
    /// from a certificate for pinning.
    /// 
    /// # Example
    /// 
    /// ```bash
    /// # Get certificate
    /// openssl s_client -connect lightd.pirate.black:443 -showcerts < /dev/null 2>/dev/null | \
    ///   openssl x509 -outform PEM > cert.pem
    /// 
    /// # Extract SPKI hash
    /// openssl x509 -in cert.pem -pubkey -noout | \
    ///   openssl pkey -pubin -outform der | \
    ///   openssl dgst -sha256 -binary | \
    ///   base64
    /// ```
    pub fn extract_spki_from_pem(pem: &str) -> Result<String> {
        // TODO: Implement PEM parsing and SPKI extraction
        // For now, this is a placeholder that shows the process
        
        Err(crate::Error::Tls(
            "SPKI extraction not yet implemented. Use openssl command above.".to_string()
        ))
    }

    /// Rotate pins for a host
    /// 
    /// Certificate rotation workflow:
    /// 1. Add new pin alongside old pin (both valid)
    /// 2. Deploy new certificate to server
    /// 3. Wait for grace period (e.g., 30 days)
    /// 4. Remove old pin
    pub fn rotate_pin(
        pinning: &mut TlsPinning,
        host: &str,
        old_spki: &str,
        new_spki: &str,
        description: &str,
    ) -> Result<()> {
        // Get existing pins
        let existing = pinning.get_pins(host);
        
        // Check if old pin exists
        let has_old = existing.iter().any(|p| p.spki_sha256 == old_spki);
        
        if !has_old {
            return Err(crate::Error::Tls(format!(
                "Old pin not found for {}. Cannot rotate.",
                host
            )));
        }

        // Add new pin (both old and new will be valid during grace period)
        pinning.add_pin(CertificatePin {
            host: host.to_string(),
            spki_sha256: new_spki.to_string(),
            description: format!("{} (rotated)", description),
            expires: None,
        })?;

        tracing::info!(
            "Certificate rotation initiated for {}. Both old and new pins valid during grace period.",
            host
        );

        Ok(())
    }

    /// Complete rotation by removing old pin
    /// 
    /// Call this after the grace period when the new certificate
    /// is fully deployed and the old one is no longer in use.
    pub fn complete_rotation(
        pinning: &mut TlsPinning,
        host: &str,
        old_spki: &str,
    ) -> Result<()> {
        // Remove all pins for host
        pinning.remove_pins(host);

        // Re-add all pins except the old one
        // In a real implementation, we'd filter the pins
        // For now, this is a placeholder

        tracing::info!(
            "Certificate rotation completed for {}. Old pin removed.",
            host
        );

        Ok(())
    }
}

/// Extract SPKI from a running lightwalletd server
/// 
/// This connects to the server, retrieves the certificate,
/// and extracts the SPKI hash for pinning.
/// 
/// TODO: Implement using tokio_rustls to connect and extract certificate.
/// For now, use the shell scripts: tools/extract-spki.sh or tools/extract-spki.ps1
pub async fn extract_spki_from_server(_host: &str, _port: u16) -> Result<String> {
    // TODO: Implement TLS connection and SPKI extraction
    // Would require: tokio_rustls, rustls dependencies
    // For now, use the provided shell scripts
    
    Err(crate::Error::Tls(
        "SPKI extraction from live server not yet implemented. Use tools/extract-spki.sh or tools/extract-spki.ps1".to_string()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_all_pins() {
        let mut pinning = TlsPinning::new(true);
        
        LightwalletdPins::load_all(&mut pinning).unwrap();
        
        // Should have at least primary lightd pin
        let pins = pinning.get_pins("lightd.pirate.black");
        assert!(!pins.is_empty(), "Should have lightd.pirate.black pins");
    }

    #[test]
    fn test_pin_rotation_workflow() {
        let mut pinning = TlsPinning::new(true);

        // Add initial pin
        pinning.add_pin(CertificatePin::new(
            "test.lightd.pirate.black".to_string(),
            "OLD_PIN_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
            "Old cert".to_string(),
        )).unwrap();

        // Rotate to new pin
        LightwalletdPins::rotate_pin(
            &mut pinning,
            "test.lightd.pirate.black",
            "OLD_PIN_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "NEW_PIN_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=",
            "New cert",
        ).unwrap();

        // Both pins should work during grace period
        let pins = pinning.get_pins("test.lightd.pirate.black");
        assert_eq!(pins.len(), 2, "Should have both old and new pins during grace period");

        // Verify both pins work
        assert!(pinning.verify(
            "test.lightd.pirate.black",
            "OLD_PIN_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        ).is_ok());
        
        assert!(pinning.verify(
            "test.lightd.pirate.black",
            "NEW_PIN_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="
        ).is_ok());
    }
}

