use super::*;
use incrementalmerkletree::{frontier::CommitmentTree, Address, Level};
use shardtree::store::{Checkpoint as ShardCheckpoint, ShardStore};
use shardtree::{LocatedPrunableTree, Node, PrunableTree};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;
use std::rc::Rc;

const SHARD_LEAF_COUNT: u64 = 1u64 << SAPLING_SHARD_HEIGHT;
const SUBTREE_ROOT_SAMPLE_INTERVAL: u64 = 16;
const MIN_PARALLEL_TREE_CHUNK_LEAVES: usize = 256;
const MAX_PARALLEL_TREE_CHUNK_LEAVES: usize = 4_096;
const TARGET_TREE_CHUNKS_PER_THREAD: usize = 2;

#[derive(Clone)]
pub(super) struct HistoricalSubtreeRoot<H> {
    expected_end_height: u64,
    expected_root: Option<H>,
    sample_anchor: Option<u64>,
    trusted: bool,
}

struct HistoricalSubtreeBuffer<H> {
    subtree_index: u64,
    expected_end_height: u64,
    expected_root: Option<H>,
    verify_sample: bool,
    root_persisted: bool,
    leaves_emitted: bool,
    sample_tree: Option<CommitmentTree<H, SAPLING_SHARD_HEIGHT>>,
    sample_leaf_count: u64,
    buffered_leaves: Vec<(u64, u64, H, Retention<BlockHeight>)>,
}

pub(super) struct HistoricalSubtreeSkipState<H> {
    pub(super) roots_by_index: HashMap<u64, HistoricalSubtreeRoot<H>>,
    current_buffer: Option<HistoricalSubtreeBuffer<H>>,
    passthrough_subtree: Option<u64>,
    verified_samples: HashSet<u64>,
    pending_roots: Vec<(u64, u64, H)>,
    grafting_disabled: bool,
    pool_name: &'static str,
    leaf_backed_hints: HashSet<u64>,
}

impl<H> HistoricalSubtreeSkipState<H> {
    pub(super) fn new(roots_by_index: HashMap<u64, HistoricalSubtreeRoot<H>>) -> Self {
        Self {
            roots_by_index,
            current_buffer: None,
            passthrough_subtree: None,
            verified_samples: HashSet::new(),
            pending_roots: Vec::new(),
            grafting_disabled: false,
            pool_name: "unknown",
            leaf_backed_hints: HashSet::new(),
        }
    }

    pub(super) fn with_leaf_backed_hints(
        mut self,
        pool_name: &'static str,
        hints: &HashSet<u64>,
    ) -> Self {
        self.pool_name = pool_name;
        self.leaf_backed_hints.extend(hints.iter().copied());
        self
    }

    fn has_deferred_leaves(&self) -> bool {
        self.current_buffer
            .as_ref()
            .is_some_and(|buffer| !buffer.leaves_emitted && !buffer.root_persisted)
    }
}

pub(super) struct HistoricalPrefillState {
    pub(super) sapling: HistoricalSubtreeSkipState<SaplingNode>,
    pub(super) orchard: HistoricalSubtreeSkipState<MerkleHashOrchard>,
    pub(super) sapling_prefetched: usize,
    pub(super) orchard_prefetched: usize,
}

pub(super) struct HistoricalSubtreeRootRequest {
    start_sapling_index: u32,
    start_orchard_index: u32,
    historical_ceiling: u64,
    fetch_sapling: bool,
    fetch_ironwood: bool,
}

impl HistoricalSubtreeRootRequest {
    pub(super) fn retain_capabilities(
        &mut self,
        fetch_sapling: bool,
        fetch_ironwood: bool,
    ) -> bool {
        self.fetch_sapling &= fetch_sapling;
        self.fetch_ironwood &= fetch_ironwood;
        self.fetch_sapling || self.fetch_ironwood
    }

    pub(super) fn requested_pools(&self) -> (bool, bool) {
        (self.fetch_sapling, self.fetch_ironwood)
    }
}

#[derive(Default)]
pub(super) struct RemoteHistoricalSubtreeRoots {
    sapling: HashMap<u64, HistoricalSubtreeRoot<SaplingNode>>,
    ironwood: HashMap<u64, HistoricalSubtreeRoot<MerkleHashOrchard>>,
}

#[derive(Clone)]
pub(super) struct VerifiedSubtreeRoot<H> {
    pub(super) index: u64,
    pub(super) end_height: u64,
    pub(super) root: H,
}

#[derive(Clone, Default)]
pub(super) struct VerifiedSubtreeRoots {
    pub(super) sapling: Vec<VerifiedSubtreeRoot<SaplingNode>>,
    pub(super) ironwood: Vec<VerifiedSubtreeRoot<MerkleHashOrchard>>,
}

impl VerifiedSubtreeRoots {
    pub(super) fn is_empty(&self) -> bool {
        self.sapling.is_empty() && self.ironwood.is_empty()
    }

    pub(super) fn counts(&self) -> (usize, usize) {
        (self.sapling.len(), self.ironwood.len())
    }
}

impl HistoricalPrefillState {
    pub(super) fn prefetched_any(&self) -> bool {
        self.sapling_prefetched > 0 || self.orchard_prefetched > 0
    }

    pub(super) fn merge_remote_roots(&mut self, remote: RemoteHistoricalSubtreeRoots) {
        self.sapling_prefetched = self.sapling_prefetched.saturating_add(remote.sapling.len());
        self.orchard_prefetched = self
            .orchard_prefetched
            .saturating_add(remote.ironwood.len());
        for (index, root) in remote.sapling {
            self.sapling.roots_by_index.entry(index).or_insert(root);
        }
        for (index, root) in remote.ironwood {
            self.orchard.roots_by_index.entry(index).or_insert(root);
        }
    }

    pub(super) fn sapling_checkpoint_safe(&self) -> bool {
        !self.sapling.has_deferred_leaves()
    }

    pub(super) fn orchard_checkpoint_safe(&self) -> bool {
        !self.orchard.has_deferred_leaves()
    }

