//! Device-local flow control for compact-block intake.
//!
//! Network ranges are intentionally absent from this module. The light server
//! must not observe local scan-batch decisions; this controller only sizes work
//! after compact blocks have crossed the durable cache boundary.

use crate::{CancelToken, Error, Result};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

/// Initial block ceiling for a durable compact-block stream segment.
pub const DEFAULT_DURABLE_SEGMENT_BLOCKS: u64 = 1_024;

/// Device-independent choices available to the local durable-segment router.
pub const DURABLE_SEGMENT_BLOCK_BUCKETS: [u64; 8] =
    [256, 512, 1_024, 2_048, 4_096, 8_192, 16_384, 32_768];

/// Channel slots used for durable standardized segments. Actual memory remains
/// bounded by [`PrefetchWatermarks`], not this count.
pub const DURABLE_SEGMENT_CHANNEL_CAPACITY: usize = 64;

/// Keep one locally sized batch ready for trial-decryption lookahead. Durable
/// segments remain available behind this router without freezing its target.
pub const LOCAL_SCAN_BATCH_CHANNEL_CAPACITY: usize = 1;

/// Default processing-time target for network-fed local scan batches.
/// Cached rescans use throughput and parallel-saturation feedback instead.
pub const DEFAULT_NETWORK_SCAN_BATCH_TARGET: Duration = Duration::from_millis(250);

const DURABLE_SEGMENT_NETWORK_TARGET: Duration = Duration::from_secs(1);
const DURABLE_SEGMENT_CACHE_TARGET: Duration = Duration::from_millis(100);
const DURABLE_SEGMENT_CONFIRMATIONS: u8 = 3;
const DURABLE_SEGMENT_COOLDOWN: u8 = 2;
const CACHED_SCAN_CONFIRMATIONS: u8 = 3;
const CACHED_SCAN_COOLDOWN: u8 = 8;
const CACHED_SCAN_THROUGHPUT_GAIN_PPM: u64 = 20_000;
const CACHED_SCAN_PARALLEL_GAIN_PPM: u64 = 30_000;
const CACHED_SCAN_REGRESSION_TOLERANCE_PPM: u64 = 30_000;
const RATE_SCALE_PPM: u128 = 1_000_000;

/// Measurements from one validated and persisted durable stream segment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DurableSegmentObservation {
    pub(crate) blocks: u64,
    pub(crate) encoded_bytes: u64,
    pub(crate) network_wait: Duration,
    pub(crate) cache_write: Duration,
    pub(crate) queued_bytes: u64,
    pub(crate) high_water_bytes: u64,
    pub(crate) stream_tail: bool,
}

/// Bucketed controller for the internal durable stream handoff.
///
/// It changes at most one bucket after three agreeing observations, then waits
/// two more observations before another change. Server request ranges remain
/// unchanged; this controls only validation/cache handoff boundaries.
#[derive(Clone, Debug)]
pub(crate) struct AdaptiveDurableSegmentController {
    current_index: usize,
    max_segment_bytes: u64,
    network_nanos_per_block: u64,
    cache_nanos_per_block: u64,
    pending_direction: i8,
    confirmations: u8,
    cooldown: u8,
}

impl AdaptiveDurableSegmentController {
    pub(crate) fn new(max_segment_bytes: u64) -> Self {
        let current_index = DURABLE_SEGMENT_BLOCK_BUCKETS
            .iter()
            .position(|blocks| *blocks == DEFAULT_DURABLE_SEGMENT_BLOCKS)
            .expect("default durable segment bucket");
        Self {
            current_index,
            max_segment_bytes: max_segment_bytes.max(1),
            network_nanos_per_block: 0,
            cache_nanos_per_block: 0,
            pending_direction: 0,
            confirmations: 0,
            cooldown: 0,
        }
    }

    pub(crate) fn target_blocks(&self) -> u64 {
        DURABLE_SEGMENT_BLOCK_BUCKETS[self.current_index]
    }

