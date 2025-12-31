//! Transaction fee calculation
//!
//! Pirate Chain wallet defaults to a fixed fee (0.0001 ARRR = 10,000 arrrtoshis),
//! aligned with current chain policy.

use crate::{Error, Result};

/// Default fixed fee (in arrrtoshis)
pub const DEFAULT_FEE: u64 = 10_000;

/// Legacy ZIP-317 constants kept for backward compatibility.
#[deprecated(note = "Pirate uses a fixed fee; use DEFAULT_FEE instead.")]
pub const ZIP317_MARGINAL_FEE: u64 = DEFAULT_FEE;

#[deprecated(note = "ZIP-317 is not used on Pirate; use DEFAULT_FEE instead.")]
pub const ZIP317_GRACE_ACTIONS: u64 = 2;

/// Minimum fee (in arrrtoshis)
pub const MIN_FEE: u64 = DEFAULT_FEE;

/// Maximum fee (safety limit, in arrrtoshis)
/// 0.01 ARRR = 1,000,000 arrrtoshis
pub const MAX_FEE: u64 = 1_000_000;

/// Legacy constant for backward compatibility
#[deprecated(note = "Use DEFAULT_FEE instead")]
pub const DEFAULT_FEE_PER_ACTION: u64 = DEFAULT_FEE;

/// Fee calculator using a fixed Pirate Chain fee.
#[derive(Debug, Clone)]
pub struct FeeCalculator;

impl FeeCalculator {
    /// Create new fee calculator with fixed fees
    pub fn new() -> Self {
        Self
    }

    /// Calculate fee using the fixed Pirate Chain fee.
    ///
    /// # Arguments
    /// * `num_spends` - Number of Sapling spends (logical actions)
    /// * `num_outputs` - Number of Sapling outputs (logical actions)
    /// * `_has_memo` - Whether transaction has memo (ignored for fixed fee)
    ///
    /// # Returns
    /// Fixed fee in arrrtoshis.
    pub fn calculate_fee(
        &self,
        _num_spends: usize,
        _num_outputs: usize,
        _has_memo: bool,
    ) -> Result<u64> {
        let fee = DEFAULT_FEE;

        // Enforce maximum (safety check)
        if fee > MAX_FEE {
            return Err(Error::FeeCalculation(format!(
                "Calculated fee {} exceeds maximum {}",
                fee, MAX_FEE
            )));
        }

        tracing::debug!("Fixed fee: {} arrrtoshis", fee);

        Ok(fee)
    }

    /// Calculate fee for simple send (1 input, 1 output)
    pub fn calculate_simple_send_fee(&self) -> Result<u64> {
        self.calculate_fee(1, 1, false)
    }

    /// Calculate fee for send with memo
    pub fn calculate_send_with_memo_fee(&self) -> Result<u64> {
        self.calculate_fee(1, 1, true)
    }

    /// Calculate fee for multi-output send
    pub fn calculate_multi_send_fee(&self, num_outputs: usize, has_memo: bool) -> Result<u64> {
        self.calculate_fee(num_outputs, num_outputs, has_memo)
    }

    /// Estimate maximum fee for given available notes
    pub fn estimate_max_fee(&self, num_available_notes: usize, num_outputs: usize) -> Result<u64> {
        self.calculate_fee(num_available_notes, num_outputs, true)
    }

    /// Validate fee is within acceptable range
    pub fn validate_fee(&self, fee: u64) -> Result<()> {
        if fee < MIN_FEE {
            return Err(Error::FeeTooLow(format!(
                "Fee {} is below minimum {}",
                fee, MIN_FEE
            )));
        }

        if fee > MAX_FEE {
            return Err(Error::FeeTooHigh(format!(
                "Fee {} exceeds maximum {}",
                fee, MAX_FEE
            )));
        }

        Ok(())
    }

    /// Legacy accessor for marginal fee per logical action.
    /// Pirate uses a fixed fee, so this returns DEFAULT_FEE.
    pub fn marginal_fee(&self) -> u64 {
        DEFAULT_FEE
    }
    
    /// Legacy accessor for grace actions (ZIP-317).
    pub fn grace_actions(&self) -> u64 {
        ZIP317_GRACE_ACTIONS
    }
}

