//! Diversifier rotation service for generating unlinkable Sapling addresses
//!
//! Prevents address reuse by default for maximum privacy.

use crate::Result;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};

/// Maximum diversifier index (11 bytes = 2^88, but we use u32 for practicality)
pub const MAX_DIVERSIFIER_INDEX: u32 = u32::MAX;

/// Default gap limit for address scanning
pub const DEFAULT_GAP_LIMIT: u32 = 20;

/// Diversifier index for Sapling address derivation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DiversifierIndex(u32);

impl DiversifierIndex {
    /// Create new diversifier index
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Create from u32
    pub const fn from_u32(index: u32) -> Self {
        Self(index)
    }

    /// Get inner value as u32
    pub const fn as_u32(&self) -> u32 {
        self.0
    }

    /// Get next diversifier index
    pub fn next(&self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Get previous diversifier index (saturating at 0)
    pub fn prev(&self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    /// Increment by amount
    pub fn increment(&self, amount: u32) -> Self {
        Self(self.0.saturating_add(amount))
    }

    /// Check if at maximum
    pub fn is_max(&self) -> bool {
        self.0 == MAX_DIVERSIFIER_INDEX
    }

    /// Serialize to bytes (little-endian)
    pub fn to_bytes(&self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    /// Deserialize from bytes (little-endian)
    pub fn from_bytes(bytes: [u8; 4]) -> Self {
        Self(u32::from_le_bytes(bytes))
    }
}

impl Default for DiversifierIndex {
    fn default() -> Self {
        Self(0)
    }
}

impl From<u32> for DiversifierIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<DiversifierIndex> for u32 {
    fn from(value: DiversifierIndex) -> Self {
        value.0
    }
}

impl std::fmt::Display for DiversifierIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Address usage tracking for preventing reuse
#[derive(Debug, Clone)]
pub struct AddressUsage {
    /// Diversifier index
    pub index: DiversifierIndex,
    /// Number of times shared (given out)
    pub share_count: u32,
    /// Whether address has received funds
    pub has_received: bool,
    /// Whether address has been used in a spend
    pub has_spent: bool,
    /// Last shared timestamp
    pub last_shared: Option<chrono::DateTime<chrono::Utc>>,
    /// First receive timestamp
    pub first_receive: Option<chrono::DateTime<chrono::Utc>>,
    /// Label
    pub label: Option<String>,
}

impl AddressUsage {
    /// Create new usage tracking
    pub fn new(index: DiversifierIndex) -> Self {
        Self {
            index,
            share_count: 0,
            has_received: false,
            has_spent: false,
            last_shared: None,
            first_receive: None,
            label: None,
        }
    }

    /// Mark as shared (given out)
    pub fn mark_shared(&mut self) {
        self.share_count += 1;
        self.last_shared = Some(chrono::Utc::now());
    }

    /// Mark as having received funds
    pub fn mark_received(&mut self) {
        if !self.has_received {
            self.has_received = true;
            self.first_receive = Some(chrono::Utc::now());
        }
    }

    /// Mark as having been spent from
    pub fn mark_spent(&mut self) {
        self.has_spent = true;
    }

    /// Check if address should be avoided for new payments
    /// (already used or shared multiple times)
    pub fn should_avoid(&self) -> bool {
        self.has_received || self.has_spent || self.share_count > 1
    }

    /// Check if address is virgin (never shared or used)
    pub fn is_virgin(&self) -> bool {
        self.share_count == 0 && !self.has_received && !self.has_spent
    }
}

/// Policy for address rotation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationPolicy {
    /// Always use fresh address (maximum privacy)
    AlwaysFresh,
    /// Reuse current address until it receives funds
    ReuseUntilReceived,
    /// Reuse current address until shared N times
    ReuseUntilShared(u32),
    /// Manual rotation only (user controls)
    Manual,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        // Default to maximum privacy
        RotationPolicy::AlwaysFresh
    }
}

/// Diversifier rotation service
/// 
/// Manages address derivation with diversifier rotation to prevent address reuse.
/// Tracks usage of each address and provides fresh addresses by default.
pub struct DiversifierRotationService {
    /// Current diversifier index (next to use)
    current_index: AtomicU32,
    /// Highest diversifier index ever used
    highest_used: AtomicU32,
    /// Set of used indices (for gap detection)
    used_indices: HashSet<DiversifierIndex>,
    /// Usage tracking per index
    usage: std::collections::HashMap<DiversifierIndex, AddressUsage>,
    /// Rotation policy
    policy: RotationPolicy,
    /// Gap limit for address scanning
    gap_limit: u32,
}

impl DiversifierRotationService {
    /// Create new rotation service
    pub fn new(policy: RotationPolicy) -> Self {
        Self {
            current_index: AtomicU32::new(0),
            highest_used: AtomicU32::new(0),
            used_indices: HashSet::new(),
            usage: std::collections::HashMap::new(),
            policy,
            gap_limit: DEFAULT_GAP_LIMIT,
        }
    }

