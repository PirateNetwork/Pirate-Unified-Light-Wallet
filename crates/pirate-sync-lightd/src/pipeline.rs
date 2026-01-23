//! Sync Pipeline — Streaming compact block processing with batched trial decryption
//!
//! Pipeline stages:
//! 1. Stream compact blocks (gRPC) in ranges (2k blocks per batch)
//! 2. Batched trial decryption in bounded thread pool
//! 3. Lazy memo decode (only when displaying)
//! 4. Apply note commitments to SaplingFrontier
//! 5. Mini-checkpoint every N batches (N=5) to frontier_snapshots
//!
//! Performance counters:
//! - blocks_per_second
//! - notes_decrypted
//! - last_batch_ms

use crate::client::{CompactBlockData, LightClient};
use crate::frontier::SaplingFrontier;
use crate::sapling::trial_decrypt::try_decrypt_compact_output;
use crate::{Error, Result};
use pirate_storage_sqlite::models::AddressScope;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};

/// Default batch size for block streaming (2k blocks)
pub const PIPELINE_BATCH_SIZE: u64 = 2_000;

/// Number of batches between mini-checkpoints
pub const MINI_CHECKPOINT_INTERVAL: u32 = 5;

/// Maximum concurrent trial decryption tasks
pub const MAX_DECRYPT_CONCURRENCY: usize = 8;

/// Channel buffer size for block streaming
pub const BLOCK_CHANNEL_BUFFER: usize = 4;

/// Performance counters for sync progress
#[derive(Debug, Default)]
pub struct PerfCounters {
    /// Total blocks processed
    pub blocks_processed: AtomicU64,
    /// Total notes decrypted (matched wallet)
    pub notes_decrypted: AtomicU64,
    /// Total note commitments applied to frontier
    pub commitments_applied: AtomicU64,
    /// Last batch processing time in milliseconds
    pub last_batch_ms: AtomicU64,
    /// Total processing time in milliseconds
    pub total_time_ms: AtomicU64,
    /// Number of batches processed
    pub batches_processed: AtomicU64,
}

impl PerfCounters {
    /// Create new perf counters
    pub fn new() -> Self {
        Self::default()
    }

    /// Get blocks per second
    pub fn blocks_per_second(&self) -> f64 {
        let blocks = self.blocks_processed.load(Ordering::Relaxed);
        let time_ms = self.total_time_ms.load(Ordering::Relaxed);
        if time_ms == 0 {
            return 0.0;
        }
        (blocks as f64) / (time_ms as f64 / 1000.0)
    }

    /// Get average batch time in ms
    pub fn avg_batch_ms(&self) -> u64 {
        let batches = self.batches_processed.load(Ordering::Relaxed);
        let time_ms = self.total_time_ms.load(Ordering::Relaxed);
        if batches == 0 {
            return 0;
        }
        time_ms / batches
    }

    /// Record batch completion
    pub fn record_batch(&self, blocks: u64, notes: u64, commitments: u64, duration_ms: u64) {
        self.blocks_processed.fetch_add(blocks, Ordering::Relaxed);
        self.notes_decrypted.fetch_add(notes, Ordering::Relaxed);
        self.commitments_applied
            .fetch_add(commitments, Ordering::Relaxed);
        self.last_batch_ms.store(duration_ms, Ordering::Relaxed);
        self.total_time_ms.fetch_add(duration_ms, Ordering::Relaxed);
        self.batches_processed.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot of counters
    pub fn snapshot(&self) -> PerfSnapshot {
        PerfSnapshot {
            blocks_processed: self.blocks_processed.load(Ordering::Relaxed),
            notes_decrypted: self.notes_decrypted.load(Ordering::Relaxed),
            commitments_applied: self.commitments_applied.load(Ordering::Relaxed),
            last_batch_ms: self.last_batch_ms.load(Ordering::Relaxed),
            blocks_per_second: self.blocks_per_second(),
            avg_batch_ms: self.avg_batch_ms(),
        }
    }

    /// Reset counters
    pub fn reset(&self) {
        self.blocks_processed.store(0, Ordering::Relaxed);
        self.notes_decrypted.store(0, Ordering::Relaxed);
        self.commitments_applied.store(0, Ordering::Relaxed);
        self.last_batch_ms.store(0, Ordering::Relaxed);
        self.total_time_ms.store(0, Ordering::Relaxed);
        self.batches_processed.store(0, Ordering::Relaxed);
    }
}

/// Snapshot of performance counters
#[derive(Debug, Clone)]
pub struct PerfSnapshot {
    /// Total blocks processed
    pub blocks_processed: u64,
    /// Total notes decrypted
    pub notes_decrypted: u64,
    /// Total commitments applied
    pub commitments_applied: u64,
    /// Last batch time in ms
    pub last_batch_ms: u64,
    /// Blocks per second
    pub blocks_per_second: f64,
    /// Average batch time in ms
    pub avg_batch_ms: u64,
}

/// Note type (Sapling or Orchard)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteType {
    /// Sapling note
    Sapling,
    /// Orchard note
    Orchard,
}