    pub(super) fn common_checkpoint_safe(&self) -> bool {
        self.sapling_checkpoint_safe() && self.orchard_checkpoint_safe()
    }

    pub(super) fn pending_verified_roots(&self) -> VerifiedSubtreeRoots {
        let mut sapling = if self.sapling.grafting_disabled {
            Vec::new()
        } else {
            self.sapling
                .pending_roots
                .iter()
                .map(|(index, end_height, root)| VerifiedSubtreeRoot {
                    index: *index,
                    end_height: *end_height,
                    root: *root,
                })
                .collect::<Vec<_>>()
        };
        let mut ironwood = if self.orchard.grafting_disabled {
            Vec::new()
        } else {
            self.orchard
                .pending_roots
                .iter()
                .map(|(index, end_height, root)| VerifiedSubtreeRoot {
                    index: *index,
                    end_height: *end_height,
                    root: *root,
                })
                .collect::<Vec<_>>()
        };
        sapling.sort_by_key(|root| root.index);
        ironwood.sort_by_key(|root| root.index);
        VerifiedSubtreeRoots { sapling, ironwood }
    }

    pub(super) fn mark_verified_roots_persisted(&mut self, persisted: &VerifiedSubtreeRoots) {
        mark_pool_roots_persisted(&mut self.sapling, &persisted.sapling);
        mark_pool_roots_persisted(&mut self.orchard, &persisted.ironwood);
    }
}

fn mark_pool_roots_persisted<H: Clone>(
    state: &mut HistoricalSubtreeSkipState<H>,
    persisted: &[VerifiedSubtreeRoot<H>],
) {
    let persisted_indices = persisted
        .iter()
        .map(|root| root.index)
        .collect::<BTreeSet<_>>();
    state
        .pending_roots
        .retain(|(index, _, _)| !persisted_indices.contains(index));
    for index in persisted_indices {
        if let Some(root) = state.roots_by_index.get_mut(&index) {
            root.trusted = true;
        }
    }
}

type WarmSaplingStore<'a> = CachingShardStore<
    SqliteShardStore<&'a rusqlite::Connection, SaplingNode, SAPLING_SHARD_HEIGHT>,
>;
type WarmOrchardStore<'a> = CachingShardStore<
    SqliteShardStore<&'a rusqlite::Connection, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>,
>;

pub(super) struct SyncWarmTrees<'a> {
    sapling_tree:
        ShardTree<WarmSaplingStore<'a>, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT>,
    orchard_tree:
        ShardTree<WarmOrchardStore<'a>, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT>,
    dirty: bool,
}

impl<'a> SyncWarmTrees<'a> {
    pub(super) fn load(conn: &'a rusqlite::Connection) -> Result<Self> {
        let sapling_backend =
            SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                conn,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open warm Sapling shard store: {}", e)))?;
        let orchard_backend =
            SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                conn,
                ORCHARD_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open warm Orchard shard store: {}", e)))?;
        let sapling_store = CachingShardStore::load(sapling_backend)
            .map_err(|e| Error::Sync(format!("Failed to load warm Sapling cache: {}", e)))?;
        let orchard_store = CachingShardStore::load(orchard_backend)
            .map_err(|e| Error::Sync(format!("Failed to load warm Orchard cache: {}", e)))?;

        Ok(Self {
            sapling_tree: ShardTree::new(sapling_store, SHARDTREE_PRUNING_DEPTH),
            orchard_tree: ShardTree::new(orchard_store, SHARDTREE_PRUNING_DEPTH),
            dirty: false,
        })
    }

    pub(super) fn persist_batches(
        &mut self,
        batches: &[ShardtreeBatch],
        batch_end_height: Option<u64>,
    ) -> Result<ShardtreePersistResult> {
        let result = apply_shardtree_batches_to_trees(
            &mut self.sapling_tree,
            &mut self.orchard_tree,
            batches,
            batch_end_height,
            None,
        )?;
        if !batches.is_empty() {
            self.dirty = true;
        }
        Ok(result)
    }

    pub(super) fn checkpoint_tip(&mut self, checkpoint_id: BlockHeight) -> Result<bool> {
        let sapling = self
            .sapling_tree
            .checkpoint(checkpoint_id)
            .map_err(|e| Error::Sync(format!("Failed warm Sapling checkpoint: {}", e)))?;
        let orchard = self
            .orchard_tree
            .checkpoint(checkpoint_id)
            .map_err(|e| Error::Sync(format!("Failed warm Orchard checkpoint: {}", e)))?;
        let changed = sapling || orchard;
        if changed {
            self.dirty = true;
        }
        Ok(changed)
    }

    pub(super) fn flush_and_reload(self, conn: &'a rusqlite::Connection) -> Result<Self> {
        let SyncWarmTrees {
            sapling_tree,
            orchard_tree,
            dirty,
        } = self;
        if dirty {
            sapling_tree
                .into_store()
                .flush()
                .map_err(|e| Error::Sync(format!("Failed to flush warm Sapling tree: {}", e)))?;
            orchard_tree
                .into_store()
                .flush()
                .map_err(|e| Error::Sync(format!("Failed to flush warm Orchard tree: {}", e)))?;
        }
        Self::load(conn)
    }
}

