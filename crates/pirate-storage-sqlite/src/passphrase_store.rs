//! In-memory storage for the app passphrase.
//!
//! The passphrase is held only for the lifetime of the process and is never persisted.

use crate::{Error, Result};
use parking_lot::RwLock;
use std::sync::OnceLock;
use zeroize::Zeroizing;

static PASSPHRASE_STORE: OnceLock<RwLock<Option<Zeroizing<String>>>> = OnceLock::new();

fn store() -> &'static RwLock<Option<Zeroizing<String>>> {
    PASSPHRASE_STORE.get_or_init(|| RwLock::new(None))
}

/// Store the app passphrase in memory.
pub fn set_passphrase(passphrase: String) {
    *store().write() = Some(Zeroizing::new(passphrase));
}

/// Clear the in-memory passphrase.
pub fn clear_passphrase() {
    *store().write() = None;
}

/// Check whether a passphrase is loaded in memory.
pub fn is_passphrase_set() -> bool {
    store().read().is_some()
}

/// Get the in-memory passphrase.
pub fn get_passphrase() -> Result<Zeroizing<String>> {
    store()
        .read()
        .as_ref()
        .map(|p| Zeroizing::new(p.to_string()))
        .ok_or_else(|| Error::Security("App is locked".to_string()))
}