/// Decrypted note with lazy memo
#[derive(Debug, Clone)]
pub struct DecryptedNote {
    /// Note type (Sapling or Orchard)
    pub note_type: NoteType,
    /// Block height
    pub height: u64,
    /// Transaction index in block
    pub tx_index: usize,
    /// Output index in transaction
    pub output_index: usize,
    /// Note value in arrrtoshis
    pub value: u64,
    /// Note commitment
    pub commitment: [u8; 32],
    /// Nullifier for spending
    pub nullifier: [u8; 32],
    /// Encrypted memo (lazy decode - only decrypt when needed)
    pub encrypted_memo: Vec<u8>,
    /// Cached decrypted memo (None until decoded)
    memo_cache: Option<String>,
    /// Transaction hash (for memo fetching and storage)
    pub tx_hash: Vec<u8>,
    /// Transaction ID (same as tx_hash, for storage compatibility)
    pub txid: Vec<u8>,
    /// Key group id that matched this note (if known)
    pub key_id: Option<i64>,
    /// Address scope (external receive or internal change)
    pub address_scope: AddressScope,
    /// Diversifier bytes (Sapling/Orchard)
    pub diversifier: Vec<u8>,
    /// Sapling note leadbyte (v1/v2) for nullifier derivation
    pub sapling_rseed_leadbyte: Option<u8>,
    /// Sapling note rseed bytes (32 bytes) for nullifier derivation
    pub sapling_rseed: Option<[u8; 32]>,
    /// Serialized Sapling merkle path bytes
    pub merkle_path: Vec<u8>,
    /// Serialized note bytes (for storage, Sapling/Orchard)
    pub note_bytes: Vec<u8>,
    /// Anchor (Orchard commitment tree root, Orchard only)
    pub anchor: Option<[u8; 32]>,
    /// Position in note commitment tree (Sapling/Orchard)
    pub position: Option<u64>,
    /// Orchard note rho bytes (for nullifier derivation)
    pub orchard_rho: Option<[u8; 32]>,
    /// Orchard note rseed bytes (for nullifier derivation)
    pub orchard_rseed: Option<[u8; 32]>,
}

impl DecryptedNote {
    /// Create new decrypted note with lazy memo (defaults to Sapling)
    pub fn new(
        height: u64,
        tx_index: usize,
        output_index: usize,
        value: u64,
        commitment: [u8; 32],
        nullifier: [u8; 32],
        encrypted_memo: Vec<u8>,
    ) -> Self {
        Self {
            note_type: NoteType::Sapling,
            height,
            tx_index,
            output_index,
            value,
            commitment,
            nullifier,
            encrypted_memo,
            memo_cache: None,
            tx_hash: Vec::new(),
            txid: Vec::new(),
            key_id: None,
            address_scope: AddressScope::External,
            diversifier: Vec::new(),
            sapling_rseed_leadbyte: None,
            sapling_rseed: None,
            merkle_path: Vec::new(),
            note_bytes: Vec::new(),
            anchor: None,
            position: None,
            orchard_rho: None,
            orchard_rseed: None,
        }
    }

