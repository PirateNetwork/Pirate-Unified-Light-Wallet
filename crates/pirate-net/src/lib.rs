//! Network privacy layer
//!
//! Provides Tor integration via Arti, DNSCrypt, SOCKS5 proxy,
//! and TLS certificate pinning for secure, private connections.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod dns;
pub mod error;
pub mod lightwalletd_pins;
pub mod proxy;
pub mod tls;
pub mod tor;
mod transport;
pub mod transport_config;

// Re-export main types
pub use dns::{DnsResolver, DnsProvider, DnsConfig};
pub use error::{Error, Result};
pub use lightwalletd_pins::LightwalletdPins;
pub use proxy::ProxyConfig;
pub use tls::TlsPinning;
// Re-export CertificatePin from tls module
pub use crate::tls::CertificatePin;
pub use tor::{TorClient, TorConfig, TorStatus};
pub use transport::{TransportManager, TransportMode, TransportConfig, Socks5Config};
pub use transport_config::{StoredTransportConfig, TransportConfigStorage};

