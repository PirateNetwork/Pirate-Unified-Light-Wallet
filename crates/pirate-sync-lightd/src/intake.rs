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
const SHIELDED_WORK_TARGET: Duration = Duration::from_millis(500);
const SHIELDED_WORK_MIN_PER_LANE: u64 = 256;
const SHIELDED_WORK_INITIAL_PER_LANE: u64 = 1_024;
const SHIELDED_WORK_MAX_PER_LANE: u64 = 8_192;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LocalBatchWeight {
    pub(crate) blocks: u64,
    pub(crate) encoded_bytes: u64,
    pub(crate) shielded_work_items: u64,
}

/// Chooses how many blocks from a durable network segment fit in the current
/// device-local scan batch.
///
/// A return value of zero means the existing batch should be emitted first. A
/// single block larger than the byte target is admitted by itself so the
/// pipeline always makes progress while preserving its strict block ordering.
pub(crate) fn local_batch_prefix_len(
    encoded_block_bytes: &[u64],
    block_work_items: &[u64],
    current: LocalBatchWeight,
    target: LocalBatchWeight,
) -> usize {
    if encoded_block_bytes.is_empty() || encoded_block_bytes.len() != block_work_items.len() {
        return 0;
    }

    let target_blocks = target.blocks.max(1);
    let target_bytes = target.encoded_bytes.max(1);
    let target_work_items = target.shielded_work_items.max(1);
    if current.blocks >= target_blocks
        || current.encoded_bytes >= target_bytes
        || current.shielded_work_items >= target_work_items
    {
        return 0;
    }

    let block_slots = target_blocks.saturating_sub(current.blocks) as usize;
    let mut selected = 0usize;
    let mut added_bytes = 0u64;
    let mut added_work_items = 0u64;
    for (encoded_bytes, work_items) in encoded_block_bytes
        .iter()
        .copied()
        .zip(block_work_items.iter().copied())
        .take(block_slots)
    {
        let encoded_bytes = encoded_bytes.max(1);
        if current
            .encoded_bytes
            .saturating_add(added_bytes)
            .saturating_add(encoded_bytes)
            > target_bytes
            || current
                .shielded_work_items
                .saturating_add(added_work_items)
                .saturating_add(work_items)
                > target_work_items
        {
            break;
        }
        added_bytes = added_bytes.saturating_add(encoded_bytes);
        added_work_items = added_work_items.saturating_add(work_items);
        selected += 1;
    }

    if selected == 0 && current.blocks == 0 {
        1
    } else {
        selected
    }
}

/// Device-local shielded-work controller.
///
/// Block and byte bounds protect memory, while this bound prevents a dense
/// compact range from turning into one very long trial-decryption or tree
/// insertion step. It is never used to choose a server-visible request range.
#[derive(Clone, Debug)]
pub(crate) struct AdaptiveShieldedWorkBatcher {
    min_items: u64,
    max_items: u64,
    current_items: u64,
    ewma_nanos_per_item: Option<u128>,
}

impl AdaptiveShieldedWorkBatcher {
    pub(crate) fn new(parallel_lanes: usize) -> Self {
        let lanes = parallel_lanes.max(1) as u64;
        let min_items = lanes.saturating_mul(SHIELDED_WORK_MIN_PER_LANE).max(1);
        let max_items = lanes
            .saturating_mul(SHIELDED_WORK_MAX_PER_LANE)
            .max(min_items);
        Self {
            min_items,
            max_items,
            current_items: lanes
                .saturating_mul(SHIELDED_WORK_INITIAL_PER_LANE)
                .clamp(min_items, max_items),
            ewma_nanos_per_item: None,
        }
    }

    pub(crate) fn target_items(&self) -> u64 {
        self.current_items
    }

    pub(crate) fn observe(
        &mut self,
        requested_items: u64,
        completed_items: u64,
        processing_time: Duration,
        stream_tail: bool,
    ) -> u64 {
        if completed_items == 0 || stream_tail || requested_items != self.current_items {
            return self.current_items;
        }

        // Sparse batches are bounded by blocks or bytes and do not describe
        // shielded-work saturation. They must not inflate this controller.
        if completed_items < requested_items.saturating_mul(3).div_ceil(4) {
            return self.current_items;
        }

        let sample = processing_time
            .as_nanos()
            .max(1)
            .checked_div(u128::from(completed_items))
            .unwrap_or(1)
            .max(1);
        self.ewma_nanos_per_item = Some(match self.ewma_nanos_per_item {
            Some(previous) => previous.saturating_mul(3).saturating_add(sample) / 4,
            None => sample,
        });
        let ideal = SHIELDED_WORK_TARGET
            .as_nanos()
            .checked_div(self.ewma_nanos_per_item.unwrap_or(1))
            .unwrap_or(u128::from(self.max_items))
            .min(u128::from(self.max_items)) as u64;
        let lower = self
            .current_items
            .saturating_mul(3)
            .div_ceil(4)
            .max(self.min_items);
        let upper = self
            .current_items
            .saturating_mul(5)
            .div_ceil(4)
            .min(self.max_items);
        let quantum = SHIELDED_WORK_MIN_PER_LANE.max(1);
        let next = ideal.clamp(lower, upper).saturating_add(quantum / 2) / quantum * quantum;
        self.current_items = next.clamp(self.min_items, self.max_items);
        self.current_items
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanBatchSource {
    Cache,
    Network,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanBatchDecision {
    CacheAtCeiling,
    CacheCollecting,
    CacheCooldown,
    CacheProbeLarger,
    CacheProbeSmaller,
    CachePlateau,
    CacheRegression,
    CacheStaleSample,
    IgnoredTail,
    NetworkLatency,
}

impl ScanBatchDecision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::CacheAtCeiling => "cache_at_ceiling",
            Self::CacheCollecting => "cache_collecting",
            Self::CacheCooldown => "cache_cooldown",
            Self::CacheProbeLarger => "cache_probe_larger",
            Self::CacheProbeSmaller => "cache_probe_smaller",
            Self::CachePlateau => "cache_plateau",
            Self::CacheRegression => "cache_regression",
            Self::CacheStaleSample => "cache_stale_sample",
            Self::IgnoredTail => "ignored_tail",
            Self::NetworkLatency => "network_latency",
        }
    }
}