    /// Create new Orchard decrypted note
    #[allow(clippy::too_many_arguments)]
    pub fn new_orchard(
        height: u64,
        tx_index: usize,
        output_index: usize,
        value: u64,
        commitment: [u8; 32],
        nullifier: [u8; 32],
        encrypted_memo: Vec<u8>,
        anchor: Option<[u8; 32]>,
        position: Option<u64>,
    ) -> Self {
        Self {
            note_type: NoteType::Orchard,
            height,
            tx_index,
            output_index,
            value,
            commitment,
            nullifier,
            encrypted_memo,
            memo_cache: None,
            tx_hash: Vec::new(),
            txid: Vec::new(),
            key_id: None,
            address_scope: AddressScope::External,
            diversifier: Vec::new(),
            sapling_rseed_leadbyte: None,
            sapling_rseed: None,
            merkle_path: Vec::new(),
            note_bytes: Vec::new(),
            anchor,
            position,
            orchard_rho: None,
            orchard_rseed: None,
        }
    }

    /// Set transaction hash and ID
    pub fn set_tx_hash(&mut self, tx_hash: Vec<u8>) {
        self.tx_hash = tx_hash.clone();
        self.txid = tx_hash;
    }

    /// Set memo bytes (encrypted memo)
    pub fn set_memo_bytes(&mut self, memo_bytes: Vec<u8>) {
        self.encrypted_memo = memo_bytes;
        self.memo_cache = None; // Clear cache when memo is updated
    }

    /// Get memo bytes (for compatibility with sync.rs)
    pub fn memo_bytes(&self) -> Option<&[u8]> {
        if self.encrypted_memo.is_empty() {
            None
        } else {
            Some(&self.encrypted_memo)
        }
    }

    /// Get memo (lazy decode on first access)
    pub fn memo(&mut self) -> Option<&str> {
        if self.memo_cache.is_none() && !self.encrypted_memo.is_empty() {
            // Decode memo only when accessed
            self.memo_cache = decode_memo(&self.encrypted_memo);
        }
        self.memo_cache.as_deref()
    }

    /// Check if memo is already decoded
    pub fn is_memo_decoded(&self) -> bool {
        self.memo_cache.is_some()
    }
}

/// Decode memo bytes to UTF-8 string (512-byte field with null padding).
///
/// Note: This expects **already-decrypted memo bytes** (from full transaction fetch),
/// not ciphertext. Compact trial decryption does not provide memo bytes.
fn decode_memo(memo_bytes: &[u8]) -> Option<String> {
    if memo_bytes.is_empty() {
        return None;
    }

    let memo_bytes = if memo_bytes.len() > 512 {
        &memo_bytes[..512]
    } else {
        memo_bytes
    };

    // If first byte > 0xF4, this isn't a text memo.
    if memo_bytes[0] > 0xF4 {
        return None;
    }

    let trimmed: Vec<u8> = memo_bytes.iter().copied().take_while(|&b| b != 0).collect();

    if trimmed.is_empty() {
        return None;
    }

    String::from_utf8(trimmed).ok()
}

/// Batch processing result
#[derive(Debug)]
pub struct BatchResult {
    /// Start height of batch
    pub start_height: u64,
    /// End height of batch
    pub end_height: u64,
    /// Decrypted notes found
    pub notes: Vec<DecryptedNote>,
    /// Note commitments to apply
    pub commitments: Vec<[u8; 32]>,
    /// Processing duration
    pub duration: Duration,
}

/// Mini-checkpoint callback
pub type MiniCheckpointCallback = Arc<dyn Fn(u32, &[u8]) -> Result<()> + Send + Sync>;

