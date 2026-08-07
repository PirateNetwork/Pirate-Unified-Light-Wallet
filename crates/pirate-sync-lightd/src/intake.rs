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

    pub(crate) async fn reserve(
        self: &Arc<Self>,
        encoded_bytes: u64,
        cancel: &CancelToken,
    ) -> Result<PrefetchReservation> {
        let charged_bytes = encoded_bytes.max(1).min(self.high_bytes);
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
