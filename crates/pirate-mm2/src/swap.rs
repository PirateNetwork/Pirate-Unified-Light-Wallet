//! Atomic swap state machine and progress tracking

use crate::{client::*, Error, Result};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Swap progress stage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwapStage {
    /// Initiating swap
    Initiating,
    /// Negotiating with counterparty
    Negotiating,
    /// Sending taker fee
    SendingFee,
    /// Waiting for maker payment
    WaitingForMakerPayment,
    /// Validating maker payment
    ValidatingMakerPayment,
    /// Sending taker payment
    SendingTakerPayment,
    /// Waiting for completion
    WaitingForCompletion,
    /// Swap completed successfully
    Completed,
    /// Swap failed
    Failed,
    /// Swap refunded
    Refunded,
}

impl SwapStage {
    /// Get human-readable stage name
    pub fn display_name(&self) -> &'static str {
        match self {
            SwapStage::Initiating => "Initiating Swap",
            SwapStage::Negotiating => "Negotiating",
            SwapStage::SendingFee => "Sending Fee",
            SwapStage::WaitingForMakerPayment => "Waiting for Payment",
            SwapStage::ValidatingMakerPayment => "Validating Payment",
            SwapStage::SendingTakerPayment => "Sending Payment",
            SwapStage::WaitingForCompletion => "Completing Swap",
            SwapStage::Completed => "Completed",
            SwapStage::Failed => "Failed",
            SwapStage::Refunded => "Refunded",
        }
    }

    /// Get progress percentage (0-100)
    pub fn progress_percent(&self) -> u8 {
        match self {
            SwapStage::Initiating => 0,
            SwapStage::Negotiating => 10,
            SwapStage::SendingFee => 20,
            SwapStage::WaitingForMakerPayment => 40,
            SwapStage::ValidatingMakerPayment => 60,
            SwapStage::SendingTakerPayment => 75,
            SwapStage::WaitingForCompletion => 90,
            SwapStage::Completed => 100,
            SwapStage::Failed => 0,
            SwapStage::Refunded => 0,
        }
    }

    /// Is swap in terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SwapStage::Completed | SwapStage::Failed | SwapStage::Refunded
        )
    }

    /// Is swap successful
    pub fn is_successful(&self) -> bool {
        matches!(self, SwapStage::Completed)
    }
}

/// Swap progress tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapProgress {
    /// Swap UUID
    pub uuid: String,
    /// Current stage
    pub stage: SwapStage,
    /// Base coin (what we're buying)
    pub base_coin: String,
    /// Rel coin (what we're spending)
    pub rel_coin: String,
    /// Base amount
    pub base_amount: String,
    /// Rel amount
    pub rel_amount: String,
    /// Started at
    pub started_at: SystemTime,
    /// Last updated
    pub updated_at: SystemTime,
    /// Error message if failed
    pub error: Option<String>,
}

impl SwapProgress {
    /// Create new swap progress tracker
    pub fn new(result: SwapResult) -> Self {
        let now = SystemTime::now();
        
        Self {
            uuid: result.uuid,
            stage: SwapStage::Initiating,
            base_coin: result.base,
            rel_coin: result.rel,
            base_amount: result.base_amount,
            rel_amount: result.rel_amount,
            started_at: now,
            updated_at: now,
            error: None,
        }
    }

    /// Update from MM2 swap status
    pub fn update_from_status(&mut self, status: SwapStatus) {
        self.updated_at = SystemTime::now();
        
        // Determine stage from events
        let latest_event = status.events.last();
        
        if let Some(event) = latest_event {
            self.stage = Self::event_to_stage(&event.event);
            
            // Capture error if failed
            if matches!(self.stage, SwapStage::Failed) {
                self.error = Some(format!("{:?} failed", event.event));
            }
        }
    }

    /// Map MM2 event to swap stage
    fn event_to_stage(event: &SwapEventType) -> SwapStage {
        match event {
            SwapEventType::Started => SwapStage::Initiating,
            SwapEventType::Negotiated => SwapStage::Negotiating,
            SwapEventType::TakerFeeSent => SwapStage::SendingFee,
            SwapEventType::MakerPaymentReceived |
            SwapEventType::MakerPaymentWaitConfirmStarted => SwapStage::WaitingForMakerPayment,
            SwapEventType::MakerPaymentValidatedAndConfirmed => SwapStage::ValidatingMakerPayment,
            SwapEventType::TakerPaymentSent => SwapStage::SendingTakerPayment,
            SwapEventType::TakerPaymentSpent |
            SwapEventType::MakerPaymentSpent => SwapStage::WaitingForCompletion,
            SwapEventType::Finished => SwapStage::Completed,
            SwapEventType::MakerPaymentRefunded => SwapStage::Refunded,
            _ => SwapStage::Failed,
        }
    }

