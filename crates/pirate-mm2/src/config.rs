//! MM2 configuration

use serde::{Deserialize, Serialize};

/// MM2 configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mm2Config {
    /// RPC port
    pub rpc_port: u16,
    /// GUI name
    pub gui: String,
    /// Passphrase (encrypted)
    pub passphrase: String,
    /// RPC userpass authentication
    pub userpass: String,
}

impl Default for Mm2Config {
    fn default() -> Self {
        Self {
            rpc_port: 7783,
            gui: "pirate-wallet".to_string(),
            passphrase: String::new(),
            userpass: String::new(),
        }
    }
}