    /// Create with custom gap limit
    pub fn with_gap_limit(mut self, gap_limit: u32) -> Self {
        self.gap_limit = gap_limit;
        self
    }

    /// Restore from persisted state
    pub fn restore(
        current_index: u32,
        highest_used: u32,
        used_indices: Vec<u32>,
        policy: RotationPolicy,
    ) -> Self {
        let mut service = Self::new(policy);
        service.current_index.store(current_index, Ordering::SeqCst);
        service.highest_used.store(highest_used, Ordering::SeqCst);
        service.used_indices = used_indices.into_iter().map(DiversifierIndex).collect();
        service
    }

    /// Get next fresh diversifier index (prevents reuse)
    pub fn next_fresh_index(&self) -> DiversifierIndex {
        loop {
            let current = self.current_index.fetch_add(1, Ordering::SeqCst);
            let index = DiversifierIndex::new(current);

            // Check if already used based on policy
            if let Some(usage) = self.usage.get(&index) {
                if usage.should_avoid() {
                    continue; // Skip to next
                }
            }

            // Update highest used if needed
            self.highest_used.fetch_max(current, Ordering::SeqCst);

            return index;
        }
    }

    /// Get current diversifier index without advancing
    pub fn current_index(&self) -> DiversifierIndex {
        DiversifierIndex::new(self.current_index.load(Ordering::SeqCst))
    }

    /// Get highest used diversifier index
    pub fn highest_used_index(&self) -> DiversifierIndex {
        DiversifierIndex::new(self.highest_used.load(Ordering::SeqCst))
    }

    /// Request new address based on rotation policy
    pub fn request_address(&mut self) -> Result<DiversifierIndex> {
        let index = match self.policy {
            RotationPolicy::AlwaysFresh => {
                // Always get fresh index
                self.next_fresh_index()
            }
            RotationPolicy::ReuseUntilReceived => {
                let current = self.current_index();
                if let Some(usage) = self.usage.get(&current) {
                    if usage.has_received {
                        self.next_fresh_index()
                    } else {
                        current
                    }
                } else {
                    current
                }
            }
            RotationPolicy::ReuseUntilShared(max_shares) => {
                let current = self.current_index();
                if let Some(usage) = self.usage.get(&current) {
                    if usage.share_count >= max_shares || usage.has_received {
                        self.next_fresh_index()
                    } else {
                        current
                    }
                } else {
                    current
                }
            }
            RotationPolicy::Manual => {
                // Just return current, user controls rotation
                self.current_index()
            }
        };

        // Track usage
        self.mark_shared(index);

        Ok(index)
    }

    /// Mark diversifier as shared (address given out)
    pub fn mark_shared(&mut self, index: DiversifierIndex) {
        self.used_indices.insert(index);
        self.usage
            .entry(index)
            .or_insert_with(|| AddressUsage::new(index))
            .mark_shared();
    }

    /// Mark diversifier as having received funds
    pub fn mark_received(&mut self, index: DiversifierIndex) {
        self.used_indices.insert(index);
        self.usage
            .entry(index)
            .or_insert_with(|| AddressUsage::new(index))
            .mark_received();
    }

    /// Mark diversifier as having been spent from
    pub fn mark_spent(&mut self, index: DiversifierIndex) {
        self.used_indices.insert(index);
        self.usage
            .entry(index)
            .or_insert_with(|| AddressUsage::new(index))
            .mark_spent();
    }

    /// Get usage info for diversifier
    pub fn get_usage(&self, index: DiversifierIndex) -> Option<&AddressUsage> {
        self.usage.get(&index)
    }

    /// Set label for diversifier
    pub fn set_label(&mut self, index: DiversifierIndex, label: String) {
        self.usage
            .entry(index)
            .or_insert_with(|| AddressUsage::new(index))
            .label = Some(label);
    }

    /// Get label for diversifier
    pub fn get_label(&self, index: DiversifierIndex) -> Option<&String> {
        self.usage.get(&index).and_then(|u| u.label.as_ref())
    }

    /// Force advance to specific index
    pub fn advance_to(&self, index: DiversifierIndex) {
        self.current_index.store(index.as_u32(), Ordering::SeqCst);
        self.highest_used.fetch_max(index.as_u32(), Ordering::SeqCst);
    }

    /// Reset to beginning (dangerous, only for testing/recovery)
    pub fn reset(&mut self) {
        self.current_index.store(0, Ordering::SeqCst);
        self.highest_used.store(0, Ordering::SeqCst);
        self.used_indices.clear();
        self.usage.clear();
    }

    /// Get all used indices
    pub fn used_indices(&self) -> Vec<DiversifierIndex> {
        let mut indices: Vec<_> = self.used_indices.iter().copied().collect();
        indices.sort();
        indices
    }

    /// Get gap between current and highest used
    pub fn current_gap(&self) -> u32 {
        let current = self.current_index.load(Ordering::SeqCst);
        let highest = self.highest_used.load(Ordering::SeqCst);
        current.saturating_sub(highest)
    }

