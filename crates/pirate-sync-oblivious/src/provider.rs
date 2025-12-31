//! Oblivious sync provider

use crate::{Error, Result};
use async_trait::async_trait;

/// Sync provider trait
#[async_trait]
pub trait SyncProvider {
    /// Start sync
    async fn sync(&mut self) -> Result<()>;
}

/// Oblivious sync provider
pub struct ObliviousProvider;

impl ObliviousProvider {
    /// Create new provider
    pub fn new() -> Self {
        Self
    }
}

impl Default for ObliviousProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SyncProvider for ObliviousProvider {
    async fn sync(&mut self) -> Result<()> {
        #[cfg(feature = "oblivious_sync")]
        {
            // Future implementation
            Err(Error::NotImplemented)
        }
        
        #[cfg(not(feature = "oblivious_sync"))]
        {
            Err(Error::NotImplemented)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "Oblivious sync not implemented"]
    async fn test_oblivious_sync() {
        let mut provider = ObliviousProvider::new();
        let result = provider.sync().await;
        assert!(result.is_err());
    }
}