    /// Get elapsed time in seconds
    pub fn elapsed_secs(&self) -> u64 {
        self.started_at
            .elapsed()
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Get progress percentage
    pub fn progress_percent(&self) -> u8 {
        self.stage.progress_percent()
    }
}

/// Swap manager for tracking multiple swaps
pub struct SwapManager {
    swaps: std::collections::HashMap<String, SwapProgress>,
}

impl SwapManager {
    /// Create new swap manager
    pub fn new() -> Self {
        Self {
            swaps: std::collections::HashMap::new(),
        }
    }

    /// Add swap to tracker
    pub fn add_swap(&mut self, result: SwapResult) {
        let progress = SwapProgress::new(result);
        self.swaps.insert(progress.uuid.clone(), progress);
    }

    /// Update swap progress
    pub fn update_swap(&mut self, uuid: &str, status: SwapStatus) -> Result<()> {
        let swap = self.swaps.get_mut(uuid)
            .ok_or_else(|| Error::Swap(format!("Swap {} not found", uuid)))?;
        
        swap.update_from_status(status);
        Ok(())
    }

    /// Get swap progress
    pub fn get_swap(&self, uuid: &str) -> Option<&SwapProgress> {
        self.swaps.get(uuid)
    }

    /// Get all swaps
    pub fn all_swaps(&self) -> Vec<&SwapProgress> {
        self.swaps.values().collect()
    }

    /// Get active swaps (not terminal)
    pub fn active_swaps(&self) -> Vec<&SwapProgress> {
        self.swaps
            .values()
            .filter(|s| !s.stage.is_terminal())
            .collect()
    }

    /// Get completed swaps
    pub fn completed_swaps(&self) -> Vec<&SwapProgress> {
        self.swaps
            .values()
            .filter(|s| s.stage == SwapStage::Completed)
            .collect()
    }

    /// Remove swap
    pub fn remove_swap(&mut self, uuid: &str) {
        self.swaps.remove(uuid);
    }
}

impl Default for SwapManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Quote for buying ARRR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuyArrrQuote {
    /// Source coin (what user will spend)
    pub source_coin: String,
    /// Source amount
    pub source_amount: String,
    /// ARRR amount to receive
    pub arrr_amount: String,
    /// Exchange rate
    pub rate: String,
    /// Estimated network fees
    pub estimated_fee: String,
    /// Best order UUID
    pub order_uuid: String,
    /// Valid until (timestamp)
    pub valid_until: SystemTime,
}

impl BuyArrrQuote {
    /// Check if quote is still valid
    pub fn is_valid(&self) -> bool {
        SystemTime::now() < self.valid_until
    }

    /// Get remaining validity in seconds
    pub fn remaining_validity_secs(&self) -> u64 {
        self.valid_until
            .duration_since(SystemTime::now())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_stage_progress() {
        assert_eq!(SwapStage::Initiating.progress_percent(), 0);
        assert_eq!(SwapStage::Completed.progress_percent(), 100);
        assert!(SwapStage::Completed.is_terminal());
        assert!(SwapStage::Completed.is_successful());
        assert!(!SwapStage::Negotiating.is_terminal());
    }

    #[test]
    fn test_swap_manager() {
        let mut manager = SwapManager::new();
        
        let result = SwapResult {
            uuid: "test-uuid".to_string(),
            action: "buy".to_string(),
            base: "ARRR".to_string(),
            base_amount: "100".to_string(),
            rel: "BTC".to_string(),
            rel_amount: "0.001".to_string(),
            method: "setprice".to_string(),
            sender_pubkey: "".to_string(),
            dest_pub_key: "".to_string(),
        };

        manager.add_swap(result);
        
        assert_eq!(manager.all_swaps().len(), 1);
        assert_eq!(manager.active_swaps().len(), 1);
        assert_eq!(manager.completed_swaps().len(), 0);
        
        let swap = manager.get_swap("test-uuid").unwrap();
        assert_eq!(swap.stage, SwapStage::Initiating);
    }

    #[test]
    fn test_quote_validity() {
        let quote = BuyArrrQuote {
            source_coin: "BTC".to_string(),
            source_amount: "0.001".to_string(),
            arrr_amount: "100".to_string(),
            rate: "0.00001".to_string(),
            estimated_fee: "0.0001".to_string(),
            order_uuid: "test".to_string(),
            valid_until: SystemTime::now() + std::time::Duration::from_secs(60),
        };

        assert!(quote.is_valid());
        assert!(quote.remaining_validity_secs() > 50);
    }
}