impl Default for FeeCalculator {
    fn default() -> Self {
        Self::new()
    }
}

/// Fee policy for dynamic fee adjustment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeePolicy {
    /// Minimum fee (slow confirmation)
    Low,
    /// Standard fee (normal confirmation)
    Standard,
    /// High fee (fast confirmation)
    High,
    /// Custom fee
    Custom(u64),
}

impl FeePolicy {
    /// Get fee multiplier
    pub fn multiplier(&self) -> f64 {
        match self {
            FeePolicy::Low => 0.5,
            FeePolicy::Standard => 1.0,
            FeePolicy::High => 2.0,
            FeePolicy::Custom(_) => 1.0,
        }
    }

    /// Apply policy to base fee
    pub fn apply(&self, base_fee: u64) -> u64 {
        match self {
            FeePolicy::Custom(fee) => *fee,
            _ => ((base_fee as f64) * self.multiplier()) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_send_fee() {
        let calculator = FeeCalculator::new();
        let fee = calculator.calculate_simple_send_fee().unwrap();

        assert_eq!(fee, DEFAULT_FEE);
        assert!(fee >= MIN_FEE);
        assert!(fee <= MAX_FEE);
    }

    #[test]
    fn test_memo_does_not_increase_fee() {
        let calculator = FeeCalculator::new();

        let fee_without_memo = calculator.calculate_fee(1, 1, false).unwrap();
        let fee_with_memo = calculator.calculate_fee(1, 1, true).unwrap();

        // Fees should be identical (fixed fee)
        assert_eq!(fee_with_memo, fee_without_memo);
    }
    
    #[test]
    fn test_fixed_fee_ignores_counts() {
        let calculator = FeeCalculator::new();
        
        let fee_1_1 = calculator.calculate_fee(1, 1, false).unwrap();
        let fee_2_2 = calculator.calculate_fee(2, 2, false).unwrap();
        let fee_1_0 = calculator.calculate_fee(1, 0, false).unwrap();

        assert_eq!(fee_1_1, DEFAULT_FEE);
        assert_eq!(fee_2_2, DEFAULT_FEE);
        assert_eq!(fee_1_0, DEFAULT_FEE);
    }

    #[test]
    fn test_more_outputs_does_not_change_fee() {
        let calculator = FeeCalculator::new();

        let fee_one_output = calculator.calculate_fee(1, 1, false).unwrap();
        let fee_three_outputs = calculator.calculate_fee(1, 3, false).unwrap();

        assert_eq!(fee_three_outputs, fee_one_output);
    }

    #[test]
    fn test_multi_send_fee() {
        let calculator = FeeCalculator::new();

        let fee = calculator.calculate_multi_send_fee(5, false).unwrap();

        assert!(fee >= MIN_FEE);
        assert!(fee <= MAX_FEE);
    }

    #[test]
    fn test_fee_validation() {
        let calculator = FeeCalculator::new();

        assert!(calculator.validate_fee(MIN_FEE).is_ok());
        assert!(calculator.validate_fee(MAX_FEE).is_ok());
        assert!(calculator.validate_fee(MIN_FEE - 1).is_err());
        assert!(calculator.validate_fee(MAX_FEE + 1).is_err());
    }

    #[test]
    fn test_fee_policy() {
        let low = FeePolicy::Low;
        let standard = FeePolicy::Standard;
        let high = FeePolicy::High;

        let base_fee = DEFAULT_FEE;

        assert_eq!(low.apply(base_fee), 5_000);
        assert_eq!(standard.apply(base_fee), DEFAULT_FEE);
        assert_eq!(high.apply(base_fee), 20_000);
    }

    #[test]
    fn test_custom_fee_policy() {
        let custom = FeePolicy::Custom(15_000);
        assert_eq!(custom.apply(10_000), 15_000);
    }

    #[test]
    fn test_estimate_max_fee() {
        let calculator = FeeCalculator::new();

        let max_fee = calculator.estimate_max_fee(10, 5).unwrap();

        assert_eq!(max_fee, DEFAULT_FEE);
    }
}
