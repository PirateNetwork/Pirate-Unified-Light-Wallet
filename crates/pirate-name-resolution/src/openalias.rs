//! OpenAlias resolution

use crate::{Error, Result};

/// OpenAlias resolver
pub struct OpenAliasResolver;

impl OpenAliasResolver {
    /// Create new resolver
    pub fn new() -> Self {
        Self
    }

    /// Resolve alias to address
    pub async fn resolve(&self, alias: &str) -> Result<String> {
        if !alias.contains('@') && !alias.contains('.') {
            return Err(Error::InvalidName(format!("Invalid alias: {}", alias)));
        }

        #[cfg(feature = "names")]
        {
            // In production, query DNS TXT records
            tracing::info!("Resolving {} via OpenAlias", alias);
            Err(Error::Resolution("Not implemented".to_string()))
        }

        #[cfg(not(feature = "names"))]
        {
            Err(Error::FeatureDisabled)
        }
    }
}

impl Default for OpenAliasResolver {
    fn default() -> Self {
        Self::new()
    }
}

