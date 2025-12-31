//! Unstoppable Domains resolution

use crate::{Error, Result};

/// Unstoppable Domains resolver
pub struct UnstoppableResolver {
    api_key: Option<String>,
}

impl UnstoppableResolver {
    /// Create new resolver
    pub fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }

    /// Resolve domain to ARRR address
    pub async fn resolve(&self, domain: &str) -> Result<String> {
        if !domain.ends_with(".arrr") && !domain.ends_with(".crypto") {
            return Err(Error::InvalidName(format!("Invalid domain: {}", domain)));
        }

        #[cfg(feature = "names")]
        {
            // In production, query Unstoppable API
            tracing::info!("Resolving {} via Unstoppable", domain);
            Err(Error::Resolution("Not implemented".to_string()))
        }

        #[cfg(not(feature = "names"))]
        {
            Err(Error::FeatureDisabled)
        }
    }
}

impl Default for UnstoppableResolver {
    fn default() -> Self {
        Self::new(None)
    }
}