    /// Check if gap limit exceeded
    pub fn is_gap_exceeded(&self) -> bool {
        self.current_gap() > self.gap_limit
    }

    /// Get policy
    pub fn policy(&self) -> RotationPolicy {
        self.policy
    }

    /// Set policy
    pub fn set_policy(&mut self, policy: RotationPolicy) {
        self.policy = policy;
    }

    /// Get state for persistence
    pub fn get_state(&self) -> DiversifierState {
        DiversifierState {
            current_index: self.current_index.load(Ordering::SeqCst),
            highest_used: self.highest_used.load(Ordering::SeqCst),
            used_indices: self.used_indices.iter().map(|i| i.as_u32()).collect(),
            policy: self.policy,
        }
    }
}

impl Default for DiversifierRotationService {
    fn default() -> Self {
        Self::new(RotationPolicy::default())
    }
}

/// Serializable diversifier state
#[derive(Debug, Clone)]
pub struct DiversifierState {
    /// Current diversifier index
    pub current_index: u32,
    /// Highest used index
    pub highest_used: u32,
    /// All used indices
    pub used_indices: Vec<u32>,
    /// Rotation policy
    pub policy: RotationPolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diversifier_index() {
        let idx = DiversifierIndex::new(0);
        assert_eq!(idx.as_u32(), 0);

        let next = idx.next();
        assert_eq!(next.as_u32(), 1);

        let prev = next.prev();
        assert_eq!(prev.as_u32(), 0);

        // Saturating at 0
        let zero = DiversifierIndex::new(0);
        assert_eq!(zero.prev().as_u32(), 0);
    }

    #[test]
    fn test_always_fresh_policy() {
        let mut service = DiversifierRotationService::new(RotationPolicy::AlwaysFresh);

        let idx1 = service.request_address().unwrap();
        let idx2 = service.request_address().unwrap();
        let idx3 = service.request_address().unwrap();

        // Should always get different indices
        assert_ne!(idx1, idx2);
        assert_ne!(idx2, idx3);
        assert_ne!(idx1, idx3);
    }

    #[test]
    fn test_reuse_until_received_policy() {
        let mut service = DiversifierRotationService::new(RotationPolicy::ReuseUntilReceived);

        let idx1 = service.request_address().unwrap();
        let idx2 = service.request_address().unwrap();

        // Should reuse same index until received
        assert_eq!(idx1, idx2);

        // Mark as received
        service.mark_received(idx1);

        // Now should get new index
        let idx3 = service.request_address().unwrap();
        assert_ne!(idx1, idx3);
    }

    #[test]
    fn test_reuse_until_shared_policy() {
        let mut service = DiversifierRotationService::new(RotationPolicy::ReuseUntilShared(3));

        let idx1 = service.request_address().unwrap();
        let idx2 = service.request_address().unwrap();
        let idx3 = service.request_address().unwrap();

        // Should reuse for first 3 shares
        assert_eq!(idx1, idx2);
        assert_eq!(idx2, idx3);

        // Fourth request should get new index
        let idx4 = service.request_address().unwrap();
        assert_ne!(idx3, idx4);
    }

    #[test]
    fn test_manual_policy() {
        let mut service = DiversifierRotationService::new(RotationPolicy::Manual);

        let idx1 = service.request_address().unwrap();
        let idx2 = service.request_address().unwrap();

        // Should always return same index in manual mode
        assert_eq!(idx1, idx2);

        // Manually advance
        service.advance_to(DiversifierIndex::new(10));

        let idx3 = service.request_address().unwrap();
        assert_eq!(idx3.as_u32(), 10);
    }

    #[test]
    fn test_usage_tracking() {
        let mut service = DiversifierRotationService::new(RotationPolicy::AlwaysFresh);

        let idx = service.request_address().unwrap();

        let usage = service.get_usage(idx).unwrap();
        assert_eq!(usage.share_count, 1);
        assert!(!usage.has_received);

        service.mark_received(idx);

        let usage = service.get_usage(idx).unwrap();
        assert!(usage.has_received);
    }

    #[test]
    fn test_label_management() {
        let mut service = DiversifierRotationService::new(RotationPolicy::AlwaysFresh);

        let idx = service.request_address().unwrap();
        service.set_label(idx, "My Address".to_string());

        assert_eq!(service.get_label(idx), Some(&"My Address".to_string()));
    }

    #[test]
    fn test_state_persistence() {
        let mut service = DiversifierRotationService::new(RotationPolicy::AlwaysFresh);

        // Generate some addresses
        for _ in 0..5 {
            service.request_address().unwrap();
        }

        let state = service.get_state();
        assert_eq!(state.current_index, 5);
        assert_eq!(state.used_indices.len(), 5);

        // Restore
        let restored = DiversifierRotationService::restore(
            state.current_index,
            state.highest_used,
            state.used_indices,
            state.policy,
        );

        assert_eq!(restored.current_index().as_u32(), 5);
    }
}