/// Pipeline configuration
#[derive(Clone)]
pub struct PipelineConfig {
    /// Batch size for block streaming
    pub batch_size: u64,
    /// Number of batches between mini-checkpoints
    pub mini_checkpoint_interval: u32,
    /// Maximum concurrent decryption tasks
    pub max_decrypt_concurrency: usize,
    /// App version for checkpoints
    pub app_version: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            batch_size: PIPELINE_BATCH_SIZE,
            mini_checkpoint_interval: MINI_CHECKPOINT_INTERVAL,
            max_decrypt_concurrency: MAX_DECRYPT_CONCURRENCY,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Sync pipeline for streaming block processing
pub struct SyncPipeline {
    /// Light client for block fetching
    client: LightClient,
    /// Pipeline configuration
    config: PipelineConfig,
    /// Sapling frontier for commitment tree
    frontier: Arc<RwLock<SaplingFrontier>>,
    /// Performance counters
    perf: Arc<PerfCounters>,
    /// Mini-checkpoint callback
    on_mini_checkpoint: Option<MiniCheckpointCallback>,
    /// Cancellation flag
    cancelled: Arc<RwLock<bool>>,
    /// Sapling IVK bytes (32 bytes) for trial decryption
    viewing_key: Option<Vec<u8>>,
}

impl SyncPipeline {
    /// Create new sync pipeline
    pub fn new(endpoint: String, config: PipelineConfig) -> Self {
        Self {
            client: LightClient::new(endpoint),
            config,
            frontier: Arc::new(RwLock::new(SaplingFrontier::new())),
            perf: Arc::new(PerfCounters::new()),
            on_mini_checkpoint: None,
            cancelled: Arc::new(RwLock::new(false)),
            viewing_key: None,
        }
    }

    /// Set viewing key for trial decryption
    pub fn with_viewing_key(mut self, key: Vec<u8>) -> Self {
        self.viewing_key = Some(key);
        self
    }

    /// Set mini-checkpoint callback
    pub fn with_mini_checkpoint_callback(mut self, callback: MiniCheckpointCallback) -> Self {
        self.on_mini_checkpoint = Some(callback);
        self
    }

    /// Set existing frontier
    pub fn with_frontier(mut self, frontier: SaplingFrontier) -> Self {
        self.frontier = Arc::new(RwLock::new(frontier));
        self
    }

    /// Get performance counters reference
    pub fn perf_counters(&self) -> Arc<PerfCounters> {
        Arc::clone(&self.perf)
    }

    /// Get frontier reference
    pub fn frontier(&self) -> Arc<RwLock<SaplingFrontier>> {
        Arc::clone(&self.frontier)
    }

    /// Cancel pipeline
    pub async fn cancel(&self) {
        *self.cancelled.write().await = true;
    }

    /// Check if cancelled
    async fn is_cancelled(&self) -> bool {
        *self.cancelled.read().await
    }

    /// Run the sync pipeline
    pub async fn run(&mut self, start_height: u64, end_height: u64) -> Result<PipelineResult> {
        *self.cancelled.write().await = false;
        self.perf.reset();

        // Connect to lightwalletd
        self.client.connect().await?;

        let pipeline_start = Instant::now();
        let mut all_notes = Vec::new();
        let mut current_height = start_height;
        let mut batch_count = 0u32;
        let mut last_checkpoint_height = start_height;

        tracing::info!(
            "Starting pipeline: {} -> {} ({} blocks, batch_size={})",
            start_height,
            end_height,
            end_height - start_height + 1,
            self.config.batch_size
        );

        // Create channel for streaming blocks
        let (tx, mut rx) = mpsc::channel::<Vec<CompactBlockData>>(BLOCK_CHANNEL_BUFFER);

        // Spawn block fetcher
        let client = self.client.clone();
        let batch_size = self.config.batch_size;
        let cancelled = Arc::clone(&self.cancelled);
        let fetch_start = start_height;
        let fetch_end = end_height;

        let fetcher = tokio::spawn(async move {
            let mut current = fetch_start;
            while current <= fetch_end {
                if *cancelled.read().await {
                    break;
                }

                let batch_end = std::cmp::min(current + batch_size - 1, fetch_end);
                match client.stream_blocks(current, batch_end).await {
                    Ok(blocks) => {
                        if tx.send(blocks).await.is_err() {
                            break; // Receiver dropped
                        }
                    }
                    Err(e) => {
                        tracing::error!("Block fetch failed at {}: {:?}", current, e);
                        break;
                    }
                }
                current = batch_end + 1;
            }
        });

        // Process batches from channel
        while let Some(blocks) = rx.recv().await {
            if self.is_cancelled().await {
                tracing::warn!("Pipeline cancelled at height {}", current_height);
                break;
            }

            let batch_start = Instant::now();
            let _batch_start_height = blocks.first().map(|b| b.height).unwrap_or(current_height);
            let batch_end_height = blocks.last().map(|b| b.height).unwrap_or(current_height);

            // Stage 1: Batched trial decryption
            let (notes, commitments) = self.process_batch(&blocks).await?;

            // Stage 2: Apply commitments to frontier
            self.apply_commitments(&commitments).await?;

            // Record perf
            let batch_duration = batch_start.elapsed();
            self.perf.record_batch(
                blocks.len() as u64,
                notes.len() as u64,
                commitments.len() as u64,
                batch_duration.as_millis() as u64,
            );

            all_notes.extend(notes);
            current_height = batch_end_height + 1;
            batch_count += 1;

            // Mini-checkpoint every N batches
            if batch_count.is_multiple_of(self.config.mini_checkpoint_interval) {
                self.create_mini_checkpoint(batch_end_height as u32).await?;
                last_checkpoint_height = batch_end_height;
            }

            tracing::debug!(
                "Batch {}: {} blocks in {}ms ({:.1} blk/s), {} notes, {} commitments",
                batch_count,
                blocks.len(),
                batch_duration.as_millis(),
                self.perf.blocks_per_second(),
                all_notes.len(),
                self.perf.commitments_applied.load(Ordering::Relaxed)
            );
        }

        // Wait for fetcher to complete
        let _ = fetcher.await;

        // Final checkpoint
        if current_height > last_checkpoint_height + 1 {
            self.create_mini_checkpoint((current_height - 1) as u32)
                .await?;
        }

        let total_duration = pipeline_start.elapsed();
        let perf_snapshot = self.perf.snapshot();

        tracing::info!(
            "Pipeline complete: {} blocks in {:.2}s ({:.1} blk/s), {} notes found",
            perf_snapshot.blocks_processed,
            total_duration.as_secs_f64(),
            perf_snapshot.blocks_per_second,
            all_notes.len()
        );

        Ok(PipelineResult {
            start_height,
            end_height: current_height.saturating_sub(1),
            notes: all_notes,
            perf: perf_snapshot,
            duration: total_duration,
            cancelled: self.is_cancelled().await,
        })
    }

