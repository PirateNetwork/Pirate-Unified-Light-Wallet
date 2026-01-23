//! Error types for Pirate Core
//!
//! Comprehensive error taxonomy for transaction building and wallet operations.

use std::fmt;

/// Result type
pub type Result<T> = std::result::Result<T, Error>;

/// Pirate Core errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Insufficient funds for transaction
    #[error("Insufficient funds: {0}")]
    InsufficientFunds(String),

    /// Invalid address format
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// Memo too long
    #[error("Memo too long: {0}")]
    MemoTooLong(String),

    /// Invalid memo
    #[error("Invalid memo: {0}")]
    InvalidMemo(String),

    /// Fee too low
    #[error("Fee too low: {0}")]
    FeeTooLow(String),

    /// Fee too high
    #[error("Fee too high: {0}")]
    FeeTooHigh(String),

    /// Fee calculation error
    #[error("Fee calculation error: {0}")]
    FeeCalculation(String),

    /// Transaction building error
    #[error("Transaction build error: {0}")]
    TransactionBuild(String),

    /// Transaction signing error
    #[error("Transaction signing error: {0}")]
    TransactionSigning(String),

    /// Transaction broadcast error
    #[error("Transaction broadcast error: {0}")]
    TransactionBroadcast(String),

    /// Invalid transaction
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    /// Key derivation error
    #[error("Key derivation error: {0}")]
    KeyDerivation(String),

    /// Invalid key
    #[error("Invalid key: {0}")]
    InvalidKey(String),

    /// Note decryption error
    #[error("Note decryption error: {0}")]
    NoteDecryption(String),

    /// Invalid note
    #[error("Invalid note: {0}")]
    InvalidNote(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Network is down/unavailable
    #[error("Network unavailable: {0}")]
    NetworkDown(String),

    /// Transaction broadcast failed
    #[error("Broadcast failed: {0}")]
    BroadcastFailed(String),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Sync error
    #[error("Sync error: {0}")]
    Sync(String),

    /// Invalid amount
    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    /// Amount overflow
    #[error("Amount overflow: {0}")]
    AmountOverflow(String),

    /// Invalid diversifier index
    #[error("Invalid diversifier index: {0}")]
    InvalidDiversifierIndex(String),

    /// Wallet not found
    #[error("Wallet not found: {0}")]
    WalletNotFound(String),

    /// Wallet already exists
    #[error("Wallet already exists: {0}")]
    WalletAlreadyExists(String),

    /// Invalid mnemonic
    #[error("Invalid mnemonic: {0}")]
    InvalidMnemonic(String),

    /// Invalid seed
    #[error("Invalid seed: {0}")]
    InvalidSeed(String),

    /// Rescan error
    #[error("Rescan error: {0}")]
    Rescan(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Check if error is a user-facing error (vs internal error)
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            Error::InsufficientFunds(_)
                | Error::InvalidAddress(_)
                | Error::MemoTooLong(_)
                | Error::FeeTooLow(_)
                | Error::FeeTooHigh(_)
                | Error::InvalidAmount(_)
                | Error::InvalidMnemonic(_)
                | Error::NetworkDown(_)
                | Error::BroadcastFailed(_)
        )
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            Error::InsufficientFunds(_) => {
                "You don't have enough funds for this transaction. Please check your balance and try again.".to_string()
            }
            Error::InvalidAddress(_) => {
                "The recipient address is invalid. Please check and try again.".to_string()
            }
            Error::MemoTooLong(_) => {
                format!("Your memo is too long. Maximum length is {} characters.", crate::memo::MAX_MEMO_LENGTH)
            }
            Error::FeeTooLow(_) => {
                "The transaction fee is too low. Please increase the fee.".to_string()
            }
            Error::FeeTooHigh(_) => {
                "The transaction fee seems unusually high. Please review.".to_string()
            }
            Error::InvalidAmount(_) => {
                "The amount is invalid. Please enter a valid amount.".to_string()
            }
            Error::InvalidMnemonic(_) => {
                "The recovery phrase is invalid. Please check and try again.".to_string()
            }
            Error::NetworkDown(_) => {
                "Unable to connect to the network. Please check your connection and try again.".to_string()
            }
            Error::BroadcastFailed(_) => {
                "Failed to broadcast transaction. The network may be congested. Please try again.".to_string()
            }
            _ => self.to_string(),
        }
    }

    /// Get error category for logging/metrics
    pub fn category(&self) -> ErrorCategory {
        match self {
            Error::InsufficientFunds(_) | Error::InvalidAmount(_) | Error::AmountOverflow(_) => {
                ErrorCategory::Amount
            }
            Error::InvalidAddress(_) => ErrorCategory::Address,
            Error::MemoTooLong(_) | Error::InvalidMemo(_) => ErrorCategory::Memo,
            Error::FeeTooLow(_) | Error::FeeTooHigh(_) | Error::FeeCalculation(_) => {
                ErrorCategory::Fee
            }
            Error::TransactionBuild(_)
            | Error::TransactionSigning(_)
            | Error::TransactionBroadcast(_)
            | Error::BroadcastFailed(_)
            | Error::InvalidTransaction(_) => ErrorCategory::Transaction,
            Error::KeyDerivation(_) | Error::InvalidKey(_) => ErrorCategory::Keys,
            Error::NoteDecryption(_) | Error::InvalidNote(_) => ErrorCategory::Notes,
            Error::Network(_) | Error::NetworkDown(_) => ErrorCategory::Network,
            Error::Storage(_) => ErrorCategory::Storage,
            Error::Sync(_) | Error::Rescan(_) => ErrorCategory::Sync,
            Error::WalletNotFound(_) | Error::WalletAlreadyExists(_) => ErrorCategory::Wallet,
            Error::InvalidMnemonic(_) | Error::InvalidSeed(_) => ErrorCategory::Wallet,
            Error::InvalidDiversifierIndex(_) => ErrorCategory::Address,
            Error::Io(_) | Error::Serialization(_) | Error::Other(_) => ErrorCategory::Internal,
        }
    }
}