    pub(crate) fn observe(&mut self, observation: DurableSegmentObservation) -> u64 {
        if observation.blocks == 0 || observation.stream_tail {
            return self.target_blocks();
        }

        let current_target = self.target_blocks();
        let average_encoded_bytes = observation
            .encoded_bytes
            .max(1)
            .div_ceil(observation.blocks);
        let byte_safe_blocks = (self.max_segment_bytes / average_encoded_bytes).max(1);

        // Ignore short retry fragments unless the exact byte ceiling caused
        // them. They do not represent sustained network or cache throughput.
        let byte_limited = observation.encoded_bytes
            >= self
                .max_segment_bytes
                .saturating_sub(self.max_segment_bytes / 4);
        if observation.blocks < current_target.div_ceil(2) && !byte_limited {
            return current_target;
        }

        let network_sample = nanos_per_block(observation.network_wait, observation.blocks);
        let cache_sample = nanos_per_block(observation.cache_write, observation.blocks);
        self.network_nanos_per_block =
            update_rate_ewma(self.network_nanos_per_block, network_sample);
        self.cache_nanos_per_block = update_rate_ewma(self.cache_nanos_per_block, cache_sample);

        let network_ideal =
            duration_target_blocks(DURABLE_SEGMENT_NETWORK_TARGET, self.network_nanos_per_block);
        let cache_ideal =
            duration_target_blocks(DURABLE_SEGMENT_CACHE_TARGET, self.cache_nanos_per_block);
        let mut ideal = network_ideal.min(cache_ideal).min(byte_safe_blocks).max(1);

        // Once the scanner is already exerting backpressure, larger durable
        // chunks cannot improve network utilization and only increase the work
        // retained across cancellation or rollback.
        let high_pressure = observation.high_water_bytes > 0
            && observation.queued_bytes
                >= observation
                    .high_water_bytes
                    .saturating_sub(observation.high_water_bytes / 4);
        if high_pressure {
            ideal = ideal.min(current_target);
        }

        let candidate_index = nearest_durable_bucket(ideal, byte_safe_blocks);
        let direction = match candidate_index.cmp(&self.current_index) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        if direction == 0 {
            self.pending_direction = 0;
            self.confirmations = 0;
            self.cooldown = self.cooldown.saturating_sub(1);
            return current_target;
        }

        if self.cooldown > 0 {
            self.cooldown -= 1;
            return current_target;
        }

        if self.pending_direction == direction {
            self.confirmations = self.confirmations.saturating_add(1);
        } else {
            self.pending_direction = direction;
            self.confirmations = 1;
        }
        if self.confirmations < DURABLE_SEGMENT_CONFIRMATIONS {
            return current_target;
        }

        if direction < 0 {
            self.current_index = self.current_index.saturating_sub(1);
        } else {
            self.current_index =
                (self.current_index + 1).min(DURABLE_SEGMENT_BLOCK_BUCKETS.len().saturating_sub(1));
        }
        self.pending_direction = 0;
        self.confirmations = 0;
        self.cooldown = DURABLE_SEGMENT_COOLDOWN;
        self.target_blocks()
    }
}

fn nanos_per_block(duration: Duration, blocks: u64) -> u64 {
    duration
        .as_nanos()
        .max(1)
        .div_ceil(u128::from(blocks.max(1)))
        .min(u128::from(u64::MAX)) as u64
}

fn update_rate_ewma(previous: u64, sample: u64) -> u64 {
    if previous == 0 {
        sample.max(1)
    } else {
        ((u128::from(previous) * 3 + u128::from(sample)) / 4).min(u128::from(u64::MAX)) as u64
    }
}

fn duration_target_blocks(target: Duration, nanos_per_block: u64) -> u64 {
    target
        .as_nanos()
        .checked_div(u128::from(nanos_per_block.max(1)))
        .unwrap_or(u128::from(u64::MAX))
        .min(u128::from(u64::MAX)) as u64
}

fn nearest_durable_bucket(ideal: u64, byte_safe_blocks: u64) -> usize {
    DURABLE_SEGMENT_BLOCK_BUCKETS
        .iter()
        .enumerate()
        .filter(|(_, blocks)| **blocks <= byte_safe_blocks)
        .min_by_key(|(_, blocks)| blocks.abs_diff(ideal))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// Chooses how many blocks from a durable network segment fit in the current
/// device-local scan batch.
///
/// A return value of zero means the existing batch should be emitted first. A
/// single block larger than the byte target is admitted by itself so the
/// pipeline always makes progress while preserving its strict block ordering.
pub(crate) fn local_batch_prefix_len(
    encoded_block_bytes: &[u64],
    current_blocks: u64,
    current_bytes: u64,
    target_blocks: u64,
    target_bytes: u64,
) -> usize {
    if encoded_block_bytes.is_empty() {
        return 0;
    }

    let target_blocks = target_blocks.max(1);
    let target_bytes = target_bytes.max(1);
    if current_blocks >= target_blocks || current_bytes >= target_bytes {
        return 0;
    }

    let block_slots = target_blocks.saturating_sub(current_blocks) as usize;
    let mut selected = 0usize;
    let mut added_bytes = 0u64;
    for encoded_bytes in encoded_block_bytes.iter().copied().take(block_slots) {
        let encoded_bytes = encoded_bytes.max(1);
        if current_bytes
            .saturating_add(added_bytes)
            .saturating_add(encoded_bytes)
            > target_bytes
        {
            break;
        }
        added_bytes = added_bytes.saturating_add(encoded_bytes);
        selected += 1;
    }

    if selected == 0 && current_blocks == 0 {
        1
    } else {
        selected
    }
}