    /// Process a batch of blocks (trial decryption)
    async fn process_batch(
        &self,
        blocks: &[CompactBlockData],
    ) -> Result<(Vec<DecryptedNote>, Vec<[u8; 32]>)> {
        let semaphore = Arc::new(Semaphore::new(self.config.max_decrypt_concurrency));
        let viewing_key = self.viewing_key.clone();

        let mut tasks = Vec::new();
        for (block_idx, block) in blocks.iter().enumerate() {
            let sem = Arc::clone(&semaphore);
            let block = block.clone();
            let vk = viewing_key.clone();

            let task = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                process_block_trial_decrypt(&block, block_idx, vk.as_deref())
            });

            tasks.push(task);
        }

        let mut all_notes = Vec::new();
        let mut all_commitments = Vec::new();

        for task in tasks {
            let (notes, commitments) = task.await.map_err(|e| Error::Sync(e.to_string()))??;
            all_notes.extend(notes);
            all_commitments.extend(commitments);
        }

        Ok((all_notes, all_commitments))
    }

    /// Apply note commitments to frontier
    async fn apply_commitments(&self, commitments: &[[u8; 32]]) -> Result<()> {
        let mut frontier = self.frontier.write().await;
        for cm in commitments {
            frontier.apply_note_commitment(*cm)?;
        }
        Ok(())
    }

    /// Create mini-checkpoint
    async fn create_mini_checkpoint(&self, height: u32) -> Result<()> {
        if let Some(ref callback) = self.on_mini_checkpoint {
            let frontier = self.frontier.read().await;
            let frontier_bytes = frontier.serialize();
            callback(height, &frontier_bytes)?;
            tracing::debug!("Mini-checkpoint at height {}", height);
        }
        Ok(())
    }
}

