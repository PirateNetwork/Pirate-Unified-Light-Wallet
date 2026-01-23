//! Memo handling for Sapling transactions
//!
//! Provides memo validation, encoding, and decoding.
//! Enforces UTF-8 encoding and maximum length limits.

use crate::{Error, Result};
use zcash_primitives::memo::MemoBytes;

/// Maximum memo length in bytes (512 bytes for Sapling)
pub const MAX_MEMO_LENGTH: usize = 512;

/// Maximum memo length for display warning (before actual limit)
pub const MEMO_WARNING_LENGTH: usize = 400;

/// Memo types
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Memo {
    /// Empty memo (no data)
    #[default]
    Empty,
    /// Text memo (UTF-8 string)
    Text(String),
    /// Arbitrary bytes
    Arbitrary(Vec<u8>),
}

impl Memo {
    /// Create text memo with strict UTF-8 validation
    pub fn from_text(text: String) -> Result<Self> {
        if text.is_empty() {
            return Ok(Memo::Empty);
        }

        // Validate UTF-8 (Rust strings are already UTF-8, but check for control chars)
        if !Self::is_valid_memo_text(&text) {
            return Err(Error::InvalidMemo(
                "Memo contains invalid characters (control characters not allowed)".to_string(),
            ));
        }

        // Check byte length (UTF-8 can have multi-byte chars)
        let byte_len = text.len();
        if byte_len > MAX_MEMO_LENGTH {
            return Err(Error::MemoTooLong(format!(
                "Memo is {} bytes, maximum is {} bytes",
                byte_len, MAX_MEMO_LENGTH
            )));
        }

        Ok(Memo::Text(text))
    }

    /// Validate memo text for allowed characters
    /// Allows: printable ASCII, Unicode letters/numbers/symbols, whitespace
    /// Disallows: control characters (except newline, tab)
    fn is_valid_memo_text(text: &str) -> bool {
        text.chars().all(|c| {
            // Allow printable characters, newlines, tabs
            c == '\n' || c == '\t' || c == '\r' ||
            // Allow all non-control characters
            (!c.is_control())
        })
    }

    /// Create text memo, truncating if too long
    pub fn from_text_truncated(text: String) -> Self {
        if text.is_empty() {
            return Memo::Empty;
        }

        // Truncate to fit within MAX_MEMO_LENGTH bytes
        let truncated = Self::truncate_to_bytes(&text, MAX_MEMO_LENGTH);

        // Filter control characters
        let filtered: String = truncated
            .chars()
            .filter(|c| *c == '\n' || *c == '\t' || *c == '\r' || !c.is_control())
            .collect();

        if filtered.is_empty() {
            Memo::Empty
        } else {
            Memo::Text(filtered)
        }
    }

    /// Truncate string to fit within byte limit (respecting UTF-8 boundaries)
    fn truncate_to_bytes(s: &str, max_bytes: usize) -> String {
        if s.len() <= max_bytes {
            return s.to_string();
        }

        let mut result = String::new();
        for c in s.chars() {
            let char_len = c.len_utf8();
            if result.len() + char_len > max_bytes {
                break;
            }
            result.push(c);
        }
        result
    }

    /// Get remaining bytes available in memo
    pub fn remaining_bytes(&self) -> usize {
        MAX_MEMO_LENGTH.saturating_sub(self.byte_len())
    }

    /// Get byte length of memo content
    pub fn byte_len(&self) -> usize {
        match self {
            Memo::Empty => 0,
            Memo::Text(text) => text.len(),
            Memo::Arbitrary(bytes) => bytes.len(),
        }
    }

    /// Check if memo is approaching length limit
    pub fn is_near_limit(&self) -> bool {
        self.byte_len() > MEMO_WARNING_LENGTH
    }

    /// Create from arbitrary bytes
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() {
            return Ok(Memo::Empty);
        }

        if bytes.len() > MAX_MEMO_LENGTH {
            return Err(Error::MemoTooLong(format!(
                "Memo length {} exceeds maximum {}",
                bytes.len(),
                MAX_MEMO_LENGTH
            )));
        }