/// Measurements from one committed local scan batch.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScanBatchObservation {
    /// Block target used when this batch was assembled.
    pub(crate) requested_blocks: u64,
    /// Exact byte target used when this batch was assembled.
    pub(crate) requested_bytes: u64,
    /// Number of compact blocks committed by the batch.
    pub(crate) blocks: u64,
    /// Exact encoded bytes represented by the batch.
    pub(crate) encoded_bytes: u64,
    /// Local work excluding time waiting for the intake queue.
    pub(crate) processing_time: Duration,
    /// Time the scan loop waited for its next durable batch.
    pub(crate) intake_wait: Duration,
    /// Encoded bytes currently admitted ahead of the scanner.
    pub(crate) queued_bytes: u64,
    /// Source of the already durable compact blocks.
    pub(crate) source: ScanBatchSource,
    /// Wall time spent constructing immutable ShardTree insertions.
    pub(crate) tree_parallel_wall: Duration,
    /// Worker-active time accumulated during parallel ShardTree construction.
    pub(crate) tree_parallel_worker_active: Duration,
    /// Whether this was the shortened final batch of a bounded sync.
    pub(crate) stream_tail: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct CachedOperatingPoint {
    target_blocks: u64,
    blocks_per_second: u64,
    parallel_saturation_ppm: u64,
    has_parallel_sample: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct CachedSampleAccumulator {
    target_blocks: u64,
    samples: u8,
    blocks: u128,
    processing_nanos: u128,
    parallel_wall_nanos: u128,
    parallel_worker_nanos: u128,
}

impl CachedSampleAccumulator {
    fn clear_for(&mut self, target_blocks: u64) {
        *self = Self {
            target_blocks,
            ..Self::default()
        };
    }

    fn push(&mut self, observation: ScanBatchObservation) {
        if self.target_blocks != observation.requested_blocks {
            self.clear_for(observation.requested_blocks);
        }
        self.samples = self.samples.saturating_add(1);
        self.blocks = self.blocks.saturating_add(u128::from(observation.blocks));
        self.processing_nanos = self
            .processing_nanos
            .saturating_add(observation.processing_time.as_nanos().max(1));
        self.parallel_wall_nanos = self
            .parallel_wall_nanos
            .saturating_add(observation.tree_parallel_wall.as_nanos());
        self.parallel_worker_nanos = self
            .parallel_worker_nanos
            .saturating_add(observation.tree_parallel_worker_active.as_nanos());
    }

    fn operating_point(&self, parallel_workers: u64) -> CachedOperatingPoint {
        let blocks_per_second = self
            .blocks
            .saturating_mul(1_000_000_000)
            .checked_div(self.processing_nanos.max(1))
            .unwrap_or(u128::from(u64::MAX))
            .min(u128::from(u64::MAX)) as u64;
        let has_parallel_sample = self.parallel_wall_nanos > 0;
        let parallel_capacity_nanos = self
            .parallel_wall_nanos
            .saturating_mul(u128::from(parallel_workers.max(1)));
        let parallel_saturation_ppm = if has_parallel_sample {
            self.parallel_worker_nanos
                .saturating_mul(RATE_SCALE_PPM)
                .checked_div(parallel_capacity_nanos.max(1))
                .unwrap_or(RATE_SCALE_PPM)
                .min(RATE_SCALE_PPM) as u64
        } else {
            0
        };
        CachedOperatingPoint {
            target_blocks: self.target_blocks,
            blocks_per_second,
            parallel_saturation_ppm,
            has_parallel_sample,
        }
    }
}

#[derive(Clone, Debug)]
struct CachedScanThroughputController {
    parallel_workers: u64,
    samples: CachedSampleAccumulator,
    baseline: Option<CachedOperatingPoint>,
    probe_direction: i8,
    next_probe_direction: i8,
    cooldown: u8,
    last_blocks_per_second: u64,
    last_parallel_saturation_ppm: u64,
}

impl CachedScanThroughputController {
    fn new(parallel_workers: usize) -> Self {
        Self {
            parallel_workers: parallel_workers.max(1) as u64,
            samples: CachedSampleAccumulator::default(),
            baseline: None,
            probe_direction: 0,
            next_probe_direction: 0,
            cooldown: 0,
            last_blocks_per_second: 0,
            last_parallel_saturation_ppm: 0,
        }
    }

    fn reset(&mut self) {
        self.samples = CachedSampleAccumulator::default();
        self.baseline = None;
        self.probe_direction = 0;
        self.next_probe_direction = 0;
        self.cooldown = 0;
        self.last_blocks_per_second = 0;
        self.last_parallel_saturation_ppm = 0;
    }

    fn observe(
        &mut self,
        current_blocks: u64,
        min_blocks: u64,
        max_blocks: u64,
        observation: ScanBatchObservation,
    ) -> (u64, ScanBatchDecision) {
        if observation.blocks == 0 || observation.stream_tail {
            return (current_blocks, ScanBatchDecision::IgnoredTail);
        }
        if observation.requested_blocks != current_blocks {
            return (current_blocks, ScanBatchDecision::CacheStaleSample);
        }

        // Exact byte limits in the decoder and queue watermarks remain the
        // memory boundary. A short byte-limited batch is therefore valid, but
        // it must not teach the block controller that fewer blocks are faster.
        let byte_limited = observation.blocks < observation.requested_blocks
            && observation.encoded_bytes
                >= observation
                    .requested_bytes
                    .max(1)
                    .saturating_sub(observation.requested_bytes.max(1) / 4);
        if observation.blocks < observation.requested_blocks.div_ceil(2) && !byte_limited {
            return (current_blocks, ScanBatchDecision::CacheStaleSample);
        }

        self.samples.push(observation);
        if self.samples.samples < CACHED_SCAN_CONFIRMATIONS {
            return (current_blocks, ScanBatchDecision::CacheCollecting);
        }

        let point = self.samples.operating_point(self.parallel_workers);
        self.last_blocks_per_second = point.blocks_per_second;
        self.last_parallel_saturation_ppm = point.parallel_saturation_ppm;
        self.samples.clear_for(current_blocks);

        if self.cooldown > 0 {
            self.baseline = Some(point);
            self.cooldown -= 1;
            return (current_blocks, ScanBatchDecision::CacheCooldown);
        }

        if self.probe_direction != 0 {
            let baseline = self
                .baseline
                .expect("an active cached-batch probe has a baseline");
            let throughput_improved = rate_improved(
                point.blocks_per_second,
                baseline.blocks_per_second,
                CACHED_SCAN_THROUGHPUT_GAIN_PPM,
            );
            let throughput_ok = rate_within_regression(
                point.blocks_per_second,
                baseline.blocks_per_second,
                CACHED_SCAN_REGRESSION_TOLERANCE_PPM,
            );
            let parallel_comparable = point.has_parallel_sample && baseline.has_parallel_sample;
            let parallel_improved = parallel_comparable
                && point.parallel_saturation_ppm
                    >= baseline
                        .parallel_saturation_ppm
                        .saturating_add(CACHED_SCAN_PARALLEL_GAIN_PPM);
            let candidate_better = throughput_improved || (throughput_ok && parallel_improved);

            if candidate_better {
                self.baseline = Some(point);
                let (next, direction) =
                    probe_scan_target(current_blocks, min_blocks, max_blocks, self.probe_direction);
                if direction != 0 {
                    self.probe_direction = direction;
                    return (next, probe_decision(direction));
                }

                self.next_probe_direction = -self.probe_direction;
                self.probe_direction = 0;
                self.cooldown = CACHED_SCAN_COOLDOWN;
                return (current_blocks, ScanBatchDecision::CacheAtCeiling);
            } else {
                self.cooldown = CACHED_SCAN_COOLDOWN;
                let decision = if throughput_ok {
                    ScanBatchDecision::CachePlateau
                } else {
                    ScanBatchDecision::CacheRegression
                };
                self.next_probe_direction = -self.probe_direction;
                self.probe_direction = 0;
                self.baseline = Some(baseline);
                return (
                    baseline.target_blocks.clamp(min_blocks, max_blocks),
                    decision,
                );
            }
        }

        self.baseline = Some(point);
        let preferred_direction = if self.next_probe_direction != 0 {
            std::mem::take(&mut self.next_probe_direction)
        } else if current_blocks < max_blocks {
            1
        } else {
            -1
        };
        let (next, direction) =
            probe_scan_target(current_blocks, min_blocks, max_blocks, preferred_direction);
        self.probe_direction = direction;
        if direction == 0 {
            (current_blocks, ScanBatchDecision::CacheAtCeiling)
        } else {
            (next, probe_decision(direction))
        }
    }
}

fn probe_decision(direction: i8) -> ScanBatchDecision {
    if direction < 0 {
        ScanBatchDecision::CacheProbeSmaller
    } else {
        ScanBatchDecision::CacheProbeLarger
    }
}

fn probe_scan_target(
    current: u64,
    min_blocks: u64,
    max_blocks: u64,
    preferred_direction: i8,
) -> (u64, i8) {
    let preferred = if preferred_direction < 0 { -1 } else { 1 };
    for direction in [preferred, -preferred] {
        let candidate = if direction < 0 {
            shrink_scan_target(current, min_blocks, max_blocks)
        } else {
            grow_scan_target(current, min_blocks, max_blocks)
        };
        if candidate != current {
            return (candidate, direction);
        }
    }
    (current, 0)
}

fn rate_improved(candidate: u64, baseline: u64, improvement_ppm: u64) -> bool {
    u128::from(candidate).saturating_mul(RATE_SCALE_PPM)
        >= u128::from(baseline)
            .saturating_mul(RATE_SCALE_PPM.saturating_add(u128::from(improvement_ppm)))
}

fn rate_within_regression(candidate: u64, baseline: u64, tolerance_ppm: u64) -> bool {
    u128::from(candidate).saturating_mul(RATE_SCALE_PPM)
        >= u128::from(baseline)
            .saturating_mul(RATE_SCALE_PPM.saturating_sub(u128::from(tolerance_ppm.min(1_000_000))))
}

fn grow_scan_target(current: u64, min_blocks: u64, max_blocks: u64) -> u64 {
    let mut next = current
        .saturating_mul(5)
        .div_ceil(4)
        .max(current.saturating_add(1));
    if max_blocks.saturating_sub(min_blocks) >= 64 {
        next = next.saturating_add(32) / 64 * 64;
    }
    next.clamp(min_blocks, max_blocks)
}

fn shrink_scan_target(current: u64, min_blocks: u64, max_blocks: u64) -> u64 {
    let mut next = current.saturating_mul(4) / 5;
    if max_blocks.saturating_sub(min_blocks) >= 64 {
        next = next.saturating_add(32) / 64 * 64;
    }
    if next >= current {
        next = current.saturating_sub(1);
    }
    next.clamp(min_blocks, max_blocks)
}

/// Device-local controller with separate network-latency and cached-throughput
/// feedback. Profile limits and exact byte admission remain the safety envelope.
#[derive(Clone, Debug)]
pub(crate) struct AdaptiveScanBatcher {
    min_blocks: u64,
    max_blocks: u64,
    current_blocks: u64,
    network_target_time: Duration,
    max_batch_bytes: u64,
    network_ewma_nanos_per_block: Option<u128>,
    network_samples: u32,
    cached: CachedScanThroughputController,
    last_source: Option<ScanBatchSource>,
    last_decision: ScanBatchDecision,
}

impl AdaptiveScanBatcher {
    /// Creates a controller bounded by local profile safety limits.
    #[cfg(test)]
    pub(crate) fn new(
        initial_blocks: u64,
        min_blocks: u64,
        max_blocks: u64,
        network_target_time: Duration,
        max_batch_bytes: u64,
    ) -> Self {
        Self::with_parallelism(
            initial_blocks,
            min_blocks,
            max_blocks,
            network_target_time,
            max_batch_bytes,
            1,
        )
    }

    pub(crate) fn with_parallelism(
        initial_blocks: u64,
        min_blocks: u64,
        max_blocks: u64,
        network_target_time: Duration,
        max_batch_bytes: u64,
        parallel_workers: usize,
    ) -> Self {
        let max_blocks = max_blocks.max(1);
        let min_blocks = min_blocks.max(1).min(max_blocks);
        Self {
            min_blocks,
            max_blocks,
            current_blocks: initial_blocks.clamp(min_blocks, max_blocks),
            network_target_time: network_target_time.max(Duration::from_millis(1)),
            max_batch_bytes: max_batch_bytes.max(1),
            network_ewma_nanos_per_block: None,
            network_samples: 0,
            cached: CachedScanThroughputController::new(parallel_workers),
            last_source: None,
            last_decision: ScanBatchDecision::NetworkLatency,
        }
    }

    /// Returns the current local scan-batch target.
    pub(crate) fn target_blocks(&self) -> u64 {
        self.current_blocks
    }

    pub(crate) fn last_decision(&self) -> ScanBatchDecision {
        self.last_decision
    }

    pub(crate) fn cached_blocks_per_second(&self) -> u64 {
        self.cached.last_blocks_per_second
    }

    pub(crate) fn cached_parallel_saturation_ppm(&self) -> u64 {
        self.cached.last_parallel_saturation_ppm
    }

    /// Records one completed batch and returns the next local target.
    pub(crate) fn observe(&mut self, observation: ScanBatchObservation) -> u64 {
        if self.last_source != Some(observation.source) {
            self.cached.reset();
            self.network_ewma_nanos_per_block = None;
            self.network_samples = 0;
            self.last_source = Some(observation.source);
        }

        if observation.source == ScanBatchSource::Cache {
            let (next, decision) = self.cached.observe(
                self.current_blocks,
                self.min_blocks,
                self.max_blocks,
                observation,
            );
            self.current_blocks = next;
            self.last_decision = decision;
            return self.current_blocks;
        }

        self.last_decision = ScanBatchDecision::NetworkLatency;
        if observation.blocks == 0 || observation.stream_tail {
            self.last_decision = ScanBatchDecision::IgnoredTail;
            return self.current_blocks;
        }

        let sample_nanos = observation
            .processing_time
            .as_nanos()
            .max(1)
            .checked_div(u128::from(observation.blocks))
            .unwrap_or(1)
            .max(1);
        let ewma = match self.network_ewma_nanos_per_block {
            Some(previous) => previous.saturating_mul(7).saturating_add(sample_nanos) / 8,
            None => sample_nanos,
        };
        self.network_ewma_nanos_per_block = Some(ewma);
        self.network_samples = self.network_samples.saturating_add(1);

        // Keep the first two observations stable. They frequently include tree
        // initialization and do not represent sustained scan cost.
        if self.network_samples < 3 {
            return self.current_blocks;
        }

        let target_nanos = self.network_target_time.as_nanos().max(1);
        let mut ideal = target_nanos
            .checked_div(ewma)
            .unwrap_or(u128::from(self.max_blocks))
            .min(u128::from(self.max_blocks)) as u64;

        let avg_encoded_bytes = observation
            .encoded_bytes
            .max(1)
            .div_ceil(observation.blocks.max(1));
        let byte_safe_blocks = (self.max_batch_bytes / avg_encoded_bytes).max(1);
        ideal = ideal.min(byte_safe_blocks);

        // Slow network intake benefits from exposing more producer/consumer
        // overlap. Cached reads deliberately skip this latency heuristic.
        if observation.queued_bytes < self.max_batch_bytes / 4
            && observation.intake_wait > observation.processing_time / 3
        {
            ideal = ideal.min(self.current_blocks.saturating_mul(3) / 4);
        }

        // Damp every adjustment to avoid oscillating under bursty compact-block
        // density, storage latency, or mobile thermal throttling.
        let lower = self
            .current_blocks
            .saturating_mul(3)
            .div_ceil(4)
            .max(self.min_blocks);
        let upper = self
            .current_blocks
            .saturating_mul(5)
            .div_ceil(4)
            .min(self.max_blocks);
        let mut next = ideal.clamp(lower, upper);

        // Quantization reduces allocator churn without making this value
        // observable to the server; network stream segmentation is independent.
        if self.max_blocks.saturating_sub(self.min_blocks) >= 64 {
            next = next.saturating_add(32) / 64 * 64;
        }
        self.current_blocks = next.clamp(self.min_blocks, self.max_blocks);
        self.current_blocks
    }
}

/// High/low-water admission control for already durable prefetched blocks.
///
/// A producer pauses after the high-water boundary and resumes only after the
/// scanner drains to the low-water boundary. Each reservation is released on
/// drop, including cancellation and error paths.
#[derive(Debug)]
pub(crate) struct PrefetchWatermarks {
    high_bytes: u64,
    low_bytes: u64,
    queued_bytes: AtomicU64,
    throttled: AtomicBool,
    changed: Notify,
}

impl PrefetchWatermarks {
    pub(crate) fn new(high_bytes: u64, low_bytes: u64) -> Arc<Self> {
        let high_bytes = high_bytes.max(1);
        Arc::new(Self {
            high_bytes,
            low_bytes: low_bytes.min(high_bytes.saturating_sub(1)),
            queued_bytes: AtomicU64::new(0),
            throttled: AtomicBool::new(false),
            changed: Notify::new(),
        })
    }

    pub(crate) fn queued_bytes(&self) -> u64 {
        self.queued_bytes.load(Ordering::Acquire)
    }

    pub(crate) fn high_bytes(&self) -> u64 {
        self.high_bytes
    }

    /// Largest ordinary segment that can be admitted as soon as the queue
    /// reaches its low-water boundary. A larger segment would require the
    /// queue to drain completely and can deadlock a one-batch lookahead that
    /// still owns an earlier reservation.
    pub(crate) fn segment_admission_bytes(&self) -> u64 {
        self.high_bytes.saturating_sub(self.low_bytes).max(1)
    }

    pub(crate) async fn reserve(
        self: &Arc<Self>,
        encoded_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<PrefetchReservation> {
        self.reserve_charged(encoded_bytes.max(1).min(self.high_bytes), cancel)
            .await
    }

    /// Reserve a durable segment. Multi-block segments are capped to this
    /// charge before admission; an indivisible oversized block uses the same
    /// bounded charge so it cannot wait forever behind a partial local batch.
    pub(crate) async fn reserve_durable_segment(
        self: &Arc<Self>,
        encoded_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<PrefetchReservation> {
        self.reserve_charged(
            encoded_bytes.max(1).min(self.segment_admission_bytes()),
            cancel,
        )
        .await
    }

    async fn reserve_charged(
        self: &Arc<Self>,
        charged_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<PrefetchReservation> {
        loop {
            let notified = self.changed.notified();
            let queued = self.queued_bytes.load(Ordering::Acquire);

            if self.throttled.load(Ordering::Acquire) && queued > self.low_bytes {
                tokio::select! {
                    _ = notified => continue,
                    _ = cancel.cancelled() => return Err(Error::Cancelled),
                }
            }

            if queued <= self.low_bytes {
                self.throttled.store(false, Ordering::Release);
            }

            if queued.saturating_add(charged_bytes) <= self.high_bytes
                && self
                    .queued_bytes
                    .compare_exchange_weak(
                        queued,
                        queued.saturating_add(charged_bytes),
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
            {
                return Ok(PrefetchReservation {
                    _inner: Arc::new(PrefetchReservationInner {
                        owner: Arc::clone(self),
                        charged_bytes,
                    }),
                });
            }

            self.throttled.store(true, Ordering::Release);
            tokio::select! {
                _ = notified => {}
                _ = cancel.cancelled() => return Err(Error::Cancelled),
            }
        }
    }
}

/// RAII reservation for one durable prefetched batch.
#[derive(Clone, Debug)]
pub(crate) struct PrefetchReservation {
    _inner: Arc<PrefetchReservationInner>,
}

#[derive(Debug)]
struct PrefetchReservationInner {
    owner: Arc<PrefetchWatermarks>,
    charged_bytes: u64,
}

impl Drop for PrefetchReservationInner {
    fn drop(&mut self) {
        let previous = self
            .owner
            .queued_bytes
            .fetch_sub(self.charged_bytes, Ordering::AcqRel);
        let remaining = previous.saturating_sub(self.charged_bytes);
        if remaining <= self.owner.low_bytes {
            self.owner.throttled.store(false, Ordering::Release);
            self.owner.changed.notify_waiters();
        } else {
            self.owner.changed.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_weight(blocks: u64, encoded_bytes: u64, shielded_work_items: u64) -> LocalBatchWeight {
        LocalBatchWeight {
            blocks,
            encoded_bytes,
            shielded_work_items,
        }
    }

    #[derive(Clone, Copy)]
    struct SimulatedDevice {
        initial_blocks: u64,
        min_blocks: u64,
        max_blocks: u64,
        high_bytes: u64,
        low_bytes: u64,
        cpu_blocks_per_second: u64,
        storage_bytes_per_second: u64,
        network_bits_per_second: u64,
        network_latency_ms: u64,
        jitter_ms: &'static [u64],
    }

    struct SimulationOutcome {
        scanned_blocks: u64,
        peak_queued_bytes: u64,
        local_batches: Vec<u64>,
        network_segments: Vec<u64>,
    }

    fn encoded_block_bytes(height: u64) -> u64 {
        let ordinary = 160 + height.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 640;
        if height != 0 && height.is_multiple_of(8_191) {
            ordinary + 512 * 1024
        } else {
            ordinary
        }
    }

    fn nanos_for_ratio(units: u64, units_per_second: u64) -> u128 {
        u128::from(units)
            .saturating_mul(1_000_000_000)
            .div_ceil(u128::from(units_per_second.max(1)))
    }

    fn simulation_duration(nanos: u128) -> Duration {
        Duration::from_nanos(nanos.min(u128::from(u64::MAX)) as u64)
    }

    fn simulate_device(profile: SimulatedDevice, total_blocks: u64) -> SimulationOutcome {
        let mut batcher = AdaptiveScanBatcher::new(
            profile.initial_blocks,
            profile.min_blocks,
            profile.max_blocks,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            profile.high_bytes / 2,
        );
        let network_segments = (0..total_blocks)
            .step_by(DEFAULT_DURABLE_SEGMENT_BLOCKS as usize)
            .map(|start| DEFAULT_DURABLE_SEGMENT_BLOCKS.min(total_blocks - start))
            .collect::<Vec<_>>();
        let mut local_batches = Vec::new();
        let mut scanned_blocks = 0u64;
        let mut queued_bytes = 0u64;
        let mut peak_queued_bytes = 0u64;
        let mut throttled = false;
        let mut previous_processing_nanos = 0u128;
        let mut sample = 0usize;

        while scanned_blocks < total_blocks {
            if queued_bytes <= profile.low_bytes {
                throttled = false;
            }
            if !throttled {
                let produced = previous_processing_nanos
                    .saturating_mul(u128::from(profile.network_bits_per_second))
                    / 8
                    / 1_000_000_000;
                queued_bytes = queued_bytes
                    .saturating_add(produced.min(u128::from(u64::MAX)) as u64)
                    .min(profile.high_bytes);
                if queued_bytes >= profile.high_bytes {
                    throttled = true;
                }
            }

            let blocks = batcher
                .target_blocks()
                .min(total_blocks.saturating_sub(scanned_blocks));
            let encoded_bytes = (scanned_blocks..scanned_blocks + blocks)
                .map(encoded_block_bytes)
                .sum::<u64>();
            let missing_bytes = encoded_bytes.saturating_sub(queued_bytes);
            let jitter_ms = profile
                .jitter_ms
                .get(sample % profile.jitter_ms.len().max(1))
                .copied()
                .unwrap_or(0);
            let intake_wait_nanos = if missing_bytes == 0 {
                0
            } else {
                u128::from(profile.network_latency_ms.saturating_add(jitter_ms))
                    .saturating_mul(1_000_000)
                    .saturating_add(nanos_for_ratio(
                        missing_bytes.saturating_mul(8),
                        profile.network_bits_per_second,
                    ))
            };
            queued_bytes = queued_bytes.saturating_add(missing_bytes);
            peak_queued_bytes = peak_queued_bytes.max(queued_bytes);
            queued_bytes = queued_bytes.saturating_sub(encoded_bytes);

            let processing_nanos =
                nanos_for_ratio(blocks, profile.cpu_blocks_per_second).saturating_add(
                    nanos_for_ratio(encoded_bytes, profile.storage_bytes_per_second),
                );
            local_batches.push(blocks);
            scanned_blocks = scanned_blocks.saturating_add(blocks);
            batcher.observe(ScanBatchObservation {
                requested_blocks: blocks,
                requested_bytes: profile.high_bytes / 2,
                blocks,
                encoded_bytes,
                processing_time: simulation_duration(processing_nanos),
                intake_wait: simulation_duration(intake_wait_nanos),
                queued_bytes,
                source: ScanBatchSource::Network,
                tree_parallel_wall: Duration::ZERO,
                tree_parallel_worker_active: Duration::ZERO,
                stream_tail: scanned_blocks == total_blocks,
            });
            previous_processing_nanos = processing_nanos;
            sample += 1;
        }

        SimulationOutcome {
            scanned_blocks,
            peak_queued_bytes,
            local_batches,
            network_segments,
        }
    }

    fn observation(
        blocks: u64,
        processing_ms: u64,
        wait_ms: u64,
        queued_bytes: u64,
    ) -> ScanBatchObservation {
        ScanBatchObservation {
            requested_blocks: blocks,
            requested_bytes: 64 * 1024 * 1024,
            blocks,
            encoded_bytes: blocks.saturating_mul(256),
            processing_time: Duration::from_millis(processing_ms),
            intake_wait: Duration::from_millis(wait_ms),
            queued_bytes,
            source: ScanBatchSource::Network,
            tree_parallel_wall: Duration::ZERO,
            tree_parallel_worker_active: Duration::ZERO,
            stream_tail: false,
        }
    }

    fn cached_observation(
        requested_blocks: u64,
        blocks: u64,
        processing_ms: u64,
        wait_ms: u64,
        tree_wall_ms: u64,
        tree_worker_active_ms: u64,
    ) -> ScanBatchObservation {
        ScanBatchObservation {
            requested_blocks,
            requested_bytes: 64 * 1024 * 1024,
            blocks,
            encoded_bytes: blocks.saturating_mul(256),
            processing_time: Duration::from_millis(processing_ms),
            intake_wait: Duration::from_millis(wait_ms),
            queued_bytes: 0,
            source: ScanBatchSource::Cache,
            tree_parallel_wall: Duration::from_millis(tree_wall_ms),
            tree_parallel_worker_active: Duration::from_millis(tree_worker_active_ms),
            stream_tail: false,
        }
    }

    fn durable_observation(
        blocks: u64,
        network_blocks_per_second: u64,
        cache_blocks_per_second: u64,
        cache_fixed_micros: u64,
    ) -> DurableSegmentObservation {
        let encoded_bytes = blocks.saturating_mul(200);
        let network_nanos = nanos_for_ratio(blocks, network_blocks_per_second);
        let cache_nanos = nanos_for_ratio(blocks, cache_blocks_per_second)
            .saturating_add(u128::from(cache_fixed_micros) * 1_000);
        DurableSegmentObservation {
            blocks,
            encoded_bytes,
            network_wait: simulation_duration(network_nanos),
            cache_write: simulation_duration(cache_nanos),
            queued_bytes: 0,
            high_water_bytes: 64 * 1024 * 1024,
            stream_tail: false,
        }
    }

    fn drive_durable_controller(
        controller: &mut AdaptiveDurableSegmentController,
        samples: usize,
        network_blocks_per_second: u64,
        cache_blocks_per_second: u64,
        cache_fixed_micros: u64,
    ) -> usize {
        let mut changes = 0;
        for _ in 0..samples {
            let before = controller.target_blocks();
            let observation = durable_observation(
                before,
                network_blocks_per_second,
                cache_blocks_per_second,
                cache_fixed_micros,
            );
            let after = controller.observe(observation);
            changes += usize::from(before != after);
        }
        changes
    }

    #[test]
    fn durable_segment_controller_has_small_bounded_state() {
        assert!(std::mem::size_of::<AdaptiveDurableSegmentController>() <= 64);
    }

    #[test]
    fn durable_segment_controller_converges_for_slow_and_fast_streams() {
        let mut slow = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        let slow_changes = drive_durable_controller(&mut slow, 30, 600, 40_000, 20_000);
        assert_eq!(slow.target_blocks(), 512);
        assert!(slow_changes <= 2);

        let mut measured = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        let measured_changes = drive_durable_controller(&mut measured, 30, 915, 40_000, 20_000);
        assert_eq!(measured.target_blocks(), 1_024);
        assert_eq!(measured_changes, 0);

        let mut fast = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        let fast_changes = drive_durable_controller(&mut fast, 60, 100_000, 100_000, 20_000);
        assert!(matches!(fast.target_blocks(), 4_096 | 8_192));
        assert!(fast_changes <= 4);
    }

    #[test]
    fn durable_segment_controller_resists_alternating_bandwidth() {
        let mut controller = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        let mut changes = 0;
        for sample in 0..120 {
            let before = controller.target_blocks();
            let network_rate = if sample % 2 == 0 { 600 } else { 100_000 };
            let after =
                controller.observe(durable_observation(before, network_rate, 80_000, 20_000));
            changes += usize::from(before != after);
        }
        assert!(
            changes <= 3,
            "alternating bandwidth caused {changes} changes"
        );
        assert!(matches!(controller.target_blocks(), 512 | 1_024 | 2_048));
    }

    #[test]
    fn durable_segment_controller_changes_one_bucket_at_a_time() {
        let mut controller = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        let mut previous = controller.current_index;
        for _ in 0..80 {
            let blocks = controller.target_blocks();
            controller.observe(durable_observation(blocks, 1_000_000, 1_000_000, 100));
            assert!(controller.current_index.abs_diff(previous) <= 1);
            previous = controller.current_index;
        }
    }

    #[test]
    fn durable_segment_controller_does_not_grow_under_queue_pressure() {
        let mut controller = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        for _ in 0..30 {
            let blocks = controller.target_blocks();
            let mut observation = durable_observation(blocks, 1_000_000, 1_000_000, 100);
            observation.queued_bytes = 60 * 1024 * 1024;
            controller.observe(observation);
        }
        assert_eq!(controller.target_blocks(), DEFAULT_DURABLE_SEGMENT_BLOCKS);
    }

    #[test]
    fn durable_segment_controller_ignores_short_retry_and_tail_fragments() {
        let mut controller = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        let mut retry = durable_observation(100, 10, 10, 1_000_000);
        assert_eq!(controller.observe(retry), DEFAULT_DURABLE_SEGMENT_BLOCKS);
        retry.stream_tail = true;
        for _ in 0..10 {
            assert_eq!(controller.observe(retry), DEFAULT_DURABLE_SEGMENT_BLOCKS);
        }
    }

    #[test]
    fn shielded_work_controller_reacts_only_to_saturated_batches() {
        let mut controller = AdaptiveShieldedWorkBatcher::new(4);
        assert_eq!(controller.target_items(), 4_096);

        let sparse_target = controller.observe(4_096, 100, Duration::from_secs(5), false);
        assert_eq!(sparse_target, 4_096);

        let slow_target = controller.observe(4_096, 4_096, Duration::from_secs(4), false);
        assert!(slow_target < 4_096);
        let mut recovered = false;
        let mut previous = slow_target;
        for _ in 0..32 {
            let next = controller.observe(previous, previous, Duration::from_millis(20), false);
            recovered |= next > previous;
            previous = next;
        }
        assert!(recovered);
    }

    #[test]
    fn shielded_work_controller_keeps_a_bounded_device_local_target() {
        let mut controller = AdaptiveShieldedWorkBatcher::new(2);
        for _ in 0..100 {
            let target = controller.target_items();
            controller.observe(target, target, Duration::from_secs(30), false);
        }
        assert_eq!(controller.target_items(), 512);

        for _ in 0..100 {
            let target = controller.target_items();
            controller.observe(target, target, Duration::from_millis(1), false);
        }
        assert_eq!(controller.target_items(), 16_384);
    }

    #[test]
    #[ignore = "manual adaptive durable-segment controller benchmark"]
    fn benchmark_adaptive_durable_segment_controller() {
        const OBSERVATIONS: u64 = 10_000_000;
        let mut controller = AdaptiveDurableSegmentController::new(64 * 1024 * 1024);
        let started = std::time::Instant::now();
        for sample in 0..OBSERVATIONS {
            let blocks = std::hint::black_box(controller.target_blocks());
            let network_rate = if sample.is_multiple_of(17) {
                600
            } else {
                100_000
            };
            std::hint::black_box(controller.observe(durable_observation(
                blocks,
                network_rate,
                80_000,
                20_000,
            )));
        }
        let elapsed = started.elapsed();
        println!(
            "adaptive durable segment controller: state={} bytes, observations={OBSERVATIONS}, elapsed={:.3}s, {:.1} ns/observation",
            std::mem::size_of::<AdaptiveDurableSegmentController>(),
            elapsed.as_secs_f64(),
            elapsed.as_nanos() as f64 / OBSERVATIONS as f64
        );
    }

    #[test]
    fn adaptive_batcher_converges_without_leaving_profile_bounds() {
        let mut batcher = AdaptiveScanBatcher::new(
            6_000,
            100,
            16_000,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
        );
        for _ in 0..12 {
            batcher.observe(observation(batcher.target_blocks(), 900, 400, 0));
        }
        assert!(batcher.target_blocks() < 6_000);
        assert!(batcher.target_blocks() >= 100);

        for _ in 0..20 {
            batcher.observe(observation(
                batcher.target_blocks(),
                40,
                0,
                32 * 1024 * 1024,
            ));
        }
        assert!(batcher.target_blocks() > 100);
        assert!(batcher.target_blocks() <= 16_000);
    }

    #[test]
    fn final_short_batch_does_not_distort_the_controller() {
        let mut batcher = AdaptiveScanBatcher::new(
            4_000,
            100,
            16_000,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
        );
        let mut tail = observation(10, 2_000, 1_000, 0);
        tail.stream_tail = true;
        assert_eq!(batcher.observe(tail), 4_000);
    }

    #[test]
    fn cache_wait_does_not_collapse_the_local_scan_target() {
        let mut batcher = AdaptiveScanBatcher::new(
            8_000,
            128,
            16_000,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
        );

        for _ in 0..24 {
            let target = batcher.target_blocks();
            batcher.observe(cached_observation(target, target, 200, 700, 100, 400));
        }

        assert!(batcher.target_blocks() >= 8_000);
    }

    #[test]
    fn cached_latency_does_not_shrink_a_saturated_profile_ceiling() {
        let mut batcher = AdaptiveScanBatcher::with_parallelism(
            16_000,
            128,
            16_000,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
            16,
        );

        let mut minimum_target = batcher.target_blocks();
        let mut rejected_smaller_probe = false;
        for _ in 0..12 {
            let target = batcher.target_blocks();
            minimum_target = minimum_target.min(target);
            batcher.observe(cached_observation(target, target, 900, 700, 600, 7_200));
            rejected_smaller_probe |= batcher.last_decision() == ScanBatchDecision::CacheRegression;
        }

        assert_eq!(minimum_target, 12_800);
        assert_eq!(batcher.target_blocks(), 16_000);
        assert!(rejected_smaller_probe);
        assert!(batcher.cached_blocks_per_second() > 0);
        assert_eq!(batcher.cached_parallel_saturation_ppm(), 750_000);
    }

    #[test]
    fn cached_controller_accepts_parallel_saturation_then_rejects_a_plateau() {
        let mut batcher = AdaptiveScanBatcher::with_parallelism(
            1_024,
            128,
            8_192,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
            8,
        );

        for _ in 0..CACHED_SCAN_CONFIRMATIONS {
            batcher.observe(cached_observation(1_024, 1_024, 100, 0, 100, 200));
        }
        assert_eq!(batcher.target_blocks(), 1_280);
        assert_eq!(batcher.last_decision(), ScanBatchDecision::CacheProbeLarger);

        // Throughput is unchanged, but the larger range keeps more tree workers
        // occupied, so retaining the larger target improves parallel efficiency.
        for _ in 0..CACHED_SCAN_CONFIRMATIONS {
            batcher.observe(cached_observation(1_280, 1_280, 125, 0, 100, 300));
        }
        assert_eq!(batcher.target_blocks(), 1_600);
        assert_eq!(batcher.last_decision(), ScanBatchDecision::CacheProbeLarger);

        // A further probe improves neither throughput nor worker occupancy and
        // therefore returns to the last proven operating point.
        for _ in 0..CACHED_SCAN_CONFIRMATIONS {
            batcher.observe(cached_observation(1_600, 1_600, 157, 0, 100, 300));
        }
        assert_eq!(batcher.target_blocks(), 1_280);
        assert_eq!(batcher.last_decision(), ScanBatchDecision::CachePlateau);
    }

    #[test]
    fn exact_byte_limited_cache_batches_do_not_reduce_the_block_target() {
        let mut batcher = AdaptiveScanBatcher::with_parallelism(
            8_000,
            128,
            16_000,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            8 * 1024 * 1024,
            8,
        );

        for _ in 0..12 {
            let target = batcher.target_blocks();
            let mut sample = cached_observation(target, 1_000, 100, 0, 80, 320);
            sample.requested_bytes = 8 * 1024 * 1024;
            sample.encoded_bytes = sample.requested_bytes;
            batcher.observe(sample);
        }

        assert!(batcher.target_blocks() >= 8_000);
    }

    #[test]
    fn stale_prefetched_cache_batch_does_not_change_the_controller() {
        let mut batcher = AdaptiveScanBatcher::with_parallelism(
            8_000,
            128,
            16_000,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
            8,
        );
        let sample = cached_observation(6_000, 6_000, 200, 0, 100, 400);

        assert_eq!(batcher.observe(sample), 8_000);
        assert_eq!(batcher.last_decision(), ScanBatchDecision::CacheStaleSample);
    }

    #[test]
    fn changing_sources_resets_cached_probe_history() {
        let mut batcher = AdaptiveScanBatcher::with_parallelism(
            1_024,
            128,
            8_192,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
            8,
        );
        for _ in 0..CACHED_SCAN_CONFIRMATIONS {
            batcher.observe(cached_observation(1_024, 1_024, 100, 0, 100, 200));
        }
        assert_eq!(batcher.target_blocks(), 1_280);

        batcher.observe(observation(1_280, 125, 0, 0));
        batcher.observe(cached_observation(1_280, 1_280, 125, 0, 100, 300));

        assert_eq!(batcher.target_blocks(), 1_280);
        assert_eq!(batcher.last_decision(), ScanBatchDecision::CacheCollecting);
    }

    #[test]
    fn cached_throughput_controller_has_small_bounded_state() {
        assert!(std::mem::size_of::<CachedScanThroughputController>() <= 192);
    }

    #[test]
    #[ignore = "manual cached-throughput controller benchmark"]
    fn benchmark_cached_throughput_controller() {
        const OBSERVATIONS: u64 = 10_000_000;
        let mut batcher = AdaptiveScanBatcher::with_parallelism(
            16_000,
            128,
            16_000,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            64 * 1024 * 1024,
            16,
        );
        let started = std::time::Instant::now();
        for _ in 0..OBSERVATIONS {
            let target = std::hint::black_box(batcher.target_blocks());
            std::hint::black_box(
                batcher.observe(cached_observation(target, target, 600, 0, 400, 4_800)),
            );
        }
        let elapsed = started.elapsed();
        println!(
            "cached throughput controller: state={} bytes, observations={OBSERVATIONS}, elapsed={:.3}s, {:.1} ns/observation",
            std::mem::size_of::<CachedScanThroughputController>(),
            elapsed.as_secs_f64(),
            elapsed.as_nanos() as f64 / OBSERVATIONS as f64
        );
    }

    #[test]
    fn local_batches_split_and_merge_standard_segments_without_overshoot() {
        assert_eq!(
            local_batch_prefix_len(
                &[10; 1_024],
                &[0; 1_024],
                local_weight(0, 0, 0),
                local_weight(1_000, 20_000, u64::MAX),
            ),
            1_000
        );
        assert_eq!(
            local_batch_prefix_len(
                &[10; 24],
                &[0; 24],
                local_weight(1_000, 10_000, 0),
                local_weight(1_000, 20_000, u64::MAX),
            ),
            0
        );
        assert_eq!(
            local_batch_prefix_len(
                &[10; 1_024],
                &[0; 1_024],
                local_weight(0, 0, 0),
                local_weight(4_000, 20_000, u64::MAX),
            ),
            1_024
        );
        assert_eq!(
            local_batch_prefix_len(
                &[10; 1_024],
                &[0; 1_024],
                local_weight(1_024, 10_240, 0),
                local_weight(4_000, 20_000, u64::MAX),
            ),
            976
        );
    }

    #[test]
    fn local_byte_limit_allows_only_an_oversized_block_to_exceed_it() {
        assert_eq!(
            local_batch_prefix_len(
                &[60, 60],
                &[0, 0],
                local_weight(0, 0, 0),
                local_weight(10, 100, u64::MAX),
            ),
            1
        );
        assert_eq!(
            local_batch_prefix_len(
                &[150, 10],
                &[0, 0],
                local_weight(0, 0, 0),
                local_weight(10, 100, u64::MAX),
            ),
            1
        );
        assert_eq!(
            local_batch_prefix_len(
                &[10],
                &[0],
                local_weight(1, 150, 0),
                local_weight(10, 100, u64::MAX),
            ),
            0
        );
    }

    #[test]
    fn local_work_limit_splits_dense_blocks_and_admits_one_oversized_block() {
        assert_eq!(
            local_batch_prefix_len(
                &[10; 4],
                &[30, 30, 30, 30],
                local_weight(0, 0, 0),
                local_weight(10, 1_000, 75),
            ),
            2
        );
        assert_eq!(
            local_batch_prefix_len(
                &[10; 2],
                &[100, 1],
                local_weight(0, 0, 0),
                local_weight(10, 1_000, 75),
            ),
            1
        );
        assert_eq!(
            local_batch_prefix_len(
                &[10],
                &[1],
                local_weight(1, 10, 75),
                local_weight(10, 1_000, 75),
            ),
            0
        );
    }

    #[test]
    fn device_and_transport_profiles_keep_network_segments_standardized() {
        const MB: u64 = 1024 * 1024;
        let profiles = [
            SimulatedDevice {
                initial_blocks: 500,
                min_blocks: 10,
                max_blocks: 1_000,
                high_bytes: 8 * MB,
                low_bytes: 4 * MB,
                cpu_blocks_per_second: 2_000,
                storage_bytes_per_second: 12 * MB,
                network_bits_per_second: 12_000_000,
                network_latency_ms: 80,
                jitter_ms: &[0],
            },
            SimulatedDevice {
                initial_blocks: 6_000,
                min_blocks: 100,
                max_blocks: 16_000,
                high_bytes: 256 * MB,
                low_bytes: 128 * MB,
                cpu_blocks_per_second: 45_000,
                storage_bytes_per_second: 800 * MB,
                network_bits_per_second: 1_000_000_000,
                network_latency_ms: 8,
                jitter_ms: &[0],
            },
            SimulatedDevice {
                initial_blocks: 4_000,
                min_blocks: 50,
                max_blocks: 4_000,
                high_bytes: 32 * MB,
                low_bytes: 16 * MB,
                cpu_blocks_per_second: 12_000,
                storage_bytes_per_second: 80 * MB,
                network_bits_per_second: 6_000_000,
                network_latency_ms: 45,
                jitter_ms: &[0],
            },
            SimulatedDevice {
                initial_blocks: 2_000,
                min_blocks: 25,
                max_blocks: 2_000,
                high_bytes: 16 * MB,
                low_bytes: 8 * MB,
                cpu_blocks_per_second: 7_500,
                storage_bytes_per_second: 35 * MB,
                network_bits_per_second: 20_000_000,
                network_latency_ms: 280,
                jitter_ms: &[0, 40, 0, 350, 20, 700, 0, 90],
            },
        ];

        let outcomes = profiles
            .into_iter()
            .map(|profile| simulate_device(profile, 65_536))
            .collect::<Vec<_>>();
        let expected_segments = outcomes[0].network_segments.clone();
        for (profile, outcome) in profiles.into_iter().zip(&outcomes) {
            assert_eq!(outcome.scanned_blocks, 65_536);
            assert_eq!(outcome.network_segments, expected_segments);
            assert!(outcome.peak_queued_bytes <= profile.high_bytes);
            assert!(outcome
                .local_batches
                .iter()
                .all(|blocks| *blocks > 0 && *blocks <= profile.max_blocks));
        }
        assert_ne!(outcomes[0].local_batches, outcomes[1].local_batches);
        assert_ne!(outcomes[1].local_batches, outcomes[3].local_batches);
    }

    #[tokio::test]
    async fn rollback_discards_admitted_prefetch_before_canonical_restart() {
        let watermarks = PrefetchWatermarks::new(128, 48);
        let cancel = CancelToken::new();
        let orphan_a = watermarks.reserve(40, &cancel).await.unwrap();
        let orphan_b = watermarks.reserve(40, &cancel).await.unwrap();
        assert_eq!(watermarks.queued_bytes(), 80);

        drop((orphan_a, orphan_b));
        assert_eq!(watermarks.queued_bytes(), 0);

        let canonical = watermarks.reserve(96, &cancel).await.unwrap();
        assert_eq!(watermarks.queued_bytes(), 96);
        drop(canonical);
        assert_eq!(watermarks.queued_bytes(), 0);
    }

    #[tokio::test]
    async fn split_segment_reservation_is_released_after_its_last_batch() {
        let watermarks = PrefetchWatermarks::new(128, 48);
        let reservation = watermarks.reserve(80, &CancelToken::new()).await.unwrap();
        let first_batch = reservation.clone();
        let second_batch = reservation.clone();
        drop(reservation);
        drop(first_batch);
        assert_eq!(watermarks.queued_bytes(), 80);
        drop(second_batch);
        assert_eq!(watermarks.queued_bytes(), 0);
    }

    #[tokio::test]
    async fn watermarks_pause_at_high_and_resume_below_low() {
        let watermarks = PrefetchWatermarks::new(100, 40);
        let cancel = CancelToken::new();
        let first = watermarks.reserve(70, &cancel).await.unwrap();
        assert_eq!(watermarks.queued_bytes(), 70);

        let waiting = {
            let watermarks = Arc::clone(&watermarks);
            let cancel = cancel.clone();
            tokio::spawn(async move { watermarks.reserve(50, &cancel).await })
        };
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(first);
        let second = waiting.await.unwrap().unwrap();
        assert_eq!(watermarks.queued_bytes(), 50);
        drop(second);
        assert_eq!(watermarks.queued_bytes(), 0);
    }

    #[tokio::test]
    async fn durable_segment_fits_at_the_low_water_boundary() {
        let watermarks = PrefetchWatermarks::new(100, 40);
        assert_eq!(watermarks.segment_admission_bytes(), 60);
        let held = watermarks.reserve(40, &CancelToken::new()).await.unwrap();
        let segment = watermarks
            .reserve_durable_segment(1_000, &CancelToken::new())
            .await
            .unwrap();
        assert_eq!(watermarks.queued_bytes(), 100);
        drop((held, segment));
        assert_eq!(watermarks.queued_bytes(), 0);
    }

    #[tokio::test]
    async fn cancelled_watermark_wait_does_not_leak_capacity() {
        let watermarks = PrefetchWatermarks::new(64, 32);
        let held = watermarks.reserve(64, &CancelToken::new()).await.unwrap();
        let cancel = CancelToken::new();
        let waiting = {
            let watermarks = Arc::clone(&watermarks);
            let cancel = cancel.clone();
            tokio::spawn(async move { watermarks.reserve(1, &cancel).await })
        };
        cancel.cancel();
        assert!(matches!(waiting.await.unwrap(), Err(Error::Cancelled)));
        drop(held);
        assert_eq!(watermarks.queued_bytes(), 0);
    }
}
