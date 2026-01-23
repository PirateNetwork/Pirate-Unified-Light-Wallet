//! Lightwalletd certificate pins
//!
//! Default certificate pins for known Pirate Chain lightwalletd servers.

use crate::{CertificatePin, Result, TlsPinning};
use base64::engine::general_purpose::STANDARD as Base64Standard;
use base64::Engine;
use native_tls::TlsConnector as NativeTlsConnector;
use rustls_pki_types::CertificateDer;
use webpki::EndEntityCert;
use sha2::{Digest, Sha256};
use tokio_native_tls::TlsConnector;

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
    fn load_community(_pinning: &mut TlsPinning) -> Result<()> {
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
    pub fn extract_spki_from_pem(_pem: &str) -> Result<String> {
        let der = pem_to_der(_pem)?;
        extract_spki_from_cert_der(&der)
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
    pub fn complete_rotation(pinning: &mut TlsPinning, host: &str, old_spki: &str) -> Result<()> {
        let existing = pinning.get_pins(host);
        let mut remaining = Vec::new();
        let mut removed = false;

        for pin in existing {
            if pin.spki_sha256 == old_spki {
                removed = true;
                continue;
            }
            remaining.push(pin.clone());
        }

        if !removed {
            return Err(crate::Error::Tls(format!(
                "Old pin not found for {}. Nothing to remove.",
                host
            )));
        }

        pinning.remove_pins(host);
        for pin in remaining {
            pinning.add_pin(pin)?;
        }

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
/// This is intended for pin extraction only and does not validate certificates.
pub async fn extract_spki_from_server(host: &str, port: u16) -> Result<String> {
    let addr = format!("{}:{}", host, port);
    let tcp = tokio::net::TcpStream::connect(&addr)
        .await
        .map_err(|e| crate::Error::Tls(format!("TCP connect failed: {}", e)))?;

    let connector = NativeTlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .map_err(|e| crate::Error::Tls(format!("TLS connector build failed: {}", e)))?;

    let connector = TlsConnector::from(connector);
    let stream = connector
        .connect(host, tcp)
        .await
        .map_err(|e| crate::Error::Tls(format!("TLS handshake failed: {}", e)))?;

    let cert = stream
        .get_ref()
        .peer_certificate()
        .map_err(|e| crate::Error::Tls(format!("TLS peer certificate error: {}", e)))?
        .ok_or_else(|| crate::Error::Tls("No peer certificate presented".to_string()))?;

    let der = cert
        .to_der()
        .map_err(|e| crate::Error::Tls(format!("Failed to read DER certificate: {}", e)))?;

    extract_spki_from_cert_der(&der)
}

fn extract_spki_from_cert_der(cert_der: &[u8]) -> Result<String> {
    let cert = CertificateDer::from(cert_der.to_vec());
    let end_entity = EndEntityCert::try_from(&cert)
        .map_err(|e| crate::Error::Tls(format!("Certificate parse failed: {:?}", e)))?;
    let spki = end_entity.subject_public_key_info();
    Ok(spki_sha256_base64(spki.as_ref()))
}

fn spki_sha256_base64(spki_der: &[u8]) -> String {
    let digest = Sha256::digest(spki_der);
    Base64Standard.encode(digest)
}

fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let mut collecting = false;
    let mut b64 = String::new();

    for line in pem.lines() {
        let line = line.trim();
        if line.starts_with("-----BEGIN CERTIFICATE-----") {
            collecting = true;
            continue;
        }
        if line.starts_with("-----END CERTIFICATE-----") {
            break;
        }
        if collecting {
            b64.push_str(line);
        }
    }

    if b64.is_empty() {
        return Err(crate::Error::Tls(
            "No PEM certificate found".to_string(),
        ));
    }

    Base64Standard
        .decode(b64.as_bytes())
        .map_err(|e| crate::Error::Tls(format!("PEM base64 decode failed: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_all_pins() {
        let mut pinning = TlsPinning::new(true);

        LightwalletdPins::load_all(&mut pinning).unwrap();

        // TLS pinning is intentionally empty until gRPC cert extraction is available.
        let pins = pinning.get_pins("lightd.pirate.black");
        assert!(pins.is_empty(), "Pins should be empty while disabled");
    }

    #[test]
    fn test_pin_rotation_workflow() {
        let mut pinning = TlsPinning::new(true);

        let old_pin = ["AAAAAAAAAAA"; 4].concat();
        let new_pin = ["BBBBBBBBBBB"; 4].concat();
        assert_eq!(old_pin.len(), 44);
        assert_eq!(new_pin.len(), 44);

        // Add initial pin
        pinning
            .add_pin(CertificatePin::new(
                "test.lightd.pirate.black".to_string(),
                old_pin.clone(),
                "Old cert".to_string(),
            ))
            .unwrap();

        // Rotate to new pin
        LightwalletdPins::rotate_pin(
            &mut pinning,
            "test.lightd.pirate.black",
            &old_pin,
            &new_pin,
            "New cert",
        )
        .unwrap();

        // Both pins should work during grace period
        let pins = pinning.get_pins("test.lightd.pirate.black");
        assert_eq!(
            pins.len(),
            2,
            "Should have both old and new pins during grace period"
        );

        // Verify both pins work
        assert!(pinning.verify("test.lightd.pirate.black", &old_pin).is_ok());

        assert!(pinning.verify("test.lightd.pirate.black", &new_pin).is_ok());
    }
}