/// Process single block for trial decryption
fn process_block_trial_decrypt(
    block: &CompactBlockData,
    _block_idx: usize,
    viewing_key: Option<&[u8]>,
) -> Result<(Vec<DecryptedNote>, Vec<[u8; 32]>)> {
    let mut notes = Vec::new();
    let mut commitments = Vec::new();

    let ivk_bytes: Option<[u8; 32]> = viewing_key.and_then(|vk| vk.try_into().ok());

    for (tx_idx, tx) in block.transactions.iter().enumerate() {
        for (output_idx, output) in tx.outputs.iter().enumerate() {
            // Extract commitment
            if output.cmu.len() == 32 {
                let mut cm = [0u8; 32];
                cm.copy_from_slice(&output.cmu);
                commitments.push(cm);

                // Real compact trial decryption (memo is not available from compact blocks).
                if let Some(ivk) = ivk_bytes.as_ref() {
                    if let Some(decrypted) = try_decrypt_compact_output(ivk, output) {
                        let mut note = DecryptedNote::new(
                            block.height,
                            tx.index.unwrap_or(tx_idx as u64) as usize,
                            output_idx,
                            decrypted.value,
                            cm,
                            [0u8; 32],  // nullifier computed later when spending
                            Vec::new(), // compact stream has no memo bytes
                        );
                        note.set_tx_hash(tx.hash.clone());
                        note.diversifier = decrypted.diversifier.to_vec();
                        note.sapling_rseed_leadbyte = Some(decrypted.leadbyte);
                        note.sapling_rseed = Some(decrypted.rseed);
                        notes.push(note);
                    }
                }
            }
        }
    }

    Ok((notes, commitments))
}

/// Pipeline execution result
#[derive(Debug)]
pub struct PipelineResult {
    /// Start height
    pub start_height: u64,
    /// End height processed
    pub end_height: u64,
    /// Decrypted notes found
    pub notes: Vec<DecryptedNote>,
    /// Performance snapshot
    pub perf: PerfSnapshot,
    /// Total duration
    pub duration: Duration,
    /// Whether pipeline was cancelled
    pub cancelled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perf_counters() {
        let counters = PerfCounters::new();
        counters.record_batch(100, 5, 200, 1000);

        assert_eq!(counters.blocks_processed.load(Ordering::Relaxed), 100);
        assert_eq!(counters.notes_decrypted.load(Ordering::Relaxed), 5);
        assert_eq!(counters.commitments_applied.load(Ordering::Relaxed), 200);
        assert_eq!(counters.last_batch_ms.load(Ordering::Relaxed), 1000);
        assert_eq!(counters.blocks_per_second(), 100.0);
    }

    #[test]
    fn test_perf_counters_multiple_batches() {
        let counters = PerfCounters::new();
        counters.record_batch(100, 5, 200, 1000);
        counters.record_batch(100, 3, 150, 800);

        assert_eq!(counters.blocks_processed.load(Ordering::Relaxed), 200);
        assert_eq!(counters.notes_decrypted.load(Ordering::Relaxed), 8);
        assert_eq!(counters.avg_batch_ms(), 900); // (1000+800)/2
    }

    #[test]
    fn test_lazy_memo_decode() {
        let mut note = DecryptedNote::new(
            1000,
            0,
            0,
            100_000_000,
            [0u8; 32],
            [0u8; 32],
            b"Hello, Pirate!".to_vec(),
        );

        assert!(!note.is_memo_decoded());
        let memo = note.memo();
        assert_eq!(memo, Some("Hello, Pirate!"));
        assert!(note.is_memo_decoded());
    }

    #[test]
    fn test_lazy_memo_empty() {
        let mut note = DecryptedNote::new(1000, 0, 0, 100_000_000, [0u8; 32], [0u8; 32], vec![]);

        assert_eq!(note.memo(), None);
    }

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.batch_size, PIPELINE_BATCH_SIZE);
        assert_eq!(config.mini_checkpoint_interval, MINI_CHECKPOINT_INTERVAL);
        assert_eq!(config.max_decrypt_concurrency, MAX_DECRYPT_CONCURRENCY);
    }

    #[test]
    fn test_decode_memo_with_padding() {
        let mut memo_bytes = b"Test memo".to_vec();
        memo_bytes.extend(vec![0u8; 503]); // Pad to 512

        let decoded = decode_memo(&memo_bytes);
        assert_eq!(decoded, Some("Test memo".to_string()));
    }

    #[tokio::test]
    async fn test_pipeline_creation() {
        let config = PipelineConfig::default();
        let pipeline = SyncPipeline::new("https://test:9067".to_string(), config);

        assert!(!pipeline.is_cancelled().await);
        pipeline.cancel().await;
        assert!(pipeline.is_cancelled().await);
    }
}