/// Error categories for classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Amount-related errors
    Amount,
    /// Address-related errors
    Address,
    /// Memo-related errors
    Memo,
    /// Fee-related errors
    Fee,
    /// Transaction-related errors
    Transaction,
    /// Key-related errors
    Keys,
    /// Note-related errors
    Notes,
    /// Network-related errors
    Network,
    /// Storage-related errors
    Storage,
    /// Sync-related errors
    Sync,
    /// Wallet-related errors
    Wallet,
    /// Internal/system errors
    Internal,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::Amount => write!(f, "Amount"),
            ErrorCategory::Address => write!(f, "Address"),
            ErrorCategory::Memo => write!(f, "Memo"),
            ErrorCategory::Fee => write!(f, "Fee"),
            ErrorCategory::Transaction => write!(f, "Transaction"),
            ErrorCategory::Keys => write!(f, "Keys"),
            ErrorCategory::Notes => write!(f, "Notes"),
            ErrorCategory::Network => write!(f, "Network"),
            ErrorCategory::Storage => write!(f, "Storage"),
            ErrorCategory::Sync => write!(f, "Sync"),
            ErrorCategory::Wallet => write!(f, "Wallet"),
            ErrorCategory::Internal => write!(f, "Internal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_error_detection() {
        assert!(Error::InsufficientFunds("test".to_string()).is_user_error());
        assert!(Error::InvalidAddress("test".to_string()).is_user_error());
        assert!(!Error::Network("test".to_string()).is_user_error());
        assert!(!Error::Storage("test".to_string()).is_user_error());
    }

    #[test]
    fn test_user_messages() {
        let error = Error::InsufficientFunds("details".to_string());
        let msg = error.user_message();
        assert!(msg.contains("don't have enough funds"));

        let error = Error::InvalidAddress("details".to_string());
        let msg = error.user_message();
        assert!(msg.contains("address is invalid"));
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(
            Error::InsufficientFunds("test".to_string()).category(),
            ErrorCategory::Amount
        );
        assert_eq!(
            Error::InvalidAddress("test".to_string()).category(),
            ErrorCategory::Address
        );
        assert_eq!(
            Error::MemoTooLong("test".to_string()).category(),
            ErrorCategory::Memo
        );
        assert_eq!(
            Error::FeeTooLow("test".to_string()).category(),
            ErrorCategory::Fee
        );
        assert_eq!(
            Error::Network("test".to_string()).category(),
            ErrorCategory::Network
        );
    }

    #[test]
    fn test_category_display() {
        assert_eq!(ErrorCategory::Amount.to_string(), "Amount");
        assert_eq!(ErrorCategory::Transaction.to_string(), "Transaction");
        assert_eq!(ErrorCategory::Network.to_string(), "Network");
    }
}