pub(super) fn warm_shardtree_cache_with_subtrees_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(
        || match env::var("PIRATE_ENABLE_WARM_SHARDTREE_CACHE_WITH_SUBTREES") {
            Ok(v) => {
                let v = v.trim();
                v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
            }
            Err(_) => false,
        },
    )
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ShardtreePersistResult {
    pub(super) max_checkpointed_height: Option<u64>,
    pub(super) batch_end_checkpointed: bool,
    pub(super) sapling_work: ShardtreePoolWork,
    pub(super) ironwood_work: ShardtreePoolWork,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub(super) struct CommittedCheckpointHeights {
    pub(super) sapling: Option<u32>,
    pub(super) ironwood: Option<u32>,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ShardtreePoolWork {
    pub(super) commitment_count: u64,
    pub(super) commitment_insert: Duration,
    pub(super) parallel_construction: Duration,
    pub(super) parallel_worker_active: Duration,
    pub(super) prepared_tree_insert: Duration,
    pub(super) prepared_tree_count: u64,
    pub(super) checkpoint_count: u64,
    pub(super) checkpoint_processing: Duration,
}

#[derive(Debug, Default, Clone)]
pub(super) struct ShardtreeBatch {
    pub(super) height: u64,
    pub(super) checkpoint_id: Option<BlockHeight>,
    pub(super) sapling_empty_checkpoint: bool,
    pub(super) orchard_empty_checkpoint: bool,
    pub(super) sapling_start_position: Option<Position>,
    pub(super) orchard_start_position: Option<Position>,
    pub(super) sapling: Vec<(SaplingNode, Retention<BlockHeight>)>,
    pub(super) orchard: Vec<(MerkleHashOrchard, Retention<BlockHeight>)>,
}

impl ShardtreeBatch {
    pub(super) fn new(height: u64) -> Self {
        Self {
            height,
            checkpoint_id: None,
            sapling_empty_checkpoint: false,
            orchard_empty_checkpoint: false,
            sapling_start_position: None,
            orchard_start_position: None,
            sapling: Vec::new(),
            orchard: Vec::new(),
        }
    }
}

struct OwnedPoolBatch<H> {
    height: u64,
    empty_checkpoint: bool,
    start_position: Option<Position>,
    leaves: Vec<(H, Retention<BlockHeight>)>,
}

struct PreparedTreeInsertion<H> {
    start_position: Position,
    subtree: LocatedPrunableTree<H>,
    checkpoints: BTreeMap<BlockHeight, Position>,
    worker_active: Duration,
}

enum PreparedPoolOperation<H> {
    Insert {
        trees: Vec<PreparedTreeInsertion<H>>,
        commitment_count: u64,
        construction: Duration,
    },
    VerifiedRoot(VerifiedSubtreeRoot<H>),
    EmptyCheckpoint(BlockHeight),
}

struct PreparedPoolInsertions<H> {
    operations: Vec<PreparedPoolOperation<H>>,
}

pub(super) struct PreparedShardtreeInsertions {
    result: ShardtreePersistResult,
    sapling: PreparedPoolInsertions<SaplingNode>,
    ironwood: PreparedPoolInsertions<MerkleHashOrchard>,
}

fn insert_verified_pool_root<S, H, const DEPTH: u8, const SHARD_HEIGHT: u8>(
    tree: &mut ShardTree<S, DEPTH, SHARD_HEIGHT>,
    root: &VerifiedSubtreeRoot<H>,
    pool_name: &str,
) -> Result<()>
where
    S: ShardStore<H = H, CheckpointId = BlockHeight>,
    H: Hashable + Clone + PartialEq,
    S::Error: std::fmt::Display,
{
    tree.insert(
        Address::from_parts(SHARD_HEIGHT.into(), root.index),
        root.root.clone(),
    )
    .map_err(|error| {
        Error::Sync(format!(
            "Failed to stage verified {} subtree root {}: {}",
            pool_name, root.index, error
        ))
    })
}

fn pool_batch_has_checkpoint<H>(
    leaves: &[(H, Retention<BlockHeight>)],
    empty_checkpoint: bool,
    checkpoint_id: BlockHeight,
) -> bool {
    empty_checkpoint
        || leaves.last().is_some_and(|(_, retention)| {
            matches!(
                retention,
                Retention::Checkpoint { id, .. } if *id == checkpoint_id
            )
        })
}

fn summarize_shardtree_batches(
    batches: &[ShardtreeBatch],
    batch_end_height: Option<u64>,
    max_committed_heights: CommittedCheckpointHeights,
) -> Result<ShardtreePersistResult> {
    let mut result = ShardtreePersistResult::default();
    for batch in batches {
        let checkpoint_height = u32::try_from(batch.height).map_err(|_| {
            Error::Sync(format!(
                "Checkpoint height {} exceeds u32::MAX",
                batch.height
            ))
        })?;
        let sapling_committed = max_committed_heights
            .sapling
            .is_some_and(|height| checkpoint_height <= height);
        let ironwood_committed = max_committed_heights
            .ironwood
            .is_some_and(|height| checkpoint_height <= height);
        if sapling_committed && ironwood_committed {
            continue;
        }
        if let Some(checkpoint_id) = batch.checkpoint_id {
            result.max_checkpointed_height = Some(
                result
                    .max_checkpointed_height
                    .map_or(batch.height, |current| current.max(batch.height)),
            );
            if batch_end_height == Some(batch.height) {
                result.batch_end_checkpointed = pool_batch_has_checkpoint(
                    &batch.sapling,
                    batch.sapling_empty_checkpoint,
                    checkpoint_id,
                ) && pool_batch_has_checkpoint(
                    &batch.orchard,
                    batch.orchard_empty_checkpoint,
                    checkpoint_id,
                );
            }
        }
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConstructionRange {
    start: u64,
    len: usize,
}

fn adaptive_tree_chunk_limit(total_leaves: usize, parallelism: usize) -> usize {
    if total_leaves == 0 {
        return 0;
    }

    let useful_tasks = total_leaves.div_ceil(MIN_PARALLEL_TREE_CHUNK_LEAVES).max(1);
    let target_tasks = parallelism
        .max(1)
        .saturating_mul(TARGET_TREE_CHUNKS_PER_THREAD)
        .min(useful_tasks)
        .max(1);
    total_leaves
        .div_ceil(target_tasks)
        .max(1)
        .checked_next_power_of_two()
        .unwrap_or(MAX_PARALLEL_TREE_CHUNK_LEAVES)
        .clamp(
            MIN_PARALLEL_TREE_CHUNK_LEAVES,
            MAX_PARALLEL_TREE_CHUNK_LEAVES,
        )
}

fn balanced_construction_ranges<const SHARD_HEIGHT: u8>(
    start_position: Position,
    total_leaves: usize,
    parallelism: usize,
    pool_name: &'static str,
) -> Result<Vec<ConstructionRange>> {
    if total_leaves == 0 {
        return Ok(Vec::new());
    }

    let shard_leaf_count = 1u64 << SHARD_HEIGHT;
    let chunk_limit = adaptive_tree_chunk_limit(total_leaves, parallelism);
    let mut ranges = Vec::with_capacity(total_leaves.div_ceil(chunk_limit).saturating_add(2));
    let mut next_position = u64::from(start_position);
    let mut remaining = total_leaves;

    while remaining > 0 {
        let shard_end = next_position
            .checked_div(shard_leaf_count)
            .and_then(|index| index.checked_add(1))
            .and_then(|index| index.checked_mul(shard_leaf_count))
            .ok_or_else(|| {
                Error::Sync(format!(
                    "{} commitment position overflow while planning ShardTree construction",
                    pool_name
                ))
            })?;
        let max_take = remaining
            .min(chunk_limit)
            .min((shard_end - next_position) as usize);
        let mut take = 1usize;
        while take <= max_take / 2 && next_position % (take.saturating_mul(2) as u64) == 0 {
            take = take.saturating_mul(2);
        }
        ranges.push(ConstructionRange {
            start: next_position,
            len: take,
        });
        next_position = next_position.checked_add(take as u64).ok_or_else(|| {
            Error::Sync(format!(
                "{} commitment position overflow while planning ShardTree construction",
                pool_name
            ))
        })?;
        remaining -= take;
    }

    Ok(ranges)
}

fn push_prepared_run<H, const SHARD_HEIGHT: u8>(
    operations: &mut Vec<PreparedPoolOperation<H>>,
    run_start: &mut Option<Position>,
    run_leaves: &mut Vec<(H, Retention<BlockHeight>)>,
    parallelism: usize,
    pool_name: &'static str,
) -> Result<()>
where
    H: Hashable + Clone + PartialEq + Send + Sync,
{
    let Some(start_position) = run_start.take() else {
        return Ok(());
    };
    let leaves = std::mem::take(run_leaves);
    if leaves.is_empty() {
        return Ok(());
    }

    let commitment_count = leaves.len() as u64;
    let ranges = balanced_construction_ranges::<SHARD_HEIGHT>(
        start_position,
        leaves.len(),
        parallelism,
        pool_name,
    )?;
    let mut chunks = Vec::with_capacity(ranges.len());
    let mut remaining = leaves.into_iter();
    for range in ranges {
        let chunk_leaves = remaining.by_ref().take(range.len).collect::<Vec<_>>();
        if chunk_leaves.len() != range.len {
            return Err(Error::Sync(format!(
                "{} adaptive ShardTree plan did not consume its complete commitment range",
                pool_name
            )));
        }
        chunks.push((Position::from(range.start), chunk_leaves));
    }
    debug_assert_eq!(remaining.len(), 0);

    let construction_start = Instant::now();
    let prepared = chunks
        .into_par_iter()
        .map(|(chunk_start, chunk_leaves)| {
            let worker_started = Instant::now();
            let chunk_len = chunk_leaves.len() as u64;
            let chunk_end = u64::from(chunk_start)
                .checked_add(chunk_len)
                .ok_or_else(|| {
                    Error::Sync(format!(
                        "{} commitment range overflow while constructing ShardTree fragment",
                        pool_name
                    ))
                })?;
            let result = LocatedPrunableTree::<H>::from_iter(
                chunk_start..Position::from(chunk_end),
                Level::from(SHARD_HEIGHT),
                chunk_leaves.into_iter(),
            )
            .ok_or_else(|| {
                Error::Sync(format!(
                    "{} parallel ShardTree construction produced no fragment",
                    pool_name
                ))
            })?;
            if result.remainder.len() != 0
                || u64::from(result.max_insert_position).saturating_add(1) != chunk_end
            {
                return Err(Error::Sync(format!(
                    "{} parallel ShardTree construction did not consume its complete range",
                    pool_name
                )));
            }
            Ok(PreparedTreeInsertion {
                start_position: chunk_start,
                subtree: result.subtree,
                checkpoints: result.checkpoints,
                worker_active: worker_started.elapsed(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let construction = construction_start.elapsed();
    operations.push(PreparedPoolOperation::Insert {
        trees: prepared,
        commitment_count,
        construction,
    });
    Ok(())
}

fn prepare_pool_insertions<H, const SHARD_HEIGHT: u8>(
    batches: Vec<OwnedPoolBatch<H>>,
    max_committed_height: Option<u32>,
    mut verified_roots: Vec<VerifiedSubtreeRoot<H>>,
    parallelism: usize,
    pool_name: &'static str,
) -> Result<PreparedPoolInsertions<H>>
where
    H: Hashable + Clone + PartialEq + Send + Sync,
{
    verified_roots.sort_by_key(|root| (root.end_height, root.index));
    let mut verified_roots = VecDeque::from(verified_roots);
    let mut operations = Vec::new();
    let mut run_start = None;
    let mut run_leaves = Vec::new();

    for batch in batches {
        let checkpoint_height = u32::try_from(batch.height).map_err(|_| {
            Error::Sync(format!(
                "Checkpoint height {} exceeds u32::MAX",
                batch.height
            ))
        })?;
        if max_committed_height.is_some_and(|height| checkpoint_height <= height) {
            push_prepared_run::<H, SHARD_HEIGHT>(
                &mut operations,
                &mut run_start,
                &mut run_leaves,
                parallelism,
                pool_name,
            )?;
            while verified_roots
                .front()
                .is_some_and(|root| root.end_height <= batch.height)
            {
                verified_roots.pop_front();
            }
            continue;
        }

        while verified_roots
            .front()
            .is_some_and(|root| root.end_height <= batch.height)
        {
            push_prepared_run::<H, SHARD_HEIGHT>(
                &mut operations,
                &mut run_start,
                &mut run_leaves,
                parallelism,
                pool_name,
            )?;
            operations.push(PreparedPoolOperation::VerifiedRoot(
                verified_roots
                    .pop_front()
                    .expect("verified root was present"),
            ));
        }

        if !batch.leaves.is_empty() {
            let start_position = batch.start_position.ok_or_else(|| {
                Error::Sync(format!(
                    "Missing {} start position for shardtree batch at height {}",
                    pool_name, batch.height
                ))
            })?;
            let expected_start = run_start.map(|start| {
                Position::from(u64::from(start).saturating_add(run_leaves.len() as u64))
            });
            if expected_start.is_some_and(|expected| expected != start_position) {
                push_prepared_run::<H, SHARD_HEIGHT>(
                    &mut operations,
                    &mut run_start,
                    &mut run_leaves,
                    parallelism,
                    pool_name,
                )?;
            }
            if run_start.is_none() {
                run_start = Some(start_position);
            }
            run_leaves.extend(batch.leaves);
        }

        if batch.empty_checkpoint {
            push_prepared_run::<H, SHARD_HEIGHT>(
                &mut operations,
                &mut run_start,
                &mut run_leaves,
                parallelism,
                pool_name,
            )?;
            operations.push(PreparedPoolOperation::EmptyCheckpoint(BlockHeight::from(
                checkpoint_height,
            )));
        }
    }

    push_prepared_run::<H, SHARD_HEIGHT>(
        &mut operations,
        &mut run_start,
        &mut run_leaves,
        parallelism,
        pool_name,
    )?;
    if let Some(root) = verified_roots.front() {
        return Err(Error::Sync(format!(
            "Verified {} subtree root {} completed at height {} outside the persisted block batch",
            pool_name, root.index, root.end_height
        )));
    }
    Ok(PreparedPoolInsertions { operations })
}

pub(super) fn prepare_parallel_shardtree_insertions(
    construction_pool: &rayon::ThreadPool,
    batches: Vec<ShardtreeBatch>,
    batch_end_height: Option<u64>,
    max_committed_heights: CommittedCheckpointHeights,
    verified_roots: &VerifiedSubtreeRoots,
) -> Result<PreparedShardtreeInsertions> {
    let result = summarize_shardtree_batches(&batches, batch_end_height, max_committed_heights)?;
    let mut sapling_batches = Vec::with_capacity(batches.len());
    let mut ironwood_batches = Vec::with_capacity(batches.len());
    for batch in batches {
        sapling_batches.push(OwnedPoolBatch {
            height: batch.height,
            empty_checkpoint: batch.sapling_empty_checkpoint,
            start_position: batch.sapling_start_position,
            leaves: batch.sapling,
        });
        ironwood_batches.push(OwnedPoolBatch {
            height: batch.height,
            empty_checkpoint: batch.orchard_empty_checkpoint,
            start_position: batch.orchard_start_position,
            leaves: batch.orchard,
        });
    }
    let sapling_roots = verified_roots.sapling.clone();
    let ironwood_roots = verified_roots.ironwood.clone();
    let parallelism = construction_pool.current_num_threads().max(1);
    let (sapling, ironwood) = construction_pool.install(|| {
        rayon::join(
            || {
                prepare_pool_insertions::<SaplingNode, SAPLING_SHARD_HEIGHT>(
                    sapling_batches,
                    max_committed_heights.sapling,
                    sapling_roots,
                    parallelism,
                    "Sapling",
                )
            },
            || {
                prepare_pool_insertions::<MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>(
                    ironwood_batches,
                    max_committed_heights.ironwood,
                    ironwood_roots,
                    parallelism,
                    "Ironwood",
                )
            },
        )
    });
    Ok(PreparedShardtreeInsertions {
        result,
        sapling: sapling?,
        ironwood: ironwood?,
    })
}

fn apply_prepared_pool_insertions<S, H, const DEPTH: u8, const SHARD_HEIGHT: u8>(
    tree: &mut ShardTree<S, DEPTH, SHARD_HEIGHT>,
    prepared: PreparedPoolInsertions<H>,
    pool_name: &str,
) -> Result<ShardtreePoolWork>
where
    S: ShardStore<H = H, CheckpointId = BlockHeight>,
    S::Error: std::fmt::Display,
    H: Hashable + Clone + PartialEq,
{
    let mut telemetry = ShardtreePoolWork::default();
    for operation in prepared.operations {
        match operation {
            PreparedPoolOperation::Insert {
                mut trees,
                commitment_count,
                construction,
            } => {
                trees.sort_by_key(|tree| u64::from(tree.start_position));
                let worker_active = trees
                    .iter()
                    .map(|tree| tree.worker_active)
                    .sum::<Duration>();
                let insert_start = Instant::now();
                for prepared_tree in trees {
                    tree.insert_tree(prepared_tree.subtree, prepared_tree.checkpoints)
                        .map_err(|error| {
                            Error::Sync(format!(
                                "Failed to insert prepared {} ShardTree fragment: {}",
                                pool_name, error
                            ))
                        })?;
                    telemetry.prepared_tree_count = telemetry.prepared_tree_count.saturating_add(1);
                }
                let insert_elapsed = insert_start.elapsed();
                telemetry.parallel_construction += construction;
                telemetry.parallel_worker_active += worker_active;
                telemetry.prepared_tree_insert += insert_elapsed;
                telemetry.commitment_insert += construction + insert_elapsed;
                telemetry.commitment_count =
                    telemetry.commitment_count.saturating_add(commitment_count);
            }
            PreparedPoolOperation::VerifiedRoot(root) => {
                insert_verified_pool_root(tree, &root, pool_name)?;
            }
            PreparedPoolOperation::EmptyCheckpoint(checkpoint_id) => {
                let checkpoint_start = Instant::now();
                tree.checkpoint(checkpoint_id).map_err(|error| {
                    Error::Sync(format!(
                        "Failed to checkpoint {} shardtree: {}",
                        pool_name, error
                    ))
                })?;
                telemetry.checkpoint_processing += checkpoint_start.elapsed();
                telemetry.checkpoint_count = telemetry.checkpoint_count.saturating_add(1);
            }
        }
    }
    Ok(telemetry)
}

pub(super) fn apply_prepared_shardtree_insertions_to_trees<SS, OS>(
    sapling_tree: &mut ShardTree<SS, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT>,
    ironwood_tree: &mut ShardTree<OS, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT>,
    prepared: PreparedShardtreeInsertions,
) -> Result<ShardtreePersistResult>
where
    SS: ShardStore<H = SaplingNode, CheckpointId = BlockHeight>,
    OS: ShardStore<H = MerkleHashOrchard, CheckpointId = BlockHeight>,
    SS::Error: std::fmt::Display,
    OS::Error: std::fmt::Display,
{
    let mut result = prepared.result;
    result.sapling_work =
        apply_prepared_pool_insertions(&mut *sapling_tree, prepared.sapling, "Sapling")?;
    result.ironwood_work =
        apply_prepared_pool_insertions(&mut *ironwood_tree, prepared.ironwood, "Ironwood")?;
    Ok(result)
}

pub(super) fn apply_shardtree_batches_to_trees<SS, OS>(
    sapling_tree: &mut ShardTree<SS, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT>,
    orchard_tree: &mut ShardTree<OS, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT>,
    batches: &[ShardtreeBatch],
    batch_end_height: Option<u64>,
    max_committed_height: Option<u32>,
) -> Result<ShardtreePersistResult>
where
    SS: shardtree::store::ShardStore<H = SaplingNode, CheckpointId = BlockHeight>,
    OS: shardtree::store::ShardStore<H = MerkleHashOrchard, CheckpointId = BlockHeight>,
    SS::Error: std::fmt::Display,
    OS::Error: std::fmt::Display,
{
    let mut result = ShardtreePersistResult::default();
    for batch in batches {
        let checkpoint_height = u32::try_from(batch.height).map_err(|_| {
            Error::Sync(format!(
                "Checkpoint height {} exceeds u32::MAX",
                batch.height
            ))
        })?;

        if let Some(max_h) = max_committed_height {
            if checkpoint_height <= max_h {
                tracing::debug!(
                    "Skipping already-committed block {} (max checkpoint={})",
                    batch.height,
                    max_h
                );
                continue;
            }
        }

        let checkpoint_id = BlockHeight::from(checkpoint_height);
        if !batch.sapling.is_empty() {
            let start_position = batch.sapling_start_position.ok_or_else(|| {
                Error::Sync(format!(
                    "Missing Sapling start position for shardtree batch at height {}",
                    batch.height
                ))
            })?;
            sapling_tree
                .batch_insert(start_position, batch.sapling.iter().cloned())
                .map_err(|e| {
                    Error::Sync(format!(
                        "Failed to batch insert Sapling commitments into shardtree: {}",
                        e
                    ))
                })?;
        }
        if batch.sapling_empty_checkpoint {
            sapling_tree.checkpoint(checkpoint_id).map_err(|e| {
                Error::Sync(format!("Failed to checkpoint Sapling shardtree: {}", e))
            })?;
        }
        if !batch.orchard.is_empty() {
            let start_position = batch.orchard_start_position.ok_or_else(|| {
                Error::Sync(format!(
                    "Missing Orchard start position for shardtree batch at height {}",
                    batch.height
                ))
            })?;
            orchard_tree
                .batch_insert(start_position, batch.orchard.iter().cloned())
                .map_err(|e| {
                    Error::Sync(format!(
                        "Failed to batch insert Orchard commitments into shardtree: {}",
                        e
                    ))
                })?;
        }
        if batch.orchard_empty_checkpoint {
            orchard_tree.checkpoint(checkpoint_id).map_err(|e| {
                Error::Sync(format!("Failed to checkpoint Orchard shardtree: {}", e))
            })?;
        }
        if batch.checkpoint_id.is_some() {
            result.max_checkpointed_height = Some(
                result
                    .max_checkpointed_height
                    .map_or(batch.height, |current| current.max(batch.height)),
            );
            if batch_end_height == Some(batch.height) {
                result.batch_end_checkpointed = true;
            }
        }
    }

    Ok(result)
}

pub(super) fn append_sapling_leaf(
    batch: &mut ShardtreeBatch,
    position: u64,
    node: SaplingNode,
    retention: Retention<BlockHeight>,
) {
    if batch.sapling.is_empty() {
        batch.sapling_start_position = Some(Position::from(position));
    }
    batch.sapling.push((node, retention));
}

pub(super) fn append_orchard_leaf(
    batch: &mut ShardtreeBatch,
    position: u64,
    node: MerkleHashOrchard,
    retention: Retention<BlockHeight>,
) {
    if batch.orchard.is_empty() {
        batch.orchard_start_position = Some(Position::from(position));
    }
    batch.orchard.push((node, retention));
}

fn flush_buffered_pool_leaves<H>(
    buffer: HistoricalSubtreeBuffer<H>,
    current_batch: &mut ShardtreeBatch,
    shardtree_batches: &mut Vec<ShardtreeBatch>,
    mut append_leaf: impl FnMut(&mut ShardtreeBatch, u64, H, Retention<BlockHeight>),
) {
    for (block_height, position, node, retention) in buffer.buffered_leaves {
        if block_height == current_batch.height {
            append_leaf(current_batch, position, node, retention);
        } else if let Some(last) = shardtree_batches.last_mut() {
            if last.height == block_height {
                append_leaf(last, position, node, retention);
            } else {
                let mut batch = ShardtreeBatch::new(block_height);
                append_leaf(&mut batch, position, node, retention);
                shardtree_batches.push(batch);
            }
        } else {
            let mut batch = ShardtreeBatch::new(block_height);
            append_leaf(&mut batch, position, node, retention);
            shardtree_batches.push(batch);
        }
    }
}

pub(super) fn merge_emitted_batches(
    target: &mut Vec<ShardtreeBatch>,
    mut emitted: Vec<ShardtreeBatch>,
) {
    for mut batch in emitted.drain(..) {
        if let Some(last) = target.last_mut() {
            if last.height == batch.height {
                last.sapling.append(&mut batch.sapling);
                last.orchard.append(&mut batch.orchard);
                continue;
            }
        }
        target.push(batch);
    }
}

pub(super) fn drain_historical_skip_state<H>(
    state: &mut HistoricalSubtreeSkipState<H>,
    append_leaf: impl FnMut(&mut ShardtreeBatch, u64, H, Retention<BlockHeight>) + Copy,
) -> Vec<ShardtreeBatch> {
    let mut emitted = Vec::new();
    if let Some(buffer) = state.current_buffer.take() {
        let mut dummy_current = ShardtreeBatch::new(u64::MAX);
        flush_buffered_pool_leaves(buffer, &mut dummy_current, &mut emitted, append_leaf);
        if dummy_current.height != u64::MAX {
            emitted.push(dummy_current);
        }
    }
    state.passthrough_subtree = None;
    emitted
}

pub(super) struct HistoricalLeafSink<'a> {
    pub(super) current_batch: &'a mut ShardtreeBatch,
    pub(super) shardtree_batches: &'a mut Vec<ShardtreeBatch>,
}

pub(super) fn process_historical_leaf<H>(
    state: Option<&mut HistoricalSubtreeSkipState<H>>,
    position: u64,
    block_height: u64,
    node: H,
    retention: Retention<BlockHeight>,
    sink: HistoricalLeafSink<'_>,
    append_leaf: impl FnMut(&mut ShardtreeBatch, u64, H, Retention<BlockHeight>) + Copy,
) {
    let HistoricalLeafSink {
        current_batch,
        shardtree_batches,
    } = sink;
    let Some(state) = state else {
        let mut append_leaf = append_leaf;
        append_leaf(current_batch, position, node, retention);
        return;
    };

    let subtree_index = position / SHARD_LEAF_COUNT;
    let subtree_offset = position % SHARD_LEAF_COUNT;
    let subtree_start = subtree_offset == 0;
    let subtree_end = subtree_offset + 1 == SHARD_LEAF_COUNT;

    if let Some(active_passthrough) = state.passthrough_subtree {
        if active_passthrough == subtree_index {
            let mut append_leaf = append_leaf;
            append_leaf(current_batch, position, node, retention);
            if subtree_end {
                state.passthrough_subtree = None;
            }
            return;
        }
        state.passthrough_subtree = None;
    }

    if let Some(buffer) = state.current_buffer.as_mut() {
        if buffer.subtree_index == subtree_index {
            if retention.is_marked() {
                let flushed = state.current_buffer.take().expect("buffer exists");
                flush_buffered_pool_leaves(flushed, current_batch, shardtree_batches, append_leaf);
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if !subtree_end {
                    state.passthrough_subtree = Some(subtree_index);
                }
                return;
            }
            buffer
                .buffered_leaves
                .push((block_height, position, node, retention));
            if subtree_end {
                let completed = state.current_buffer.take().expect("buffer exists");
                if completed.expected_end_height != block_height {
                    flush_buffered_pool_leaves(
                        completed,
                        current_batch,
                        shardtree_batches,
                        append_leaf,
                    );
                }
            }
            return;
        }

        let flushed = state.current_buffer.take().expect("buffer exists");
        flush_buffered_pool_leaves(flushed, current_batch, shardtree_batches, append_leaf);
    }

    if subtree_start {
        if let Some(expected_end_height) = state.roots_by_index.get(&subtree_index).copied() {
            if retention.is_marked() {
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if !subtree_end {
                    state.passthrough_subtree = Some(subtree_index);
                }
            } else {
                let buffer = HistoricalSubtreeBuffer {
                    subtree_index,
                    expected_end_height,
                    buffered_leaves: vec![(block_height, position, node, retention)],
                };
                if subtree_end {
                    if expected_end_height != block_height {
                        flush_buffered_pool_leaves(
                            buffer,
                            current_batch,
                            shardtree_batches,
                            append_leaf,
                        );
                    }
                } else {
                    state.current_buffer = Some(buffer);
                }
            }
            return;
        }
    }

    let mut append_leaf = append_leaf;
    append_leaf(current_batch, position, node, retention);
}

fn load_root_backed_subtree_index(
    conn: &rusqlite::Connection,
    table_prefix: &'static str,
    max_end_height: u64,
) -> Result<HashMap<u64, u64>> {
    let max_end_height = i64::try_from(max_end_height).map_err(|_| {
        Error::Sync(format!(
            "subtree max end height {} exceeds i64",
            max_end_height
        ))
    })?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT shard_index, subtree_end_height
             FROM {}_tree_shards
             WHERE subtree_end_height IS NOT NULL
               AND subtree_end_height <= ?1",
            table_prefix
        ))
        .map_err(|e| {
            Error::Sync(format!(
                "Failed to query {} subtree index: {}",
                table_prefix, e
            ))
        })?;
    let mut rows = stmt.query([max_end_height]).map_err(|e| {
        Error::Sync(format!(
            "Failed to iterate {} subtree index: {}",
            table_prefix, e
        ))
    })?;
    let mut roots = HashMap::new();
    while let Some(row) = rows.next().map_err(|e| {
        Error::Sync(format!(
            "Failed to read {} subtree index row: {}",
            table_prefix, e
        ))
    })? {
        let shard_index: i64 = row.get(0).map_err(|e| {
            Error::Sync(format!(
                "Failed to decode {} shard index: {}",
                table_prefix, e
            ))
        })?;
        let subtree_end_height: i64 = row.get(1).map_err(|e| {
            Error::Sync(format!(
                "Failed to decode {} subtree height: {}",
                table_prefix, e
            ))
        })?;
        if let (Ok(shard_index_u64), Ok(end_height_u64)) = (
            u64::try_from(shard_index),
            u64::try_from(subtree_end_height),
        ) {
            roots.insert(shard_index_u64, end_height_u64);
        }
    }
    Ok(roots)
}

fn parse_subtree_root_hash<H: HashSer>(bytes: &[u8]) -> Result<H> {
    H::read(Cursor::new(bytes))
        .map_err(|e| Error::Sync(format!("Failed to parse subtree root hash: {}", e)))
}

async fn fetch_and_store_subtree_roots<H: HashSer + Hashable + Clone + Eq>(
    client: &LightClient,
    conn: &rusqlite::Connection,
    table_prefix: &'static str,
    protocol: crate::proto_types::ShieldedProtocol,
    start_index: u32,
    max_end_height: u64,
) -> Result<usize> {
    let roots = client.get_subtree_roots(start_index, protocol, 0).await?;
    let mut parsed: Vec<PersistedSubtreeRoot<H>> = Vec::new();
    for root in roots {
        if root.completing_block_height > max_end_height {
            break;
        }
        let parsed_hash = parse_subtree_root_hash::<H>(&root.root_hash)?;
        parsed.push(PersistedSubtreeRoot::new(
            BlockHeight::from(u32::try_from(root.completing_block_height).map_err(|_| {
                Error::Sync(format!(
                    "subtree completing height {} exceeds u32",
                    root.completing_block_height
                ))
            })?),
            parsed_hash,
        ));
    }
    if parsed.is_empty() {
        return Ok(0);
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| Error::Sync(format!("Failed to start subtree-root transaction: {}", e)))?;
    put_shard_roots::<H, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT>(
        &tx,
        table_prefix,
        u64::from(start_index),
        &parsed,
    )
    .map_err(|e| {
        Error::Sync(format!(
            "Failed to persist {} subtree roots: {}",
            table_prefix, e
        ))
    })?;
    tx.commit().map_err(|e| {
        Error::Sync(format!(
            "Failed to commit {} subtree roots: {}",
            table_prefix, e
        ))
    })?;
    Ok(parsed.len())
}

pub(super) async fn prefill_historical_subtree_roots(
    client: &LightClient,
    conn: &rusqlite::Connection,
    sapling_position: u64,
    orchard_position: u64,
    end_height: u64,
) -> Result<HistoricalPrefillState> {
    let historical_ceiling = end_height.saturating_sub(SHARDTREE_PRUNING_DEPTH as u64);
    if historical_ceiling == 0 {
        append_sync_decision_log(
            "sync.rs:prefill_historical_subtree_roots",
            "subtree-root prefill skipped",
            "\"reason\":\"no_historical_range\",\"historical_ceiling\":0".to_string(),
        );
        return Ok(HistoricalPrefillState {
            sapling: HistoricalSubtreeSkipState::new(HashMap::new()),
            orchard: HistoricalSubtreeSkipState::new(HashMap::new()),
            sapling_prefetched: 0,
            orchard_prefetched: 0,
        });
    }

    let start_sapling_index = sapling_position.div_ceil(SHARD_LEAF_COUNT) as u32;
    let start_orchard_index = orchard_position.div_ceil(SHARD_LEAF_COUNT) as u32;
    let mut sapling_prefetched = 0usize;
    let mut orchard_prefetched = 0usize;

    match fetch_and_store_subtree_roots::<SaplingNode>(
        client,
        conn,
        SAPLING_TABLE_PREFIX,
        crate::proto_types::ShieldedProtocol::Sapling,
        start_sapling_index,
        historical_ceiling,
    )
    .await
    {
        Ok(count) => sapling_prefetched = count,
        Err(e) => {
            tracing::warn!(
                "Historical Sapling subtree-root prefill unavailable; continuing with leaf sync: {}",
                e
            );
            append_sync_decision_log(
                "sync.rs:prefill_historical_subtree_roots",
                "subtree-root prefill unavailable, falling back",
                format!(
                    "\"pool\":\"sapling\",\"start_index\":{},\"historical_ceiling\":{},\"error\":\"{}\"",
                    start_sapling_index,
                    historical_ceiling,
                    format!("{}", e).replace('"', "'")
                ),
            );
        }
    }
    match fetch_and_store_subtree_roots::<MerkleHashOrchard>(
        client,
        conn,
        ORCHARD_TABLE_PREFIX,
        crate::proto_types::ShieldedProtocol::Ironwood,
        start_orchard_index,
        historical_ceiling,
    )
    .await
    {
        Ok(count) => orchard_prefetched = count,
        Err(e) => {
            tracing::warn!(
                "Historical Orchard subtree-root prefill unavailable; continuing with leaf sync: {}",
                e
            );
            append_sync_decision_log(
                "sync.rs:prefill_historical_subtree_roots",
                "subtree-root prefill unavailable, falling back",
                format!(
                    "\"pool\":\"orchard\",\"start_index\":{},\"historical_ceiling\":{},\"error\":\"{}\"",
                    start_orchard_index,
                    historical_ceiling,
                    format!("{}", e).replace('"', "'")
                ),
            );
        }
    }

    let sapling_roots_by_index =
        load_root_backed_subtree_index(conn, SAPLING_TABLE_PREFIX, historical_ceiling)?;
    let orchard_roots_by_index =
        load_root_backed_subtree_index(conn, ORCHARD_TABLE_PREFIX, historical_ceiling)?;

    tracing::info!(
        "Historical subtree-root prefill: sapling_prefetched={}, orchard_prefetched={}, sapling_available={}, orchard_available={}, sapling_start_index={}, orchard_start_index={}, historical_ceiling={}",
        sapling_prefetched,
        orchard_prefetched,
        sapling_roots_by_index.len(),
        orchard_roots_by_index.len(),
        start_sapling_index,
        start_orchard_index,
        historical_ceiling
    );
    append_sync_decision_log(
        "sync.rs:prefill_historical_subtree_roots",
        "subtree-root prefill summary",
        format!(
            "\"sapling_prefetched\":{},\"orchard_prefetched\":{},\"sapling_available\":{},\"orchard_available\":{},\"sapling_start_index\":{},\"orchard_start_index\":{},\"historical_ceiling\":{}",
            sapling_prefetched,
            orchard_prefetched,
            sapling_roots_by_index.len(),
            orchard_roots_by_index.len(),
            start_sapling_index,
            start_orchard_index,
            historical_ceiling
        ),
    );

    Ok(HistoricalPrefillState {
        sapling: HistoricalSubtreeSkipState::new(sapling_roots_by_index),
        orchard: HistoricalSubtreeSkipState::new(orchard_roots_by_index),
        sapling_prefetched,
        orchard_prefetched,
    })
}
