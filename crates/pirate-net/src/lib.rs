//! Network privacy layer
//!
//! Provides Tor integration via Arti, DoH/system DNS, SOCKS5 proxy,
//! and TLS certificate pinning for secure, private connections.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod debug_log;
pub mod dns;
pub mod error;
pub mod i2p;
pub mod lightwalletd_pins;
pub mod proxy;
pub mod tls;
pub mod tor;
mod transport;
pub mod transport_config;

// Re-export main types
pub use dns::{DnsConfig, DnsProvider, DnsResolver};
pub use error::{Error, Result};
pub use i2p::{I2pClient, I2pConfig, I2pStatus};
pub use lightwalletd_pins::LightwalletdPins;
pub use proxy::ProxyConfig;
pub use tls::TlsPinning;
// Re-export CertificatePin from tls module
pub use crate::tls::CertificatePin;
pub use tor::{TorBridgeConfig, TorBridgeTransport, TorClient, TorConfig, TorStatus};
pub use transport::{Socks5Config, TransportConfig, TransportManager, TransportMode};
pub use transport_config::{StoredTransportConfig, TransportConfigStorage};