        Ok(Memo::Arbitrary(bytes))
    }

    /// Encode memo to bytes (padded to 512 bytes)
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = match self {
            Memo::Empty => vec![],
            Memo::Text(text) => text.as_bytes().to_vec(),
            Memo::Arbitrary(data) => data.clone(),
        };

        // Pad to MAX_MEMO_LENGTH
        bytes.resize(MAX_MEMO_LENGTH, 0);
        bytes
    }

    /// Decode memo from bytes
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != MAX_MEMO_LENGTH {
            return Err(Error::InvalidMemo(format!(
                "Memo must be exactly {} bytes",
                MAX_MEMO_LENGTH
            )));
        }

        // Find first null byte
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());

        if end == 0 {
            return Ok(Memo::Empty);
        }

        let data = &bytes[..end];

        // Try to decode as UTF-8 text
        match String::from_utf8(data.to_vec()) {
            Ok(text) => Ok(Memo::Text(text)),
            Err(_) => Ok(Memo::Arbitrary(data.to_vec())),
        }
    }

    /// Check if memo is empty
    pub fn is_empty(&self) -> bool {
        matches!(self, Memo::Empty)
    }

    /// Get as string (if text memo)
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Memo::Text(text) => Some(text),
            _ => None,
        }
    }

    /// Get byte length
    pub fn len(&self) -> usize {
        match self {
            Memo::Empty => 0,
            Memo::Text(text) => text.len(),
            Memo::Arbitrary(bytes) => bytes.len(),
        }
    }

    /// Validate memo
    pub fn validate(&self) -> Result<()> {
        if self.len() > MAX_MEMO_LENGTH {
            return Err(Error::MemoTooLong(format!(
                "Memo length {} exceeds maximum {}",
                self.len(),
                MAX_MEMO_LENGTH
            )));
        }
        Ok(())
    }

    /// Convert to Sapling `MemoBytes` for transaction outputs.
    pub fn to_memo_bytes(&self) -> Result<MemoBytes> {
        match self {
            Memo::Empty => Ok(MemoBytes::empty()),
            Memo::Text(text) => MemoBytes::from_bytes(text.as_bytes())
                .map_err(|e| Error::InvalidMemo(format!("Invalid memo: {:?}", e))),
            Memo::Arbitrary(bytes) => MemoBytes::from_bytes(bytes)
                .map_err(|e| Error::InvalidMemo(format!("Invalid memo bytes: {:?}", e))),
        }
    }
}

impl From<String> for Memo {
    fn from(text: String) -> Self {
        Memo::from_text(text).unwrap_or(Memo::Empty)
    }
}

impl From<&str> for Memo {
    fn from(text: &str) -> Self {
        Memo::from_text(text.to_string()).unwrap_or(Memo::Empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_memo() {
        let memo = Memo::Empty;
        assert!(memo.is_empty());
        assert_eq!(memo.len(), 0);

        let encoded = memo.encode();
        assert_eq!(encoded.len(), MAX_MEMO_LENGTH);
    }

    #[test]
    fn test_text_memo() {
        let text = "Hello, Pirate Chain!".to_string();
        let memo = Memo::from_text(text.clone()).unwrap();

        assert!(!memo.is_empty());
        assert_eq!(memo.as_str(), Some(text.as_str()));
        assert_eq!(memo.len(), text.len());
    }

    #[test]
    fn test_memo_too_long() {
        let long_text = "A".repeat(MAX_MEMO_LENGTH + 1);
        let result = Memo::from_text(long_text);

        assert!(result.is_err());
    }

    #[test]
    fn test_memo_encode_decode() {
        let original = Memo::from_text("Test message".to_string()).unwrap();
        let encoded = original.encode();
        let decoded = Memo::decode(&encoded).unwrap();

        assert_eq!(decoded.as_str(), Some("Test message"));
    }

    #[test]
    fn test_empty_memo_encode_decode() {
        let original = Memo::Empty;
        let encoded = original.encode();
        let decoded = Memo::decode(&encoded).unwrap();

        assert!(decoded.is_empty());
    }

    #[test]
    fn test_memo_from_string() {
        let memo: Memo = "Hello".to_string().into();
        assert_eq!(memo.as_str(), Some("Hello"));

        let memo: Memo = "World".into();
        assert_eq!(memo.as_str(), Some("World"));
    }

    #[test]
    fn test_arbitrary_bytes() {
        let bytes = vec![1, 2, 3, 4, 5];
        let memo = Memo::from_bytes(bytes.clone()).unwrap();

        match memo {
            Memo::Arbitrary(data) => assert_eq!(data, bytes),
            _ => panic!("Expected Arbitrary memo"),
        }
    }

    #[test]
    fn test_memo_validation() {
        let valid = Memo::from_text("Valid memo".to_string()).unwrap();
        assert!(valid.validate().is_ok());

        let empty = Memo::Empty;
        assert!(empty.validate().is_ok());
    }
}
