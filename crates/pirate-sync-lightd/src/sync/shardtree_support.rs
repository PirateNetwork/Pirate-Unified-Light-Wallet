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
        self.persist_batches_with_roots(batches, batch_end_height, &VerifiedSubtreeRoots::default())
    }

    pub(super) fn persist_batches_with_roots(
        &mut self,
        batches: &[ShardtreeBatch],
        batch_end_height: Option<u64>,
        verified_roots: &VerifiedSubtreeRoots,
    ) -> Result<ShardtreePersistResult> {
        let result = apply_shardtree_batches_to_trees(
            &mut self.sapling_tree,
            &mut self.orchard_tree,
            batches,
            batch_end_height,
            CommittedCheckpointHeights::default(),
            verified_roots,
        )?;
        if !batches.is_empty() || !verified_roots.is_empty() {
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

    pub(super) fn retain_checkpoint(&mut self, checkpoint_id: BlockHeight) -> Result<()> {
        self.sapling_tree
            .ensure_retained(checkpoint_id)
            .map_err(|e| Error::Sync(format!("Failed to retain warm Sapling checkpoint: {}", e)))?;
        self.orchard_tree
            .ensure_retained(checkpoint_id)
            .map_err(|e| Error::Sync(format!("Failed to retain warm Orchard checkpoint: {}", e)))?;
        self.dirty = true;
        Ok(())
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
    max_committed_heights: CommittedCheckpointHeights,
    verified_roots: &VerifiedSubtreeRoots,
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

        let sapling_committed = max_committed_heights
            .sapling
            .is_some_and(|height| checkpoint_height <= height);
        let ironwood_committed = max_committed_heights
            .ironwood
            .is_some_and(|height| checkpoint_height <= height);
        if sapling_committed && ironwood_committed {
            tracing::debug!(
                "Skipping block {} already committed by both shielded pools",
                batch.height
            );
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

    result.sapling_work = apply_pool_batches(
        sapling_tree,
        batches,
        max_committed_heights.sapling,
        &verified_roots.sapling,
        |batch| {
            (
                batch.sapling_start_position,
                batch.sapling.as_slice(),
                batch.sapling_empty_checkpoint,
            )
        },
        "Sapling",
    )?;
    result.ironwood_work = apply_pool_batches(
        orchard_tree,
        batches,
        max_committed_heights.ironwood,
        &verified_roots.ironwood,
        |batch| {
            (
                batch.orchard_start_position,
                batch.orchard.as_slice(),
                batch.orchard_empty_checkpoint,
            )
        },
        "Orchard",
    )?;

    Ok(result)
}

pub(super) fn sparse_preload_addresses<S, const SHARD_HEIGHT: u8>(
    store: &S,
    pool_name: &str,
) -> Result<Vec<Address>>
where
    S: shardtree::store::ShardStore<CheckpointId = BlockHeight>,
    S::Error: std::fmt::Display,
{
    let mut addresses = std::collections::BTreeSet::new();
    if let Some(frontier) = store.last_shard().map_err(|e| {
        Error::Sync(format!(
            "Failed to read {} frontier shard: {}",
            pool_name, e
        ))
    })? {
        addresses.insert(frontier.root_addr());
    }

    let checkpoint_count = store
        .checkpoint_count()
        .map_err(|e| Error::Sync(format!("Failed to count {} checkpoints: {}", pool_name, e)))?;
    store
        .for_each_checkpoint(checkpoint_count, |_, checkpoint| {
            let mut retain_position = |position: Position| {
                addresses.insert(Address::from_parts(
                    SHARD_HEIGHT.into(),
                    u64::from(position) >> SHARD_HEIGHT,
                ));
            };
            if let shardtree::store::TreeState::AtPosition(position) = checkpoint.tree_state() {
                retain_position(position);
            }
            for position in checkpoint.marks_removed() {
                retain_position(*position);
            }
            Ok(())
        })
        .map_err(|e| {
            Error::Sync(format!(
                "Failed to inspect {} checkpoint working set: {}",
                pool_name, e
            ))
        })?;

    Ok(addresses.into_iter().collect())
}

enum BufferedShardAction<H, C> {
    PutShard(LocatedPrunableTree<H>),
    TruncateShards(u64),
    PutCap(PrunableTree<H>),
    AddCheckpoint(C, ShardCheckpoint),
    UpdateCheckpoint(C, ShardCheckpoint),
    RemoveCheckpoint(C),
    AddRetainedCheckpoint(C),
    RemoveRetainedCheckpoint(C),
    TruncateCheckpointsRetaining(C),
}

type BufferedActions<H, C> = Rc<RefCell<Vec<BufferedShardAction<H, C>>>>;

struct BufferedShardStore<S>
where
    S: ShardStore,
{
    backend: S,
    pending: BufferedActions<S::H, S::CheckpointId>,
}

impl<S> BufferedShardStore<S>
where
    S: ShardStore,
{
    fn new(backend: S, pending: BufferedActions<S::H, S::CheckpointId>) -> Self {
        Self { backend, pending }
    }

    fn push(&self, action: BufferedShardAction<S::H, S::CheckpointId>) {
        self.pending.borrow_mut().push(action);
    }
}

impl<S> ShardStore for BufferedShardStore<S>
where
    S: ShardStore,
    S::H: Clone,
    S::CheckpointId: Clone + Ord,
{
    type H = S::H;
    type CheckpointId = S::CheckpointId;
    type Error = S::Error;

    fn get_shard(
        &self,
        shard_root: Address,
    ) -> std::result::Result<Option<LocatedPrunableTree<Self::H>>, Self::Error> {
        self.backend.get_shard(shard_root)
    }

    fn last_shard(&self) -> std::result::Result<Option<LocatedPrunableTree<Self::H>>, Self::Error> {
        self.backend.last_shard()
    }

    fn put_shard(
        &mut self,
        subtree: LocatedPrunableTree<Self::H>,
    ) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::PutShard(subtree));
        Ok(())
    }

    fn get_shard_roots(&self) -> std::result::Result<Vec<Address>, Self::Error> {
        self.backend.get_shard_roots()
    }

    fn truncate_shards(&mut self, shard_index: u64) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::TruncateShards(shard_index));
        Ok(())
    }

    fn get_cap(&self) -> std::result::Result<PrunableTree<Self::H>, Self::Error> {
        self.backend.get_cap()
    }

    fn put_cap(&mut self, cap: PrunableTree<Self::H>) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::PutCap(cap));
        Ok(())
    }

    fn min_checkpoint_id(&self) -> std::result::Result<Option<Self::CheckpointId>, Self::Error> {
        self.backend.min_checkpoint_id()
    }

    fn max_checkpoint_id(&self) -> std::result::Result<Option<Self::CheckpointId>, Self::Error> {
        self.backend.max_checkpoint_id()
    }

    fn add_checkpoint(
        &mut self,
        checkpoint_id: Self::CheckpointId,
        checkpoint: ShardCheckpoint,
    ) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::AddCheckpoint(
            checkpoint_id,
            checkpoint,
        ));
        Ok(())
    }

    fn checkpoint_count(&self) -> std::result::Result<usize, Self::Error> {
        self.backend.checkpoint_count()
    }

    fn get_checkpoint_at_depth(
        &self,
        checkpoint_depth: usize,
    ) -> std::result::Result<Option<(Self::CheckpointId, ShardCheckpoint)>, Self::Error> {
        self.backend.get_checkpoint_at_depth(checkpoint_depth)
    }

    fn get_checkpoint(
        &self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<Option<ShardCheckpoint>, Self::Error> {
        self.backend.get_checkpoint(checkpoint_id)
    }

    fn with_checkpoints<F>(
        &mut self,
        limit: usize,
        callback: F,
    ) -> std::result::Result<(), Self::Error>
    where
        F: FnMut(&Self::CheckpointId, &ShardCheckpoint) -> std::result::Result<(), Self::Error>,
    {
        self.backend.with_checkpoints(limit, callback)
    }

    fn for_each_checkpoint<F>(
        &self,
        limit: usize,
        callback: F,
    ) -> std::result::Result<(), Self::Error>
    where
        F: FnMut(&Self::CheckpointId, &ShardCheckpoint) -> std::result::Result<(), Self::Error>,
    {
        self.backend.for_each_checkpoint(limit, callback)
    }

    fn update_checkpoint_with<F>(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
        update: F,
    ) -> std::result::Result<bool, Self::Error>
    where
        F: Fn(&mut ShardCheckpoint) -> std::result::Result<(), Self::Error>,
    {
        let Some(mut checkpoint) = self.backend.get_checkpoint(checkpoint_id)? else {
            return Ok(false);
        };
        update(&mut checkpoint)?;
        self.push(BufferedShardAction::UpdateCheckpoint(
            checkpoint_id.clone(),
            checkpoint,
        ));
        Ok(true)
    }

    fn remove_checkpoint(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::RemoveCheckpoint(checkpoint_id.clone()));
        Ok(())
    }

    fn add_retained_checkpoint(
        &mut self,
        checkpoint_id: Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::AddRetainedCheckpoint(checkpoint_id));
        Ok(())
    }

    fn remove_retained_checkpoint(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::RemoveRetainedCheckpoint(
            checkpoint_id.clone(),
        ));
        Ok(())
    }

    fn retained_checkpoints(
        &self,
    ) -> std::result::Result<BTreeSet<Self::CheckpointId>, Self::Error> {
        self.backend.retained_checkpoints()
    }

    fn truncate_checkpoints_retaining(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.push(BufferedShardAction::TruncateCheckpointsRetaining(
            checkpoint_id.clone(),
        ));
        Ok(())
    }
}

fn tree_heap_bytes<H>(tree: &PrunableTree<H>) -> u64 {
    match &**tree {
        Node::Parent { ann, left, right } => {
            let annotation = if ann.is_some() {
                (size_of::<H>() + 2 * size_of::<usize>()) as u64
            } else {
                0
            };
            annotation
                .saturating_add((4 * size_of::<usize>()) as u64)
                .saturating_add(size_of::<PrunableTree<H>>() as u64)
                .saturating_add(tree_heap_bytes(left))
                .saturating_add(size_of::<PrunableTree<H>>() as u64)
                .saturating_add(tree_heap_bytes(right))
        }
        Node::Leaf { .. } | Node::Nil => 0,
    }
}

fn located_tree_bytes<H>(tree: &LocatedPrunableTree<H>) -> u64 {
    (size_of::<LocatedPrunableTree<H>>() as u64).saturating_add(tree_heap_bytes(tree.root()))
}

fn cap_tree_bytes<H>(tree: &PrunableTree<H>) -> u64 {
    (size_of::<PrunableTree<H>>() as u64).saturating_add(tree_heap_bytes(tree))
}

fn checkpoint_bytes(checkpoint: &ShardCheckpoint) -> u64 {
    (size_of::<ShardCheckpoint>() as u64)
        .saturating_add(
            (checkpoint.marks_removed().len() * (size_of::<Position>() + 4 * size_of::<usize>()))
                as u64,
        )
        .saturating_add((4 * size_of::<usize>()) as u64)
}

#[derive(Debug, Default, Clone, Copy)]
struct CacheCounterSnapshot {
    hits: u64,
    misses: u64,
    evictions: u64,
}

struct InstrumentedSparseStore<S>
where
    S: ShardStore,
    S::H: Clone,
    S::CheckpointId: Clone + Ord,
{
    inner: SparseCachingShardStore<S>,
    hits: Cell<u64>,
    misses: Cell<u64>,
    evictions: Cell<u64>,
    dirty_shards: RefCell<BTreeSet<u64>>,
    shard_bytes: RefCell<BTreeMap<u64, u64>>,
    backend_root_indices: RefCell<BTreeSet<u64>>,
    checkpoint_bytes: RefCell<BTreeMap<S::CheckpointId, u64>>,
    retained_checkpoints: RefCell<BTreeSet<S::CheckpointId>>,
    cap_bytes: Cell<u64>,
    current_bytes: Cell<u64>,
    peak_bytes: Cell<u64>,
}

impl<S> InstrumentedSparseStore<S>
where
    S: ShardStore,
    S::H: Clone,
    S::CheckpointId: Clone + Ord,
{
    fn new(
        inner: SparseCachingShardStore<S>,
        preloaded: &[Address],
        backend_roots: &[Address],
    ) -> std::result::Result<Self, shardtree::store::caching::SparseStoreError> {
        let mut shard_bytes = BTreeMap::new();
        for address in preloaded {
            if let Some(shard) = inner.get_shard(*address)? {
                shard_bytes.insert(
                    address.index(),
                    located_tree_bytes(&shard).saturating_add((4 * size_of::<usize>()) as u64),
                );
            }
        }
        let cap_bytes = cap_tree_bytes(&inner.get_cap()?);
        let checkpoint_count = inner.checkpoint_count()?;
        let mut checkpoints = Vec::with_capacity(checkpoint_count);
        inner.for_each_checkpoint(checkpoint_count, |id, checkpoint| {
            checkpoints.push((id.clone(), checkpoint_bytes(checkpoint)));
            Ok(())
        })?;
        let checkpoint_bytes = checkpoints.into_iter().collect::<BTreeMap<_, _>>();
        let retained_checkpoints = inner.retained_checkpoints()?;
        let backend_root_indices = backend_roots
            .iter()
            .map(|address| address.index())
            .collect::<BTreeSet<_>>();
        let current_bytes = shard_bytes
            .values()
            .chain(checkpoint_bytes.values())
            .copied()
            .sum::<u64>()
            .saturating_add(cap_bytes)
            .saturating_add(
                (retained_checkpoints.len()
                    * (size_of::<S::CheckpointId>() + 4 * size_of::<usize>()))
                    as u64,
            )
            .saturating_add(
                (backend_root_indices.len()
                    * (size_of::<u64>() + size_of::<Address>() + 4 * size_of::<usize>()))
                    as u64,
            );
        Ok(Self {
            inner,
            hits: Cell::new(0),
            misses: Cell::new(0),
            evictions: Cell::new(0),
            dirty_shards: RefCell::new(BTreeSet::new()),
            shard_bytes: RefCell::new(shard_bytes),
            backend_root_indices: RefCell::new(backend_root_indices),
            checkpoint_bytes: RefCell::new(checkpoint_bytes),
            retained_checkpoints: RefCell::new(retained_checkpoints),
            cap_bytes: Cell::new(cap_bytes),
            current_bytes: Cell::new(current_bytes),
            peak_bytes: Cell::new(current_bytes),
        })
    }

    fn adjust_bytes(&self, previous: u64, next: u64) {
        let current = self
            .current_bytes
            .get()
            .saturating_sub(previous)
            .saturating_add(next);
        self.current_bytes.set(current);
        self.peak_bytes.set(self.peak_bytes.get().max(current));
    }

    fn snapshot(&self) -> CacheCounterSnapshot {
        CacheCounterSnapshot {
            hits: self.hits.get(),
            misses: self.misses.get(),
            evictions: self.evictions.get(),
        }
    }

    fn take_dirty_shards(&self) -> BTreeSet<u64> {
        std::mem::take(&mut *self.dirty_shards.borrow_mut())
    }

    fn flush_delta(&mut self) -> std::result::Result<(), S::Error> {
        self.inner.flush_delta()
    }

    fn current_bytes(&self) -> u64 {
        self.current_bytes.get()
    }

    fn peak_bytes(&self) -> u64 {
        self.peak_bytes.get()
    }

    fn cached_shard_count(&self) -> u64 {
        self.shard_bytes.borrow().len() as u64
    }
}

impl<S> ShardStore for InstrumentedSparseStore<S>
where
    S: ShardStore,
    S::H: Clone,
    S::CheckpointId: Clone + Ord,
{
    type H = S::H;
    type CheckpointId = S::CheckpointId;
    type Error = shardtree::store::caching::SparseStoreError;

    fn get_shard(
        &self,
        shard_root: Address,
    ) -> std::result::Result<Option<LocatedPrunableTree<Self::H>>, Self::Error> {
        match self.inner.get_shard(shard_root) {
            Ok(Some(shard)) => {
                self.hits.set(self.hits.get().saturating_add(1));
                Ok(Some(shard))
            }
            Ok(None) => {
                self.misses.set(self.misses.get().saturating_add(1));
                Ok(None)
            }
            Err(error) => {
                self.misses.set(self.misses.get().saturating_add(1));
                Err(error)
            }
        }
    }

    fn last_shard(&self) -> std::result::Result<Option<LocatedPrunableTree<Self::H>>, Self::Error> {
        match self.inner.last_shard() {
            Ok(Some(shard)) => {
                self.hits.set(self.hits.get().saturating_add(1));
                Ok(Some(shard))
            }
            Ok(None) => {
                self.misses.set(self.misses.get().saturating_add(1));
                Ok(None)
            }
            Err(error) => {
                self.misses.set(self.misses.get().saturating_add(1));
                Err(error)
            }
        }
    }

    fn put_shard(
        &mut self,
        subtree: LocatedPrunableTree<Self::H>,
    ) -> std::result::Result<(), Self::Error> {
        let index = subtree.root_addr().index();
        let bytes = located_tree_bytes(&subtree).saturating_add((4 * size_of::<usize>()) as u64);
        self.inner.put_shard(subtree)?;
        self.dirty_shards.borrow_mut().insert(index);
        let previous = self
            .shard_bytes
            .borrow_mut()
            .insert(index, bytes)
            .unwrap_or(0);
        self.adjust_bytes(previous, bytes);
        Ok(())
    }

    fn get_shard_roots(&self) -> std::result::Result<Vec<Address>, Self::Error> {
        self.inner.get_shard_roots()
    }

    fn truncate_shards(&mut self, shard_index: u64) -> std::result::Result<(), Self::Error> {
        self.inner.truncate_shards(shard_index)?;
        let removed = self.shard_bytes.borrow_mut().split_off(&shard_index);
        let removed_bytes = removed.values().copied().sum::<u64>();
        let removed_backend_roots = self
            .backend_root_indices
            .borrow_mut()
            .split_off(&shard_index);
        let removed_backend_root_bytes = (removed_backend_roots.len()
            * (size_of::<u64>() + size_of::<Address>() + 4 * size_of::<usize>()))
            as u64;
        self.current_bytes.set(
            self.current_bytes
                .get()
                .saturating_sub(removed_bytes)
                .saturating_sub(removed_backend_root_bytes),
        );
        self.evictions
            .set(self.evictions.get().saturating_add(removed.len() as u64));
        self.dirty_shards
            .borrow_mut()
            .retain(|index| *index < shard_index);
        Ok(())
    }

    fn get_cap(&self) -> std::result::Result<PrunableTree<Self::H>, Self::Error> {
        self.inner.get_cap()
    }

    fn put_cap(&mut self, cap: PrunableTree<Self::H>) -> std::result::Result<(), Self::Error> {
        let bytes = cap_tree_bytes(&cap);
        self.inner.put_cap(cap)?;
        let previous = self.cap_bytes.replace(bytes);
        self.adjust_bytes(previous, bytes);
        Ok(())
    }

    fn min_checkpoint_id(&self) -> std::result::Result<Option<Self::CheckpointId>, Self::Error> {
        self.inner.min_checkpoint_id()
    }

    fn max_checkpoint_id(&self) -> std::result::Result<Option<Self::CheckpointId>, Self::Error> {
        self.inner.max_checkpoint_id()
    }

    fn add_checkpoint(
        &mut self,
        checkpoint_id: Self::CheckpointId,
        checkpoint: ShardCheckpoint,
    ) -> std::result::Result<(), Self::Error> {
        let bytes = checkpoint_bytes(&checkpoint);
        self.inner
            .add_checkpoint(checkpoint_id.clone(), checkpoint)?;
        let previous = self
            .checkpoint_bytes
            .borrow_mut()
            .insert(checkpoint_id, bytes)
            .unwrap_or(0);
        self.adjust_bytes(previous, bytes);
        Ok(())
    }

    fn checkpoint_count(&self) -> std::result::Result<usize, Self::Error> {
        self.inner.checkpoint_count()
    }

    fn get_checkpoint_at_depth(
        &self,
        checkpoint_depth: usize,
    ) -> std::result::Result<Option<(Self::CheckpointId, ShardCheckpoint)>, Self::Error> {
        self.inner.get_checkpoint_at_depth(checkpoint_depth)
    }

    fn get_checkpoint(
        &self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<Option<ShardCheckpoint>, Self::Error> {
        self.inner.get_checkpoint(checkpoint_id)
    }

    fn with_checkpoints<F>(
        &mut self,
        limit: usize,
        callback: F,
    ) -> std::result::Result<(), Self::Error>
    where
        F: FnMut(&Self::CheckpointId, &ShardCheckpoint) -> std::result::Result<(), Self::Error>,
    {
        self.inner.with_checkpoints(limit, callback)
    }

    fn for_each_checkpoint<F>(
        &self,
        limit: usize,
        callback: F,
    ) -> std::result::Result<(), Self::Error>
    where
        F: FnMut(&Self::CheckpointId, &ShardCheckpoint) -> std::result::Result<(), Self::Error>,
    {
        self.inner.for_each_checkpoint(limit, callback)
    }

    fn update_checkpoint_with<F>(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
        update: F,
    ) -> std::result::Result<bool, Self::Error>
    where
        F: Fn(&mut ShardCheckpoint) -> std::result::Result<(), Self::Error>,
    {
        let updated = self.inner.update_checkpoint_with(checkpoint_id, update)?;
        if updated {
            if let Some(checkpoint) = self.inner.get_checkpoint(checkpoint_id)? {
                let bytes = checkpoint_bytes(&checkpoint);
                let previous = self
                    .checkpoint_bytes
                    .borrow_mut()
                    .insert(checkpoint_id.clone(), bytes)
                    .unwrap_or(0);
                self.adjust_bytes(previous, bytes);
            }
        }
        Ok(updated)
    }

    fn remove_checkpoint(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.remove_checkpoint(checkpoint_id)?;
        let removed = self
            .checkpoint_bytes
            .borrow_mut()
            .remove(checkpoint_id)
            .unwrap_or(0);
        self.current_bytes
            .set(self.current_bytes.get().saturating_sub(removed));
        Ok(())
    }

    fn add_retained_checkpoint(
        &mut self,
        checkpoint_id: Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.add_retained_checkpoint(checkpoint_id.clone())?;
        if self.retained_checkpoints.borrow_mut().insert(checkpoint_id) {
            self.adjust_bytes(
                0,
                (size_of::<Self::CheckpointId>() + 4 * size_of::<usize>()) as u64,
            );
        }
        Ok(())
    }

    fn remove_retained_checkpoint(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.remove_retained_checkpoint(checkpoint_id)?;
        if self.retained_checkpoints.borrow_mut().remove(checkpoint_id) {
            self.current_bytes.set(
                self.current_bytes.get().saturating_sub(
                    (size_of::<Self::CheckpointId>() + 4 * size_of::<usize>()) as u64,
                ),
            );
        }
        Ok(())
    }

    fn retained_checkpoints(
        &self,
    ) -> std::result::Result<BTreeSet<Self::CheckpointId>, Self::Error> {
        self.inner.retained_checkpoints()
    }

    fn truncate_checkpoints_retaining(
        &mut self,
        checkpoint_id: &Self::CheckpointId,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.truncate_checkpoints_retaining(checkpoint_id)?;
        let removed = self.checkpoint_bytes.borrow_mut().split_off(checkpoint_id);
        let mut removed_bytes = removed.values().copied().sum::<u64>();
        if let Some(checkpoint) = self.inner.get_checkpoint(checkpoint_id)? {
            let bytes = checkpoint_bytes(&checkpoint);
            self.checkpoint_bytes
                .borrow_mut()
                .insert(checkpoint_id.clone(), bytes);
            removed_bytes = removed_bytes.saturating_sub(bytes);
        }
        self.current_bytes
            .set(self.current_bytes.get().saturating_sub(removed_bytes));
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ShardtreePoolPersistenceTelemetry {
    pub(super) preload_discovery: Duration,
    pub(super) shard_loading: Duration,
    pub(super) cache_hits: u64,
    pub(super) cache_misses: u64,
    pub(super) cache_evictions: u64,
    pub(super) commitment_count: u64,
    pub(super) commitment_insert: Duration,
    pub(super) parallel_construction: Duration,
    pub(super) parallel_worker_active: Duration,
    pub(super) prepared_tree_insert: Duration,
    pub(super) prepared_tree_count: u64,
    pub(super) checkpoint_count: u64,
    pub(super) checkpoint_processing: Duration,
    pub(super) dirty_shards: u64,
    pub(super) dirty_encoded_bytes: u64,
    pub(super) flush: Duration,
    pub(super) cache_bytes: u64,
    pub(super) peak_cache_bytes: u64,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ShardtreePersistenceTelemetry {
    pub(super) sapling: ShardtreePoolPersistenceTelemetry,
    pub(super) ironwood: ShardtreePoolPersistenceTelemetry,
    pub(super) transaction_lock_wait: Duration,
    pub(super) sqlite_commit: Duration,
    pub(super) cache_reused: bool,
    pub(super) cache_evicted_after_commit: bool,
}

#[derive(Debug, Default)]
struct DeltaWriteStats {
    shard_indices: BTreeSet<u64>,
}

fn take_buffered_actions<H, C>(pending: &BufferedActions<H, C>) -> Vec<BufferedShardAction<H, C>> {
    std::mem::take(&mut *pending.borrow_mut())
}

fn replay_buffered_actions<S>(
    store: &mut S,
    actions: Vec<BufferedShardAction<S::H, S::CheckpointId>>,
) -> std::result::Result<DeltaWriteStats, S::Error>
where
    S: ShardStore,
    S::H: Clone,
    S::CheckpointId: Clone + Ord,
{
    let mut stats = DeltaWriteStats::default();
    for action in actions {
        match action {
            BufferedShardAction::PutShard(shard) => {
                stats.shard_indices.insert(shard.root_addr().index());
                store.put_shard(shard)?;
            }
            BufferedShardAction::TruncateShards(index) => store.truncate_shards(index)?,
            BufferedShardAction::PutCap(cap) => store.put_cap(cap)?,
            BufferedShardAction::AddCheckpoint(id, checkpoint) => {
                store.add_checkpoint(id, checkpoint)?;
            }
            BufferedShardAction::UpdateCheckpoint(id, checkpoint) => {
                let replacement = checkpoint.clone();
                let updated = store.update_checkpoint_with(&id, move |existing| {
                    *existing = replacement.clone();
                    Ok(())
                })?;
                if !updated {
                    store.add_checkpoint(id, checkpoint)?;
                }
            }
            BufferedShardAction::RemoveCheckpoint(id) => store.remove_checkpoint(&id)?,
            BufferedShardAction::AddRetainedCheckpoint(id) => {
                store.add_retained_checkpoint(id)?;
            }
            BufferedShardAction::RemoveRetainedCheckpoint(id) => {
                store.remove_retained_checkpoint(&id)?;
            }
            BufferedShardAction::TruncateCheckpointsRetaining(id) => {
                store.truncate_checkpoints_retaining(&id)?;
            }
        }
    }
    Ok(stats)
}

fn dirty_encoded_bytes(
    tx: &rusqlite::Transaction<'_>,
    table_prefix: &'static str,
    shard_indices: &BTreeSet<u64>,
) -> Result<u64> {
    if shard_indices.is_empty() {
        return Ok(0);
    }
    let sql = format!(
        "SELECT COALESCE((SELECT length(shard_data) FROM {}_tree_shards WHERE shard_index = ?1), 0)",
        table_prefix
    );
    let mut statement = tx.prepare_cached(&sql).map_err(|error| {
        Error::Sync(format!(
            "Failed to prepare {} dirty-shard telemetry query: {}",
            table_prefix, error
        ))
    })?;
    let mut total = 0u64;
    for index in shard_indices {
        let bytes: i64 = statement
            .query_row([*index as i64], |row| row.get(0))
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to measure {} dirty shard {}: {}",
                    table_prefix, index, error
                ))
            })?;
        total = total.saturating_add(bytes.max(0) as u64);
    }
    Ok(total)
}

pub(super) fn persist_verified_pool_roots<H, const SHARD_HEIGHT: u8>(
    tx: &rusqlite::Transaction<'_>,
    table_prefix: &'static str,
    roots: &[VerifiedSubtreeRoot<H>],
) -> Result<()>
where
    H: HashSer + Hashable + Clone + Eq,
{
    for root in roots {
        let end_height = u32::try_from(root.end_height).map_err(|_| {
            Error::Sync(format!(
                "Subtree completing height {} exceeds u32",
                root.end_height
            ))
        })?;
        let persisted = PersistedSubtreeRoot::new(BlockHeight::from(end_height), root.root.clone());
        put_shard_roots::<H, { NOTE_COMMITMENT_TREE_DEPTH }, SHARD_HEIGHT>(
            tx,
            table_prefix,
            root.index,
            std::slice::from_ref(&persisted),
        )
        .map_err(|error| {
            Error::Sync(format!(
                "Failed to finalize verified {} subtree root {}: {}",
                table_prefix, root.index, error
            ))
        })?;
    }
    Ok(())
}

type WorkerSaplingBackend<'a> = BufferedShardStore<
    SqliteShardStore<&'a rusqlite::Connection, SaplingNode, SAPLING_SHARD_HEIGHT>,
>;
type WorkerIronwoodBackend<'a> = BufferedShardStore<
    SqliteShardStore<&'a rusqlite::Connection, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>,
>;
type WorkerSaplingStore<'a> = InstrumentedSparseStore<WorkerSaplingBackend<'a>>;
type WorkerIronwoodStore<'a> = InstrumentedSparseStore<WorkerIronwoodBackend<'a>>;

fn committed_checkpoint_heights(conn: &rusqlite::Connection) -> Result<CommittedCheckpointHeights> {
    let read_height = |table: &'static str| -> Result<Option<u32>> {
        conn.query_row(
            &format!("SELECT MAX(checkpoint_id) FROM {}", table),
            [],
            |row| row.get(0),
        )
        .map_err(|error| {
            Error::Sync(format!(
                "Failed to read latest {} checkpoint: {}",
                table, error
            ))
        })
    };
    Ok(CommittedCheckpointHeights {
        sapling: read_height("sapling_tree_checkpoints")?,
        ironwood: read_height("orchard_tree_checkpoints")?,
    })
}

pub(super) struct PersistenceShardTrees<'a> {
    sapling_tree:
        ShardTree<WorkerSaplingStore<'a>, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT>,
    ironwood_tree:
        ShardTree<WorkerIronwoodStore<'a>, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT>,
    sapling_pending: BufferedActions<SaplingNode, BlockHeight>,
    ironwood_pending: BufferedActions<MerkleHashOrchard, BlockHeight>,
    sapling_preload_discovery: Duration,
    ironwood_preload_discovery: Duration,
    sapling_shard_loading: Duration,
    ironwood_shard_loading: Duration,
    memory_limit_bytes: u64,
    has_committed: bool,
}

impl<'a> PersistenceShardTrees<'a> {
    pub(super) fn load(conn: &'a rusqlite::Connection, memory_limit_bytes: u64) -> Result<Self> {
        let sapling_backend =
            SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                conn,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|error| {
                Error::Sync(format!("Failed to open Sapling shard store: {}", error))
            })?;
        let sapling_discovery_start = Instant::now();
        let sapling_preloads =
            sparse_preload_addresses::<_, SAPLING_SHARD_HEIGHT>(&sapling_backend, "Sapling")?;
        let sapling_backend_roots = sapling_backend.get_shard_roots().map_err(|error| {
            Error::Sync(format!(
                "Failed to inventory Sapling shard roots for cache telemetry: {}",
                error
            ))
        })?;
        let sapling_preload_discovery = sapling_discovery_start.elapsed();
        let sapling_pending = Rc::new(RefCell::new(Vec::new()));
        let sapling_buffered =
            BufferedShardStore::new(sapling_backend, Rc::clone(&sapling_pending));
        let sapling_load_start = Instant::now();
        let sapling_sparse =
            SparseCachingShardStore::with_preloaded(sapling_buffered, sapling_preloads.clone())
                .map_err(|error| {
                    Error::Sync(format!("Failed to preload Sapling shard store: {}", error))
                })?;
        let sapling_store =
            InstrumentedSparseStore::new(sapling_sparse, &sapling_preloads, &sapling_backend_roots)
                .map_err(|error| {
                    Error::Sync(format!(
                        "Failed to initialize Sapling cache metrics: {}",
                        error
                    ))
                })?;
        let sapling_shard_loading = sapling_load_start.elapsed();

        let ironwood_backend =
            SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                conn,
                ORCHARD_TABLE_PREFIX,
            )
            .map_err(|error| {
                Error::Sync(format!("Failed to open Ironwood shard store: {}", error))
            })?;
        let ironwood_discovery_start = Instant::now();
        let ironwood_preloads =
            sparse_preload_addresses::<_, ORCHARD_SHARD_HEIGHT>(&ironwood_backend, "Ironwood")?;
        let ironwood_backend_roots = ironwood_backend.get_shard_roots().map_err(|error| {
            Error::Sync(format!(
                "Failed to inventory Ironwood shard roots for cache telemetry: {}",
                error
            ))
        })?;
        let ironwood_preload_discovery = ironwood_discovery_start.elapsed();
        let ironwood_pending = Rc::new(RefCell::new(Vec::new()));
        let ironwood_buffered =
            BufferedShardStore::new(ironwood_backend, Rc::clone(&ironwood_pending));
        let ironwood_load_start = Instant::now();
        let ironwood_sparse =
            SparseCachingShardStore::with_preloaded(ironwood_buffered, ironwood_preloads.clone())
                .map_err(|error| {
                Error::Sync(format!("Failed to preload Ironwood shard store: {}", error))
            })?;
        let ironwood_store = InstrumentedSparseStore::new(
            ironwood_sparse,
            &ironwood_preloads,
            &ironwood_backend_roots,
        )
        .map_err(|error| {
            Error::Sync(format!(
                "Failed to initialize Ironwood cache metrics: {}",
                error
            ))
        })?;
        let ironwood_shard_loading = ironwood_load_start.elapsed();

        Ok(Self {
            sapling_tree: ShardTree::new(sapling_store, SHARDTREE_PRUNING_DEPTH),
            ironwood_tree: ShardTree::new(ironwood_store, SHARDTREE_PRUNING_DEPTH),
            sapling_pending,
            ironwood_pending,
            sapling_preload_discovery,
            ironwood_preload_discovery,
            sapling_shard_loading,
            ironwood_shard_loading,
            memory_limit_bytes,
            has_committed: false,
        })
    }

    fn begin_telemetry(
        &mut self,
    ) -> (
        ShardtreePersistenceTelemetry,
        CacheCounterSnapshot,
        CacheCounterSnapshot,
    ) {
        let sapling_before = self.sapling_tree.store().snapshot();
        let ironwood_before = self.ironwood_tree.store().snapshot();
        let telemetry = ShardtreePersistenceTelemetry {
            sapling: ShardtreePoolPersistenceTelemetry {
                preload_discovery: std::mem::take(&mut self.sapling_preload_discovery),
                shard_loading: std::mem::take(&mut self.sapling_shard_loading),
                ..ShardtreePoolPersistenceTelemetry::default()
            },
            ironwood: ShardtreePoolPersistenceTelemetry {
                preload_discovery: std::mem::take(&mut self.ironwood_preload_discovery),
                shard_loading: std::mem::take(&mut self.ironwood_shard_loading),
                ..ShardtreePoolPersistenceTelemetry::default()
            },
            cache_reused: self.has_committed,
            ..ShardtreePersistenceTelemetry::default()
        };
        (telemetry, sapling_before, ironwood_before)
    }

    fn flush_to_transaction(
        &mut self,
        tx: &rusqlite::Transaction<'_>,
        telemetry: &mut ShardtreePersistenceTelemetry,
        verified_roots: &VerifiedSubtreeRoots,
    ) -> Result<()> {
        let sapling_flush_start = Instant::now();
        self.sapling_tree
            .store_mut()
            .flush_delta()
            .map_err(|error| Error::Sync(format!("Failed to flush Sapling cache: {}", error)))?;
        let sapling_actions = take_buffered_actions(&self.sapling_pending);
        let mut sapling_store =
            SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                tx,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to open transactional Sapling store: {}",
                    error
                ))
            })?;
        let sapling_delta =
            replay_buffered_actions(&mut sapling_store, sapling_actions).map_err(|error| {
                Error::Sync(format!("Failed to write Sapling cache delta: {}", error))
            })?;
        persist_verified_pool_roots::<SaplingNode, SAPLING_SHARD_HEIGHT>(
            tx,
            SAPLING_TABLE_PREFIX,
            &verified_roots.sapling,
        )?;
        telemetry.sapling.dirty_shards = sapling_delta.shard_indices.len() as u64;
        telemetry.sapling.dirty_encoded_bytes =
            dirty_encoded_bytes(tx, SAPLING_TABLE_PREFIX, &sapling_delta.shard_indices)?;
        telemetry.sapling.flush = sapling_flush_start.elapsed();
        let _ = self.sapling_tree.store().take_dirty_shards();

        let ironwood_flush_start = Instant::now();
        self.ironwood_tree
            .store_mut()
            .flush_delta()
            .map_err(|error| Error::Sync(format!("Failed to flush Ironwood cache: {}", error)))?;
        let ironwood_actions = take_buffered_actions(&self.ironwood_pending);
        let mut ironwood_store =
            SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                tx,
                ORCHARD_TABLE_PREFIX,
            )
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to open transactional Ironwood store: {}",
                    error
                ))
            })?;
        let ironwood_delta = replay_buffered_actions(&mut ironwood_store, ironwood_actions)
            .map_err(|error| {
                Error::Sync(format!("Failed to write Ironwood cache delta: {}", error))
            })?;
        persist_verified_pool_roots::<MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>(
            tx,
            ORCHARD_TABLE_PREFIX,
            &verified_roots.ironwood,
        )?;
        telemetry.ironwood.dirty_shards = ironwood_delta.shard_indices.len() as u64;
        telemetry.ironwood.dirty_encoded_bytes =
            dirty_encoded_bytes(tx, ORCHARD_TABLE_PREFIX, &ironwood_delta.shard_indices)?;
        telemetry.ironwood.flush = ironwood_flush_start.elapsed();
        let _ = self.ironwood_tree.store().take_dirty_shards();
        Ok(())
    }

    fn finish_telemetry(
        &self,
        telemetry: &mut ShardtreePersistenceTelemetry,
        sapling_before: CacheCounterSnapshot,
        ironwood_before: CacheCounterSnapshot,
    ) {
        let sapling_after = self.sapling_tree.store().snapshot();
        let ironwood_after = self.ironwood_tree.store().snapshot();
        telemetry.sapling.cache_hits = sapling_after.hits.saturating_sub(sapling_before.hits);
        telemetry.sapling.cache_misses = sapling_after.misses.saturating_sub(sapling_before.misses);
        telemetry.sapling.cache_evictions = sapling_after
            .evictions
            .saturating_sub(sapling_before.evictions);
        telemetry.sapling.cache_bytes = self.sapling_tree.store().current_bytes();
        telemetry.sapling.peak_cache_bytes = self.sapling_tree.store().peak_bytes();
        telemetry.ironwood.cache_hits = ironwood_after.hits.saturating_sub(ironwood_before.hits);
        telemetry.ironwood.cache_misses =
            ironwood_after.misses.saturating_sub(ironwood_before.misses);
        telemetry.ironwood.cache_evictions = ironwood_after
            .evictions
            .saturating_sub(ironwood_before.evictions);
        telemetry.ironwood.cache_bytes = self.ironwood_tree.store().current_bytes();
        telemetry.ironwood.peak_cache_bytes = self.ironwood_tree.store().peak_bytes();
    }

    fn commit_transaction(
        &mut self,
        tx: rusqlite::Transaction<'_>,
        telemetry: &mut ShardtreePersistenceTelemetry,
        sapling_before: CacheCounterSnapshot,
        ironwood_before: CacheCounterSnapshot,
        verified_roots: &VerifiedSubtreeRoots,
    ) -> Result<bool> {
        self.flush_to_transaction(&tx, telemetry, verified_roots)?;
        let commit_start = Instant::now();
        tx.commit().map_err(|error| {
            Error::Sync(format!(
                "Failed to commit cached shardtree transaction: {}",
                error
            ))
        })?;
        telemetry.sqlite_commit = commit_start.elapsed();
        self.has_committed = true;
        self.finish_telemetry(telemetry, sapling_before, ironwood_before);
        let cache_bytes = telemetry
            .sapling
            .cache_bytes
            .saturating_add(telemetry.ironwood.cache_bytes);
        let evict = cache_bytes > self.memory_limit_bytes;
        if evict {
            telemetry.cache_evicted_after_commit = true;
            telemetry.sapling.cache_evictions = telemetry
                .sapling
                .cache_evictions
                .saturating_add(self.sapling_tree.store().cached_shard_count());
            telemetry.ironwood.cache_evictions = telemetry
                .ironwood
                .cache_evictions
                .saturating_add(self.ironwood_tree.store().cached_shard_count());
        }
        Ok(evict)
    }

    #[cfg(test)]
    pub(super) fn persist_batches(
        &mut self,
        db: &Database,
        batches: &[ShardtreeBatch],
        batch_end_height: Option<u64>,
    ) -> Result<(ShardtreePersistResult, ShardtreePersistenceTelemetry, bool)> {
        self.persist_batches_with_roots(
            db,
            batches,
            batch_end_height,
            &VerifiedSubtreeRoots::default(),
        )
    }

    pub(super) fn persist_owned_batches_with_roots(
        &mut self,
        db: &Database,
        batches: Vec<ShardtreeBatch>,
        batch_end_height: Option<u64>,
        verified_roots: &VerifiedSubtreeRoots,
        construction_pool: &rayon::ThreadPool,
    ) -> Result<(ShardtreePersistResult, ShardtreePersistenceTelemetry, bool)> {
        let prepared_against = committed_checkpoint_heights(db.conn())?;
        let prepared = prepare_parallel_shardtree_insertions(
            construction_pool,
            batches,
            batch_end_height,
            prepared_against,
            verified_roots,
        )?;
        let (mut telemetry, sapling_before, ironwood_before) = self.begin_telemetry();
        let lock_start = Instant::now();
        let tx = db.unchecked_immediate_transaction().map_err(|error| {
            Error::Sync(format!(
                "Failed to start cached shardtree transaction: {}",
                error
            ))
        })?;
        telemetry.transaction_lock_wait = lock_start.elapsed();
        let committed_in_transaction = committed_checkpoint_heights(&tx)?;
        if committed_in_transaction != prepared_against {
            return Err(Error::Sync(
                "ShardTree checkpoints changed during parallel construction; retrying is required"
                    .to_string(),
            ));
        }
        let result = apply_prepared_shardtree_insertions_to_trees(
            &mut self.sapling_tree,
            &mut self.ironwood_tree,
            prepared,
        )?;
        telemetry.sapling.commitment_count = result.sapling_work.commitment_count;
        telemetry.sapling.commitment_insert = result.sapling_work.commitment_insert;
        telemetry.sapling.parallel_construction = result.sapling_work.parallel_construction;
        telemetry.sapling.parallel_worker_active = result.sapling_work.parallel_worker_active;
        telemetry.sapling.prepared_tree_insert = result.sapling_work.prepared_tree_insert;
        telemetry.sapling.prepared_tree_count = result.sapling_work.prepared_tree_count;
        telemetry.sapling.checkpoint_count = result.sapling_work.checkpoint_count;
        telemetry.sapling.checkpoint_processing = result.sapling_work.checkpoint_processing;
        telemetry.ironwood.commitment_count = result.ironwood_work.commitment_count;
        telemetry.ironwood.commitment_insert = result.ironwood_work.commitment_insert;
        telemetry.ironwood.parallel_construction = result.ironwood_work.parallel_construction;
        telemetry.ironwood.parallel_worker_active = result.ironwood_work.parallel_worker_active;
        telemetry.ironwood.prepared_tree_insert = result.ironwood_work.prepared_tree_insert;
        telemetry.ironwood.prepared_tree_count = result.ironwood_work.prepared_tree_count;
        telemetry.ironwood.checkpoint_count = result.ironwood_work.checkpoint_count;
        telemetry.ironwood.checkpoint_processing = result.ironwood_work.checkpoint_processing;
        let evict = self.commit_transaction(
            tx,
            &mut telemetry,
            sapling_before,
            ironwood_before,
            verified_roots,
        )?;
        Ok((result, telemetry, evict))
    }

    #[cfg(test)]
    pub(super) fn persist_batches_with_roots(
        &mut self,
        db: &Database,
        batches: &[ShardtreeBatch],
        batch_end_height: Option<u64>,
        verified_roots: &VerifiedSubtreeRoots,
    ) -> Result<(ShardtreePersistResult, ShardtreePersistenceTelemetry, bool)> {
        let (mut telemetry, sapling_before, ironwood_before) = self.begin_telemetry();
        let lock_start = Instant::now();
        let tx = db.unchecked_immediate_transaction().map_err(|error| {
            Error::Sync(format!(
                "Failed to start cached shardtree transaction: {}",
                error
            ))
        })?;
        telemetry.transaction_lock_wait = lock_start.elapsed();
        let max_committed_heights = committed_checkpoint_heights(&tx)?;
        let result = apply_shardtree_batches_to_trees(
            &mut self.sapling_tree,
            &mut self.ironwood_tree,
            batches,
            batch_end_height,
            max_committed_heights,
            verified_roots,
        )?;
        telemetry.sapling.commitment_count = result.sapling_work.commitment_count;
        telemetry.sapling.commitment_insert = result.sapling_work.commitment_insert;
        telemetry.sapling.checkpoint_count = result.sapling_work.checkpoint_count;
        telemetry.sapling.checkpoint_processing = result.sapling_work.checkpoint_processing;
        telemetry.ironwood.commitment_count = result.ironwood_work.commitment_count;
        telemetry.ironwood.commitment_insert = result.ironwood_work.commitment_insert;
        telemetry.ironwood.checkpoint_count = result.ironwood_work.checkpoint_count;
        telemetry.ironwood.checkpoint_processing = result.ironwood_work.checkpoint_processing;
        let evict = self.commit_transaction(
            tx,
            &mut telemetry,
            sapling_before,
            ironwood_before,
            verified_roots,
        )?;
        Ok((result, telemetry, evict))
    }

    pub(super) fn checkpoint_tip(
        &mut self,
        db: &Database,
        checkpoint_id: BlockHeight,
    ) -> Result<(ShardtreePersistenceTelemetry, bool)> {
        let (mut telemetry, sapling_before, ironwood_before) = self.begin_telemetry();
        let lock_start = Instant::now();
        let tx = db.unchecked_immediate_transaction().map_err(|error| {
            Error::Sync(format!(
                "Failed to start cached checkpoint transaction: {}",
                error
            ))
        })?;
        telemetry.transaction_lock_wait = lock_start.elapsed();
        let sapling_start = Instant::now();
        let _ = self
            .sapling_tree
            .checkpoint(checkpoint_id)
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to checkpoint cached Sapling tree: {}",
                    error
                ))
            })?;
        telemetry.sapling.checkpoint_processing = sapling_start.elapsed();
        telemetry.sapling.checkpoint_count = 1;
        let ironwood_start = Instant::now();
        let _ = self
            .ironwood_tree
            .checkpoint(checkpoint_id)
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to checkpoint cached Ironwood tree: {}",
                    error
                ))
            })?;
        telemetry.ironwood.checkpoint_processing = ironwood_start.elapsed();
        telemetry.ironwood.checkpoint_count = 1;
        let evict = self.commit_transaction(
            tx,
            &mut telemetry,
            sapling_before,
            ironwood_before,
            &VerifiedSubtreeRoots::default(),
        )?;
        Ok((telemetry, evict))
    }

    pub(super) fn retain_checkpoint(
        &mut self,
        db: &Database,
        checkpoint_id: BlockHeight,
    ) -> Result<(ShardtreePersistenceTelemetry, bool)> {
        let (mut telemetry, sapling_before, ironwood_before) = self.begin_telemetry();
        let lock_start = Instant::now();
        let tx = db.unchecked_immediate_transaction().map_err(|error| {
            Error::Sync(format!(
                "Failed to start cached retained-checkpoint transaction: {}",
                error
            ))
        })?;
        telemetry.transaction_lock_wait = lock_start.elapsed();
        let sapling_start = Instant::now();
        self.sapling_tree
            .ensure_retained(checkpoint_id)
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to retain cached Sapling checkpoint: {}",
                    error
                ))
            })?;
        telemetry.sapling.checkpoint_processing = sapling_start.elapsed();
        telemetry.sapling.checkpoint_count = 1;
        let ironwood_start = Instant::now();
        self.ironwood_tree
            .ensure_retained(checkpoint_id)
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to retain cached Ironwood checkpoint: {}",
                    error
                ))
            })?;
        telemetry.ironwood.checkpoint_processing = ironwood_start.elapsed();
        telemetry.ironwood.checkpoint_count = 1;
        let evict = self.commit_transaction(
            tx,
            &mut telemetry,
            sapling_before,
            ironwood_before,
            &VerifiedSubtreeRoots::default(),
        )?;
        Ok((telemetry, evict))
    }

    pub(super) fn cached_shard_counts(&self) -> (u64, u64) {
        (
            self.sapling_tree.store().cached_shard_count(),
            self.ironwood_tree.store().cached_shard_count(),
        )
    }
}

pub(super) fn log_shardtree_persistence_telemetry(
    operation: &'static str,
    telemetry: &ShardtreePersistenceTelemetry,
) {
    tracing::debug!(
        operation,
        sapling_preload_discovery_us = telemetry.sapling.preload_discovery.as_micros(),
        sapling_shard_loading_us = telemetry.sapling.shard_loading.as_micros(),
        sapling_cache_hits = telemetry.sapling.cache_hits,
        sapling_cache_misses = telemetry.sapling.cache_misses,
        sapling_cache_evictions = telemetry.sapling.cache_evictions,
        sapling_commitments = telemetry.sapling.commitment_count,
        sapling_hash_insert_us = telemetry.sapling.commitment_insert.as_micros(),
        sapling_parallel_build_us = telemetry.sapling.parallel_construction.as_micros(),
        sapling_parallel_worker_active_us = telemetry.sapling.parallel_worker_active.as_micros(),
        sapling_prepared_insert_us = telemetry.sapling.prepared_tree_insert.as_micros(),
        sapling_prepared_trees = telemetry.sapling.prepared_tree_count,
        sapling_checkpoints = telemetry.sapling.checkpoint_count,
        sapling_checkpoint_us = telemetry.sapling.checkpoint_processing.as_micros(),
        sapling_dirty_shards = telemetry.sapling.dirty_shards,
        sapling_dirty_bytes = telemetry.sapling.dirty_encoded_bytes,
        sapling_flush_us = telemetry.sapling.flush.as_micros(),
        sapling_cache_estimated_bytes = telemetry.sapling.cache_bytes,
        sapling_peak_cache_estimated_bytes = telemetry.sapling.peak_cache_bytes,
        ironwood_preload_discovery_us = telemetry.ironwood.preload_discovery.as_micros(),
        ironwood_shard_loading_us = telemetry.ironwood.shard_loading.as_micros(),
        ironwood_cache_hits = telemetry.ironwood.cache_hits,
        ironwood_cache_misses = telemetry.ironwood.cache_misses,
        ironwood_cache_evictions = telemetry.ironwood.cache_evictions,
        ironwood_commitments = telemetry.ironwood.commitment_count,
        ironwood_hash_insert_us = telemetry.ironwood.commitment_insert.as_micros(),
        ironwood_parallel_build_us = telemetry.ironwood.parallel_construction.as_micros(),
        ironwood_parallel_worker_active_us = telemetry.ironwood.parallel_worker_active.as_micros(),
        ironwood_prepared_insert_us = telemetry.ironwood.prepared_tree_insert.as_micros(),
        ironwood_prepared_trees = telemetry.ironwood.prepared_tree_count,
        ironwood_checkpoints = telemetry.ironwood.checkpoint_count,
        ironwood_checkpoint_us = telemetry.ironwood.checkpoint_processing.as_micros(),
        ironwood_dirty_shards = telemetry.ironwood.dirty_shards,
        ironwood_dirty_bytes = telemetry.ironwood.dirty_encoded_bytes,
        ironwood_flush_us = telemetry.ironwood.flush.as_micros(),
        ironwood_cache_estimated_bytes = telemetry.ironwood.cache_bytes,
        ironwood_peak_cache_estimated_bytes = telemetry.ironwood.peak_cache_bytes,
        transaction_lock_wait_us = telemetry.transaction_lock_wait.as_micros(),
        sqlite_commit_us = telemetry.sqlite_commit.as_micros(),
        cache_reused = telemetry.cache_reused,
        cache_evicted_after_commit = telemetry.cache_evicted_after_commit,
        "shardtree persistence telemetry"
    );
    if sync_performance_logging_enabled() {
        append_sync_decision_log(
            "sync.rs:shardtree_persistence",
            "shardtree persistence telemetry",
            format!(
                "\"operation\":\"{}\",\"sapling_preload_discovery_us\":{},\"sapling_shard_loading_us\":{},\"sapling_cache_hits\":{},\"sapling_cache_misses\":{},\"sapling_cache_evictions\":{},\"sapling_commitments\":{},\"sapling_hash_insert_us\":{},\"sapling_parallel_build_us\":{},\"sapling_parallel_worker_active_us\":{},\"sapling_prepared_insert_us\":{},\"sapling_prepared_trees\":{},\"sapling_checkpoints\":{},\"sapling_checkpoint_us\":{},\"sapling_dirty_shards\":{},\"sapling_dirty_bytes\":{},\"sapling_flush_us\":{},\"sapling_cache_estimated_bytes\":{},\"sapling_peak_cache_estimated_bytes\":{},\"ironwood_preload_discovery_us\":{},\"ironwood_shard_loading_us\":{},\"ironwood_cache_hits\":{},\"ironwood_cache_misses\":{},\"ironwood_cache_evictions\":{},\"ironwood_commitments\":{},\"ironwood_hash_insert_us\":{},\"ironwood_parallel_build_us\":{},\"ironwood_parallel_worker_active_us\":{},\"ironwood_prepared_insert_us\":{},\"ironwood_prepared_trees\":{},\"ironwood_checkpoints\":{},\"ironwood_checkpoint_us\":{},\"ironwood_dirty_shards\":{},\"ironwood_dirty_bytes\":{},\"ironwood_flush_us\":{},\"ironwood_cache_estimated_bytes\":{},\"ironwood_peak_cache_estimated_bytes\":{},\"transaction_lock_wait_us\":{},\"sqlite_commit_us\":{},\"cache_reused\":{},\"cache_evicted_after_commit\":{}",
                operation,
                telemetry.sapling.preload_discovery.as_micros(),
                telemetry.sapling.shard_loading.as_micros(),
                telemetry.sapling.cache_hits,
                telemetry.sapling.cache_misses,
                telemetry.sapling.cache_evictions,
                telemetry.sapling.commitment_count,
                telemetry.sapling.commitment_insert.as_micros(),
                telemetry.sapling.parallel_construction.as_micros(),
                telemetry.sapling.parallel_worker_active.as_micros(),
                telemetry.sapling.prepared_tree_insert.as_micros(),
                telemetry.sapling.prepared_tree_count,
                telemetry.sapling.checkpoint_count,
                telemetry.sapling.checkpoint_processing.as_micros(),
                telemetry.sapling.dirty_shards,
                telemetry.sapling.dirty_encoded_bytes,
                telemetry.sapling.flush.as_micros(),
                telemetry.sapling.cache_bytes,
                telemetry.sapling.peak_cache_bytes,
                telemetry.ironwood.preload_discovery.as_micros(),
                telemetry.ironwood.shard_loading.as_micros(),
                telemetry.ironwood.cache_hits,
                telemetry.ironwood.cache_misses,
                telemetry.ironwood.cache_evictions,
                telemetry.ironwood.commitment_count,
                telemetry.ironwood.commitment_insert.as_micros(),
                telemetry.ironwood.parallel_construction.as_micros(),
                telemetry.ironwood.parallel_worker_active.as_micros(),
                telemetry.ironwood.prepared_tree_insert.as_micros(),
                telemetry.ironwood.prepared_tree_count,
                telemetry.ironwood.checkpoint_count,
                telemetry.ironwood.checkpoint_processing.as_micros(),
                telemetry.ironwood.dirty_shards,
                telemetry.ironwood.dirty_encoded_bytes,
                telemetry.ironwood.flush.as_micros(),
                telemetry.ironwood.cache_bytes,
                telemetry.ironwood.peak_cache_bytes,
                telemetry.transaction_lock_wait.as_micros(),
                telemetry.sqlite_commit.as_micros(),
                telemetry.cache_reused,
                telemetry.cache_evicted_after_commit,
            ),
        );
    }
}

fn apply_pool_batches<S, H, const DEPTH: u8, const SHARD_HEIGHT: u8>(
    tree: &mut ShardTree<S, DEPTH, SHARD_HEIGHT>,
    batches: &[ShardtreeBatch],
    max_committed_height: Option<u32>,
    verified_roots: &[VerifiedSubtreeRoot<H>],
    pool_batch: impl for<'a> Fn(
            &'a ShardtreeBatch,
        ) -> (Option<Position>, &'a [(H, Retention<BlockHeight>)], bool)
        + Copy,
    pool_name: &str,
) -> Result<ShardtreePoolWork>
where
    S: shardtree::store::ShardStore<H = H, CheckpointId = BlockHeight>,
    S::Error: std::fmt::Display,
    H: incrementalmerkletree::Hashable + Clone + PartialEq,
{
    // batch_insert preserves every embedded checkpoint retention marker, so one
    // contiguous run avoids repeating the same shard-store work for every block.
    let mut telemetry = ShardtreePoolWork::default();
    let mut run_start = None;
    let mut run_first_batch = None;
    let mut run_leaf_count = 0usize;
    let mut ordered_roots = verified_roots.iter().collect::<Vec<_>>();
    ordered_roots.sort_by_key(|root| (root.end_height, root.index));
    let mut next_root = 0usize;

    let flush_run = |tree: &mut ShardTree<S, DEPTH, SHARD_HEIGHT>,
                     run_start: &mut Option<Position>,
                     run_first_batch: &mut Option<usize>,
                     run_leaf_count: &mut usize,
                     run_end_batch: usize,
                     telemetry: &mut ShardtreePoolWork|
     -> Result<()> {
        let Some(start_position) = run_start.take() else {
            return Ok(());
        };
        let first_batch = run_first_batch
            .take()
            .expect("a pending ShardTree run has a first batch");
        let leaves = batches[first_batch..run_end_batch]
            .iter()
            .flat_map(|batch| pool_batch(batch).1.iter().cloned());
        let insert_start = Instant::now();
        tree.batch_insert(start_position, leaves).map_err(|e| {
            Error::Sync(format!(
                "Failed to batch insert {} commitments into shardtree: {}",
                pool_name, e
            ))
        })?;
        telemetry.commitment_insert += insert_start.elapsed();
        telemetry.commitment_count = telemetry
            .commitment_count
            .saturating_add(*run_leaf_count as u64);
        *run_leaf_count = 0;
        Ok(())
    };

    for (batch_index, batch) in batches.iter().enumerate() {
        let checkpoint_height = u32::try_from(batch.height).map_err(|_| {
            Error::Sync(format!(
                "Checkpoint height {} exceeds u32::MAX",
                batch.height
            ))
        })?;
        if max_committed_height.is_some_and(|max_h| checkpoint_height <= max_h) {
            flush_run(
                tree,
                &mut run_start,
                &mut run_first_batch,
                &mut run_leaf_count,
                batch_index,
                &mut telemetry,
            )?;
            while ordered_roots
                .get(next_root)
                .is_some_and(|root| root.end_height <= batch.height)
            {
                next_root = next_root.saturating_add(1);
            }
            continue;
        }

        while let Some(root) = ordered_roots
            .get(next_root)
            .filter(|root| root.end_height <= batch.height)
        {
            flush_run(
                tree,
                &mut run_start,
                &mut run_first_batch,
                &mut run_leaf_count,
                batch_index,
                &mut telemetry,
            )?;
            insert_verified_pool_root(tree, root, pool_name)?;
            next_root = next_root.saturating_add(1);
        }

        let (start_position, leaves, empty_checkpoint) = pool_batch(batch);
        if !leaves.is_empty() {
            let start_position = start_position.ok_or_else(|| {
                Error::Sync(format!(
                    "Missing {} start position for shardtree batch at height {}",
                    pool_name, batch.height
                ))
            })?;
            let expected_start = run_start.map(|start| {
                Position::from(u64::from(start).saturating_add(run_leaf_count as u64))
            });
            if expected_start.is_some_and(|expected| expected != start_position) {
                flush_run(
                    tree,
                    &mut run_start,
                    &mut run_first_batch,
                    &mut run_leaf_count,
                    batch_index,
                    &mut telemetry,
                )?;
            }
            if run_start.is_none() {
                run_start = Some(start_position);
                run_first_batch = Some(batch_index);
            }
            run_leaf_count = run_leaf_count.saturating_add(leaves.len());
        }

        if empty_checkpoint {
            flush_run(
                tree,
                &mut run_start,
                &mut run_first_batch,
                &mut run_leaf_count,
                batch_index.saturating_add(1),
                &mut telemetry,
            )?;
            let checkpoint_start = Instant::now();
            tree.checkpoint(BlockHeight::from(checkpoint_height))
                .map_err(|e| {
                    Error::Sync(format!(
                        "Failed to checkpoint {} shardtree: {}",
                        pool_name, e
                    ))
                })?;
            telemetry.checkpoint_processing += checkpoint_start.elapsed();
            telemetry.checkpoint_count = telemetry.checkpoint_count.saturating_add(1);
        }
    }

    flush_run(
        tree,
        &mut run_start,
        &mut run_first_batch,
        &mut run_leaf_count,
        batches.len(),
        &mut telemetry,
    )?;
    if let Some(root) = ordered_roots.get(next_root) {
        return Err(Error::Sync(format!(
            "Verified {} subtree root {} completed at height {} outside the persisted block batch",
            pool_name, root.index, root.end_height
        )));
    }
    Ok(telemetry)
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
    pool_name: &'static str,
    reason: &'static str,
    mut append_leaf: impl FnMut(&mut ShardtreeBatch, u64, H, Retention<BlockHeight>),
) {
    let started = Instant::now();
    let subtree_index = buffer.subtree_index;
    let leaf_count = buffer.buffered_leaves.len();
    let first_height = buffer
        .buffered_leaves
        .first()
        .map(|(height, _, _, _)| *height)
        .unwrap_or_default();
    let last_height = buffer
        .buffered_leaves
        .last()
        .map(|(height, _, _, _)| *height)
        .unwrap_or_default();
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
    if pirate_core::debug_log::is_enabled() {
        append_sync_decision_log(
            "sync.rs:flush_buffered_pool_leaves",
            "historical subtree leaves materialized",
            format!(
                "\"pool\":\"{}\",\"reason\":\"{}\",\"subtree_index\":{},\"leaves\":{},\"first_height\":{},\"last_height\":{},\"assemble_us\":{}",
                pool_name,
                reason,
                subtree_index,
                leaf_count,
                first_height,
                last_height,
                started.elapsed().as_micros()
            ),
        );
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
        if !buffer.leaves_emitted && !buffer.root_persisted {
            let mut dummy_current = ShardtreeBatch::new(u64::MAX);
            flush_buffered_pool_leaves(
                buffer,
                &mut dummy_current,
                &mut emitted,
                state.pool_name,
                "sync_finalize",
                append_leaf,
            );
            if dummy_current.height != u64::MAX {
                emitted.push(dummy_current);
            }
        }
    }
    state.passthrough_subtree = None;
    emitted
}

pub(super) struct HistoricalLeafSink<'a> {
    pub(super) current_batch: &'a mut ShardtreeBatch,
    pub(super) shardtree_batches: &'a mut Vec<ShardtreeBatch>,
}

#[cfg(test)]
fn calculate_subtree_root<H: Hashable + Clone, const DEPTH: u8>(leaves: &[H]) -> Option<H> {
    if leaves.len() != (1usize << DEPTH) {
        return None;
    }
    let mut tree = CommitmentTree::<H, DEPTH>::empty();
    for leaf in leaves {
        tree.append(leaf.clone()).ok()?;
    }
    Some(tree.root())
}

fn finish_historical_buffer<H: Hashable + Clone + Eq>(
    state: &mut HistoricalSubtreeSkipState<H>,
    completed: HistoricalSubtreeBuffer<H>,
    block_height: u64,
    current_batch: &mut ShardtreeBatch,
    shardtree_batches: &mut Vec<ShardtreeBatch>,
    append_leaf: impl FnMut(&mut ShardtreeBatch, u64, H, Retention<BlockHeight>) + Copy,
) -> Result<()> {
    let height_matches = completed.expected_end_height == block_height;
    if completed.verify_sample {
        let calculated_root = completed.sample_tree.as_ref().and_then(|tree| {
            (completed.sample_leaf_count == SHARD_LEAF_COUNT).then(|| tree.root())
        });
        let root_matches = completed
            .expected_root
            .as_ref()
            .zip(calculated_root.as_ref())
            .is_some_and(|(expected, actual)| expected == actual);
        if height_matches && root_matches {
            state.verified_samples.insert(completed.subtree_index);
            tracing::info!(
                "Verified sampled historical subtree root at index {}",
                completed.subtree_index
            );
        } else {
            state.grafting_disabled = true;
            tracing::warn!(
                "Historical subtree sample {} disagreed with compact blocks; disabling subtree grafting",
                completed.subtree_index
            );
            append_sync_decision_log(
                "sync.rs:process_historical_leaf",
                "subtree-root sample mismatch",
                format!(
                    "\"subtree_index\":{},\"height_matches\":{},\"root_matches\":{}",
                    completed.subtree_index, height_matches, root_matches
                ),
            );
        }
        // Sample leaves are appended as they arrive. This keeps the durable
        // ShardTree aligned with the sync cursor even when a sample spans
        // multiple network batches; this buffer exists only to verify the root.
        return Ok(());
    }

    if !height_matches {
        if completed.root_persisted {
            return Err(Error::Sync(format!(
                "trusted historical subtree {} completed at height {}, expected {}",
                completed.subtree_index, block_height, completed.expected_end_height
            )));
        }
        state.grafting_disabled = true;
        flush_buffered_pool_leaves(
            completed,
            current_batch,
            shardtree_batches,
            state.pool_name,
            "completion_height_mismatch",
            append_leaf,
        );
        return Ok(());
    }

    if !completed.root_persisted {
        let root = completed.expected_root.ok_or_else(|| {
            Error::Sync(format!(
                "verified historical subtree {} has no expected root",
                completed.subtree_index
            ))
        })?;
        state
            .pending_roots
            .push((completed.subtree_index, completed.expected_end_height, root));
    }

    // The root is already represented, or is now queued for persistence, so
    // the compact leaves can be discarded after checking completion height.
    Ok(())
}

pub(super) fn process_historical_leaf<H>(
    state: Option<&mut HistoricalSubtreeSkipState<H>>,
    position: u64,
    block_height: u64,
    node: H,
    retention: Retention<BlockHeight>,
    sink: HistoricalLeafSink<'_>,
    append_leaf: impl FnMut(&mut ShardtreeBatch, u64, H, Retention<BlockHeight>) + Copy,
) -> Result<()>
where
    H: Hashable + Clone + Eq,
{
    let HistoricalLeafSink {
        current_batch,
        shardtree_batches,
    } = sink;
    let Some(state) = state else {
        let mut append_leaf = append_leaf;
        append_leaf(current_batch, position, node, retention);
        return Ok(());
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
            return Ok(());
        }
        state.passthrough_subtree = None;
    }

    if let Some(buffer) = state.current_buffer.as_mut() {
        if buffer.subtree_index == subtree_index {
            if buffer.verify_sample {
                buffer
                    .sample_tree
                    .as_mut()
                    .expect("sample buffer has an accumulator")
                    .append(node.clone())
                    .map_err(|_| {
                        Error::Sync(format!(
                            "Historical subtree sample {} exceeded its commitment capacity",
                            subtree_index
                        ))
                    })?;
                buffer.sample_leaf_count = buffer.sample_leaf_count.saturating_add(1);
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if subtree_end {
                    let completed = state.current_buffer.take().expect("buffer exists");
                    finish_historical_buffer(
                        state,
                        completed,
                        block_height,
                        current_batch,
                        shardtree_batches,
                        append_leaf,
                    )?;
                }
                return Ok(());
            }
            if retention.is_marked() {
                if buffer.root_persisted {
                    // An already-grafted root cannot provide a witness for a
                    // newly discovered mark. Keep scanning without duplicating
                    // leaves; the tip integrity pass will queue leaf replay.
                    buffer
                        .buffered_leaves
                        .push((block_height, position, node, retention));
                    if subtree_end {
                        let completed = state.current_buffer.take().expect("buffer exists");
                        finish_historical_buffer(
                            state,
                            completed,
                            block_height,
                            current_batch,
                            shardtree_batches,
                            append_leaf,
                        )?;
                    }
                    return Ok(());
                }
                let flushed = state.current_buffer.take().expect("buffer exists");
                flush_buffered_pool_leaves(
                    flushed,
                    current_batch,
                    shardtree_batches,
                    state.pool_name,
                    "wallet_mark_discovered",
                    append_leaf,
                );
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if !subtree_end {
                    state.passthrough_subtree = Some(subtree_index);
                }
                return Ok(());
            }
            buffer
                .buffered_leaves
                .push((block_height, position, node, retention));
            if subtree_end {
                let completed = state.current_buffer.take().expect("buffer exists");
                finish_historical_buffer(
                    state,
                    completed,
                    block_height,
                    current_batch,
                    shardtree_batches,
                    append_leaf,
                )?;
            }
            return Ok(());
        }

        let flushed = state.current_buffer.take().expect("buffer exists");
        flush_buffered_pool_leaves(
            flushed,
            current_batch,
            shardtree_batches,
            state.pool_name,
            "subtree_discontinuity",
            append_leaf,
        );
    }

    if subtree_start && !state.grafting_disabled {
        if let Some(root) = state.roots_by_index.get(&subtree_index).cloned() {
            let is_unverified_sample_anchor = root.sample_anchor == Some(subtree_index)
                && !state.verified_samples.contains(&subtree_index);
            if state.leaf_backed_hints.contains(&subtree_index) && !is_unverified_sample_anchor {
                append_sync_decision_log(
                    "sync.rs:process_historical_leaf",
                    "subtree graft bypassed for historical wallet mark",
                    format!(
                        "\"pool\":\"{}\",\"subtree_index\":{},\"start_height\":{}",
                        state.pool_name, subtree_index, block_height
                    ),
                );
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if !subtree_end {
                    state.passthrough_subtree = Some(subtree_index);
                }
                return Ok(());
            }
            let verify_sample = root.expected_root.is_some()
                && root.sample_anchor == Some(subtree_index)
                && !state.verified_samples.contains(&subtree_index);
            let sample_verified = root.sample_anchor.is_some_and(|sample_anchor| {
                sample_anchor != subtree_index && state.verified_samples.contains(&sample_anchor)
            });
            let can_skip = root.trusted || sample_verified;
            if !verify_sample && !can_skip {
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if !subtree_end {
                    state.passthrough_subtree = Some(subtree_index);
                }
                return Ok(());
            }
            if verify_sample {
                let mut sample_tree = CommitmentTree::empty();
                sample_tree.append(node.clone()).map_err(|_| {
                    Error::Sync(format!(
                        "Historical subtree sample {} exceeded its commitment capacity",
                        subtree_index
                    ))
                })?;
                let buffer = HistoricalSubtreeBuffer {
                    subtree_index,
                    expected_end_height: root.expected_end_height,
                    expected_root: root.expected_root,
                    verify_sample: true,
                    root_persisted: false,
                    leaves_emitted: true,
                    sample_tree: Some(sample_tree),
                    sample_leaf_count: 1,
                    buffered_leaves: Vec::new(),
                };
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if subtree_end {
                    finish_historical_buffer(
                        state,
                        buffer,
                        block_height,
                        current_batch,
                        shardtree_batches,
                        append_leaf,
                    )?;
                } else {
                    state.current_buffer = Some(buffer);
                }
            } else if retention.is_marked() && !root.trusted {
                let mut append_leaf = append_leaf;
                append_leaf(current_batch, position, node, retention);
                if !subtree_end {
                    state.passthrough_subtree = Some(subtree_index);
                }
            } else {
                let buffer = HistoricalSubtreeBuffer {
                    subtree_index,
                    expected_end_height: root.expected_end_height,
                    expected_root: root.expected_root,
                    verify_sample: false,
                    root_persisted: root.trusted,
                    leaves_emitted: false,
                    sample_tree: None,
                    sample_leaf_count: 0,
                    buffered_leaves: vec![(block_height, position, node, retention)],
                };
                if subtree_end {
                    finish_historical_buffer(
                        state,
                        buffer,
                        block_height,
                        current_batch,
                        shardtree_batches,
                        append_leaf,
                    )?;
                } else {
                    state.current_buffer = Some(buffer);
                }
            }
            return Ok(());
        }
    }

    let mut append_leaf = append_leaf;
    append_leaf(current_batch, position, node, retention);
    Ok(())
}

fn load_root_backed_subtree_index<H>(
    conn: &rusqlite::Connection,
    table_prefix: &'static str,
    max_end_height: u64,
) -> Result<HashMap<u64, HistoricalSubtreeRoot<H>>> {
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
            roots.insert(
                shard_index_u64,
                HistoricalSubtreeRoot {
                    expected_end_height: end_height_u64,
                    expected_root: None,
                    sample_anchor: None,
                    trusted: true,
                },
            );
        }
    }
    Ok(roots)
}

fn parse_subtree_root_hash<H: HashSer>(bytes: &[u8]) -> Result<H> {
    H::read(Cursor::new(bytes))
        .map_err(|e| Error::Sync(format!("Failed to parse subtree root hash: {}", e)))
}

async fn fetch_subtree_roots<H: HashSer + Hashable + Clone + Eq>(
    client: &LightClient,
    protocol: crate::proto_types::ShieldedProtocol,
    start_index: u32,
    max_end_height: u64,
) -> Result<HashMap<u64, HistoricalSubtreeRoot<H>>> {
    let roots = client.get_subtree_roots(start_index, protocol, 0).await?;
    let mut parsed = HashMap::new();
    for (offset, root) in roots.into_iter().enumerate() {
        if root.completing_block_height > max_end_height {
            break;
        }
        let parsed_hash = parse_subtree_root_hash::<H>(&root.root_hash)?;
        let subtree_index = u64::from(start_index).saturating_add(offset as u64);
        let sample_anchor = u64::from(start_index).saturating_add(
            (offset as u64 / SUBTREE_ROOT_SAMPLE_INTERVAL) * SUBTREE_ROOT_SAMPLE_INTERVAL,
        );
        parsed.insert(
            subtree_index,
            HistoricalSubtreeRoot {
                expected_end_height: root.completing_block_height,
                expected_root: Some(parsed_hash),
                sample_anchor: Some(sample_anchor),
                trusted: false,
            },
        );
    }
    Ok(parsed)
}

async fn fetch_subtree_roots_with_timeout<H: HashSer + Hashable + Clone + Eq>(
    client: &LightClient,
    protocol: crate::proto_types::ShieldedProtocol,
    start_index: u32,
    max_end_height: u64,
    timeout: Duration,
) -> Result<HashMap<u64, HistoricalSubtreeRoot<H>>> {
    match tokio::time::timeout(
        timeout,
        fetch_subtree_roots::<H>(client, protocol, start_index, max_end_height),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            client.record_subtree_root_timeout(protocol);
            Err(Error::Network(format!(
                "Historical {:?} subtree-root prefill timed out after {:?}",
                protocol, timeout
            )))
        }
    }
}

pub(super) fn prepare_historical_subtree_roots(
    conn: &rusqlite::Connection,
    sapling_position: u64,
    orchard_position: u64,
    end_height: u64,
    sapling_leaf_backed_hints: &HashSet<u64>,
    ironwood_leaf_backed_hints: &HashSet<u64>,
    fetch_ironwood: bool,
) -> Result<(HistoricalPrefillState, Option<HistoricalSubtreeRootRequest>)> {
    let historical_ceiling = end_height.saturating_sub(SHARDTREE_PRUNING_DEPTH as u64);
    if historical_ceiling == 0 {
        append_sync_decision_log(
            "sync.rs:prefill_historical_subtree_roots",
            "subtree-root prefill skipped",
            "\"reason\":\"no_historical_range\",\"historical_ceiling\":0".to_string(),
        );
        return Ok((
            HistoricalPrefillState {
                sapling: HistoricalSubtreeSkipState::new(HashMap::new())
                    .with_leaf_backed_hints("sapling", sapling_leaf_backed_hints),
                orchard: HistoricalSubtreeSkipState::new(HashMap::new())
                    .with_leaf_backed_hints("ironwood", ironwood_leaf_backed_hints),
                sapling_prefetched: 0,
                orchard_prefetched: 0,
            },
            None,
        ));
    }

    let start_sapling_index = sapling_position.div_ceil(SHARD_LEAF_COUNT) as u32;
    let start_orchard_index = orchard_position.div_ceil(SHARD_LEAF_COUNT) as u32;
    let sapling_roots_by_index = load_root_backed_subtree_index::<SaplingNode>(
        conn,
        SAPLING_TABLE_PREFIX,
        historical_ceiling,
    )?;
    let orchard_roots_by_index = load_root_backed_subtree_index::<MerkleHashOrchard>(
        conn,
        ORCHARD_TABLE_PREFIX,
        historical_ceiling,
    )?;

    Ok((
        HistoricalPrefillState {
            sapling: HistoricalSubtreeSkipState::new(sapling_roots_by_index)
                .with_leaf_backed_hints("sapling", sapling_leaf_backed_hints),
            orchard: HistoricalSubtreeSkipState::new(orchard_roots_by_index)
                .with_leaf_backed_hints("ironwood", ironwood_leaf_backed_hints),
            sapling_prefetched: 0,
            orchard_prefetched: 0,
        },
        Some(HistoricalSubtreeRootRequest {
            start_sapling_index,
            start_orchard_index,
            historical_ceiling,
            fetch_sapling: true,
            fetch_ironwood,
        }),
    ))
}

pub(super) async fn fetch_remote_historical_subtree_roots(
    client: &LightClient,
    request: HistoricalSubtreeRootRequest,
    timeout: Duration,
) -> RemoteHistoricalSubtreeRoots {
    let sapling = async {
        if request.fetch_sapling {
            fetch_subtree_roots_with_timeout::<SaplingNode>(
                client,
                crate::proto_types::ShieldedProtocol::Sapling,
                request.start_sapling_index,
                request.historical_ceiling,
                timeout,
            )
            .await
        } else {
            Ok(HashMap::new())
        }
    };
    let ironwood = async {
        if request.fetch_ironwood {
            fetch_subtree_roots_with_timeout::<MerkleHashOrchard>(
                client,
                crate::proto_types::ShieldedProtocol::Ironwood,
                request.start_orchard_index,
                request.historical_ceiling,
                timeout,
            )
            .await
        } else {
            Ok(HashMap::new())
        }
    };
    let (sapling, ironwood) = tokio::join!(sapling, ironwood);

    let sapling = match sapling {
        Ok(roots) => roots,
        Err(error) => {
            tracing::warn!(
                "Historical Sapling subtree-root prefill unavailable; continuing with leaf sync: {}",
                error
            );
            append_sync_decision_log(
                "sync.rs:prefill_historical_subtree_roots",
                "subtree-root prefill unavailable, falling back",
                format!(
                    "\"pool\":\"sapling\",\"start_index\":{},\"historical_ceiling\":{},\"error\":\"{}\"",
                    request.start_sapling_index,
                    request.historical_ceiling,
                    format!("{}", error).replace('"', "'")
                ),
            );
            HashMap::new()
        }
    };
    let ironwood = match ironwood {
        Ok(roots) => roots,
        Err(error) => {
            tracing::warn!(
                "Historical Ironwood subtree-root prefill unavailable; continuing with leaf sync: {}",
                error
            );
            append_sync_decision_log(
                "sync.rs:prefill_historical_subtree_roots",
                "subtree-root prefill unavailable, falling back",
                format!(
                    "\"pool\":\"ironwood\",\"start_index\":{},\"historical_ceiling\":{},\"error\":\"{}\"",
                    request.start_orchard_index,
                    request.historical_ceiling,
                    format!("{}", error).replace('"', "'")
                ),
            );
            HashMap::new()
        }
    };

    tracing::info!(
        "Remote historical subtree roots ready: sapling={}, ironwood={}, sapling_start_index={}, ironwood_start_index={}, historical_ceiling={}",
        sapling.len(),
        ironwood.len(),
        request.start_sapling_index,
        request.start_orchard_index,
        request.historical_ceiling
    );
    append_sync_decision_log(
        "sync.rs:prefill_historical_subtree_roots",
        "remote subtree-root prefill complete",
        format!(
            "\"sapling_prefetched\":{},\"ironwood_prefetched\":{},\"sapling_start_index\":{},\"ironwood_start_index\":{},\"historical_ceiling\":{}",
            sapling.len(),
            ironwood.len(),
            request.start_sapling_index,
            request.start_orchard_index,
            request.historical_ceiling
        ),
    );

    RemoteHistoricalSubtreeRoots { sapling, ironwood }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shardtree::store::{memory::MemoryShardStore, Checkpoint, ShardStore, TreeState};
    use shardtree::{LocatedTree, RetentionFlags, Tree};
    use std::hint::black_box;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Barrier;

    type MemorySaplingTree = ShardTree<
        MemoryShardStore<SaplingNode, BlockHeight>,
        { NOTE_COMMITMENT_TREE_DEPTH },
        SAPLING_SHARD_HEIGHT,
    >;
    type MemoryOrchardTree = ShardTree<
        MemoryShardStore<MerkleHashOrchard, BlockHeight>,
        { NOTE_COMMITMENT_TREE_DEPTH },
        ORCHARD_SHARD_HEIGHT,
    >;

    #[tokio::test]
    async fn timed_out_subtree_pool_is_cached_without_affecting_other_pools() {
        let client = LightClient::new("http://127.0.0.1:1".to_string());
        let result = fetch_subtree_roots_with_timeout::<SaplingNode>(
            &client,
            crate::proto_types::ShieldedProtocol::Sapling,
            0,
            1_000,
            Duration::ZERO,
        )
        .await;

        assert!(result.is_err());
        assert!(!client.subtree_root_probe_allowed(crate::proto_types::ShieldedProtocol::Sapling));
        assert!(client.subtree_root_probe_allowed(crate::proto_types::ShieldedProtocol::Ironwood));
    }

    fn checkpoint(height: u32, marking: Marking) -> Retention<BlockHeight> {
        Retention::Checkpoint {
            id: BlockHeight::from(height),
            marking,
        }
    }

    fn sample_batches() -> Vec<ShardtreeBatch> {
        let mut first = ShardtreeBatch::new(100);
        first.checkpoint_id = Some(BlockHeight::from(100));
        append_sapling_leaf(&mut first, 0, SaplingNode::empty_leaf(), Retention::Marked);
        append_sapling_leaf(
            &mut first,
            1,
            SaplingNode::empty_leaf(),
            checkpoint(100, Marking::None),
        );
        append_orchard_leaf(
            &mut first,
            0,
            MerkleHashOrchard::empty_leaf(),
            checkpoint(100, Marking::Marked),
        );

        let mut second = ShardtreeBatch::new(101);
        second.checkpoint_id = Some(BlockHeight::from(101));
        second.sapling_empty_checkpoint = true;
        append_orchard_leaf(
            &mut second,
            1,
            MerkleHashOrchard::empty_leaf(),
            checkpoint(101, Marking::None),
        );

        let mut third = ShardtreeBatch::new(102);
        third.checkpoint_id = Some(BlockHeight::from(102));
        append_sapling_leaf(
            &mut third,
            2,
            SaplingNode::empty_leaf(),
            checkpoint(102, Marking::None),
        );
        third.orchard_empty_checkpoint = true;

        vec![first, second, third]
    }

    fn apply_per_block_reference(
        sapling_tree: &mut MemorySaplingTree,
        orchard_tree: &mut MemoryOrchardTree,
        batches: &[ShardtreeBatch],
    ) {
        for batch in batches {
            let checkpoint_id = BlockHeight::from(batch.height as u32);
            if !batch.sapling.is_empty() {
                sapling_tree
                    .batch_insert(
                        batch.sapling_start_position.unwrap(),
                        batch.sapling.iter().cloned(),
                    )
                    .unwrap();
            }
            if batch.sapling_empty_checkpoint {
                sapling_tree.checkpoint(checkpoint_id).unwrap();
            }
            if !batch.orchard.is_empty() {
                orchard_tree
                    .batch_insert(
                        batch.orchard_start_position.unwrap(),
                        batch.orchard.iter().cloned(),
                    )
                    .unwrap();
            }
            if batch.orchard_empty_checkpoint {
                orchard_tree.checkpoint(checkpoint_id).unwrap();
            }
        }
    }

    fn empty_trees() -> (MemorySaplingTree, MemoryOrchardTree) {
        (
            ShardTree::new(MemoryShardStore::empty(), SHARDTREE_PRUNING_DEPTH),
            ShardTree::new(MemoryShardStore::empty(), SHARDTREE_PRUNING_DEPTH),
        )
    }

    fn test_construction_pool() -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap()
    }

    #[test]
    fn adaptive_construction_ranges_are_complete_aligned_and_shard_bounded() {
        for parallelism in [1usize, 2, 4, 16] {
            for start in [0u64, 1, 255, 1_000, SHARD_LEAF_COUNT - 7] {
                for total in [1usize, 17, 255, 1_000, 6_346, 9_017] {
                    let ranges = balanced_construction_ranges::<SAPLING_SHARD_HEIGHT>(
                        Position::from(start),
                        total,
                        parallelism,
                        "Sapling",
                    )
                    .unwrap();
                    let mut expected_start = start;
                    let mut covered = 0usize;
                    for range in ranges {
                        assert_eq!(range.start, expected_start);
                        assert!(range.len.is_power_of_two());
                        assert_eq!(range.start % range.len as u64, 0);
                        assert!(range.len <= adaptive_tree_chunk_limit(total, parallelism));
                        assert_eq!(
                            range.start / SHARD_LEAF_COUNT,
                            (range.start + range.len as u64 - 1) / SHARD_LEAF_COUNT
                        );
                        expected_start += range.len as u64;
                        covered += range.len;
                    }
                    assert_eq!(covered, total);
                    assert_eq!(expected_start, start + total as u64);
                }
            }
        }
    }

    #[test]
    fn adaptive_construction_ranges_balance_representative_sync_runs() {
        let total = 6_346usize;
        let narrow = balanced_construction_ranges::<SAPLING_SHARD_HEIGHT>(
            Position::from(0),
            total,
            1,
            "Sapling",
        )
        .unwrap();
        let wide = balanced_construction_ranges::<SAPLING_SHARD_HEIGHT>(
            Position::from(0),
            total,
            16,
            "Sapling",
        )
        .unwrap();
        let narrow_max = narrow.iter().map(|range| range.len).max().unwrap();
        let wide_max = wide.iter().map(|range| range.len).max().unwrap();

        assert_eq!(narrow_max, MAX_PARALLEL_TREE_CHUNK_LEAVES);
        assert_eq!(wide_max, MIN_PARALLEL_TREE_CHUNK_LEAVES);
        assert!(wide.len() > narrow.len());
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct BenchSaplingNode(SaplingNode);

    impl Hashable for BenchSaplingNode {
        fn empty_leaf() -> Self {
            Self(SaplingNode::empty_leaf())
        }

        fn combine(level: Level, lhs: &Self, rhs: &Self) -> Self {
            Self(SaplingNode::combine(level, &lhs.0, &rhs.0))
        }

        fn empty_root(level: Level) -> Self {
            Self(SaplingNode::empty_root(level))
        }
    }

    static BENCH_COMBINE_COUNT: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct CountingSaplingNode(SaplingNode);

    impl Hashable for CountingSaplingNode {
        fn empty_leaf() -> Self {
            Self(SaplingNode::empty_leaf())
        }

        fn combine(level: Level, lhs: &Self, rhs: &Self) -> Self {
            BENCH_COMBINE_COUNT.fetch_add(1, Ordering::Relaxed);
            Self(SaplingNode::combine(level, &lhs.0, &rhs.0))
        }

        fn empty_root(level: Level) -> Self {
            Self(SaplingNode::empty_root(level))
        }
    }

    fn build_from_iter_ephemeral_segments<H>(
        pool: &rayon::ThreadPool,
        total_leaves: usize,
        segment_leaves: usize,
    ) -> Vec<LocatedPrunableTree<H>>
    where
        H: Hashable + Clone + PartialEq + Send + Sync,
    {
        assert!(segment_leaves.is_power_of_two());
        assert_eq!(total_leaves % segment_leaves, 0);
        pool.install(|| {
            (0..total_leaves / segment_leaves)
                .into_par_iter()
                .map(|segment_index| {
                    let start = segment_index * segment_leaves;
                    let end = start + segment_leaves;
                    LocatedPrunableTree::<H>::from_iter(
                        Position::from(start as u64)..Position::from(end as u64),
                        Level::from(SAPLING_SHARD_HEIGHT),
                        std::iter::repeat_n(
                            (H::empty_leaf(), Retention::<BlockHeight>::Ephemeral),
                            segment_leaves,
                        ),
                    )
                    .expect("non-empty aligned segment")
                    .subtree
                })
                .collect()
        })
    }

    fn build_planned_ephemeral_segments<H>(
        pool: &rayon::ThreadPool,
        start_position: u64,
        total_leaves: usize,
        planned_parallelism: usize,
    ) -> Vec<LocatedPrunableTree<H>>
    where
        H: Hashable + Clone + PartialEq + Send + Sync,
    {
        let ranges = balanced_construction_ranges::<SAPLING_SHARD_HEIGHT>(
            Position::from(start_position),
            total_leaves,
            planned_parallelism,
            "Sapling",
        )
        .unwrap();
        pool.install(|| {
            ranges
                .into_par_iter()
                .map(|range| {
                    let end = range.start + range.len as u64;
                    LocatedPrunableTree::<H>::from_iter(
                        Position::from(range.start)..Position::from(end),
                        Level::from(SAPLING_SHARD_HEIGHT),
                        std::iter::repeat_n(
                            (H::empty_leaf(), Retention::<BlockHeight>::Ephemeral),
                            range.len,
                        ),
                    )
                    .expect("non-empty planned segment")
                    .subtree
                })
                .collect()
        })
    }

    fn build_pruned_ephemeral_segments<H>(
        pool: &rayon::ThreadPool,
        total_leaves: usize,
        segment_leaves: usize,
    ) -> Vec<LocatedPrunableTree<H>>
    where
        H: Hashable + Clone + PartialEq + Send + Sync,
    {
        assert!(segment_leaves.is_power_of_two());
        assert_eq!(total_leaves % segment_leaves, 0);
        let segment_level = segment_leaves.trailing_zeros() as u8;
        pool.install(|| {
            (0..total_leaves / segment_leaves)
                .into_par_iter()
                .map(|segment_index| {
                    let mut nodes = vec![H::empty_leaf(); segment_leaves];
                    let mut level = 0u8;
                    while nodes.len() > 1 {
                        nodes = nodes
                            .chunks_exact(2)
                            .map(|pair| H::combine(Level::from(level), &pair[0], &pair[1]))
                            .collect();
                        level += 1;
                    }
                    let root = nodes.pop().expect("segment contains leaves");
                    LocatedTree::from_parts(
                        Address::from_parts(Level::from(segment_level), segment_index as u64),
                        Tree::leaf((root, RetentionFlags::EPHEMERAL)),
                    )
                    .expect("aligned pruned segment")
                })
                .collect()
        })
    }

    fn best_of_three(mut run: impl FnMut()) -> Duration {
        (0..3)
            .map(|_| {
                let started = Instant::now();
                run();
                started.elapsed()
            })
            .min()
            .expect("three benchmark samples")
    }

    /// Manual release-mode microbenchmark for evaluating ShardTree construction changes.
    ///
    /// Run with:
    /// `cargo test -p pirate-sync-lightd benchmark_ephemeral_sapling_construction --release -- --ignored --nocapture`
    #[test]
    #[ignore = "manual performance harness"]
    fn benchmark_ephemeral_sapling_construction() {
        let threads = std::env::var("SHARDTREE_BENCH_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|threads| *threads > 0)
            .unwrap_or_else(|| num_cpus::get().max(1));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let correctness_leaves = 4_096;

        BENCH_COMBINE_COUNT.store(0, Ordering::Relaxed);
        let reference = build_from_iter_ephemeral_segments::<CountingSaplingNode>(
            &pool,
            correctness_leaves,
            correctness_leaves,
        );
        let reference_combines = BENCH_COMBINE_COUNT.swap(0, Ordering::Relaxed);
        let candidate = build_pruned_ephemeral_segments::<CountingSaplingNode>(
            &pool,
            correctness_leaves,
            correctness_leaves,
        );
        let candidate_combines = BENCH_COMBINE_COUNT.swap(0, Ordering::Relaxed);
        assert_eq!(candidate, reference);
        assert_eq!(reference_combines, (correctness_leaves - 1) as u64);
        assert_eq!(candidate_combines, reference_combines);

        let total_leaves = 65_536;
        eprintln!("sapling construction benchmark: threads={threads}, leaves={total_leaves}");
        for segment_leaves in [256, 512, 1_024, 2_048, 4_096, 8_192, 16_384] {
            let elapsed = best_of_three(|| {
                black_box(build_from_iter_ephemeral_segments::<BenchSaplingNode>(
                    &pool,
                    total_leaves,
                    segment_leaves,
                ));
            });
            eprintln!(
                "from_iter segment={segment_leaves:>5}: {:>8.3} ms",
                elapsed.as_secs_f64() * 1_000.0
            );
        }

        let representative_start = 1_000u64;
        let representative_leaves = 6_346usize;
        let fixed_elapsed = best_of_three(|| {
            black_box(build_planned_ephemeral_segments::<BenchSaplingNode>(
                &pool,
                representative_start,
                representative_leaves,
                1,
            ));
        });
        let adaptive_elapsed = best_of_three(|| {
            black_box(build_planned_ephemeral_segments::<BenchSaplingNode>(
                &pool,
                representative_start,
                representative_leaves,
                threads,
            ));
        });
        eprintln!(
            "representative unaligned run: leaves={representative_leaves}, fixed4096={:.3} ms, adaptive={:.3} ms, speedup={:.2}x",
            fixed_elapsed.as_secs_f64() * 1_000.0,
            adaptive_elapsed.as_secs_f64() * 1_000.0,
            fixed_elapsed.as_secs_f64() / adaptive_elapsed.as_secs_f64(),
        );

        let candidate_elapsed = best_of_three(|| {
            black_box(build_pruned_ephemeral_segments::<BenchSaplingNode>(
                &pool,
                total_leaves,
                4_096,
            ));
        });
        eprintln!(
            "pruned-root segment= 4096: {:>8.3} ms",
            candidate_elapsed.as_secs_f64() * 1_000.0
        );

        let standalone = best_of_three(|| {
            black_box(build_from_iter_ephemeral_segments::<BenchSaplingNode>(
                &pool,
                total_leaves,
                1_024,
            ));
        });
        let (overlap_wall, first_outer, second_outer) = (0..3)
            .map(|_| {
                let barrier = Barrier::new(3);
                std::thread::scope(|scope| {
                    let first = scope.spawn(|| {
                        barrier.wait();
                        let started = Instant::now();
                        black_box(build_from_iter_ephemeral_segments::<BenchSaplingNode>(
                            &pool,
                            total_leaves,
                            1_024,
                        ));
                        started.elapsed()
                    });
                    let second = scope.spawn(|| {
                        barrier.wait();
                        let started = Instant::now();
                        black_box(build_from_iter_ephemeral_segments::<BenchSaplingNode>(
                            &pool,
                            total_leaves,
                            1_024,
                        ));
                        started.elapsed()
                    });
                    let pair_started = Instant::now();
                    barrier.wait();
                    let first_outer = first.join().unwrap();
                    let second_outer = second.join().unwrap();
                    (pair_started.elapsed(), first_outer, second_outer)
                })
            })
            .min_by_key(|(wall, _, _)| *wall)
            .expect("three overlap samples");
        eprintln!(
            "shared-pool overlap segment= 1024: standalone={:.3} ms, pair_wall={:.3} ms, outer_a={:.3} ms, outer_b={:.3} ms, outer_sum/wall={:.2}",
            standalone.as_secs_f64() * 1_000.0,
            overlap_wall.as_secs_f64() * 1_000.0,
            first_outer.as_secs_f64() * 1_000.0,
            second_outer.as_secs_f64() * 1_000.0,
            (first_outer + second_outer).as_secs_f64() / overlap_wall.as_secs_f64(),
        );
    }

    fn apply_parallel_test_batches(
        sapling_tree: &mut MemorySaplingTree,
        orchard_tree: &mut MemoryOrchardTree,
        batches: Vec<ShardtreeBatch>,
        batch_end_height: Option<u64>,
        max_committed_heights: CommittedCheckpointHeights,
        verified_roots: &VerifiedSubtreeRoots,
    ) -> ShardtreePersistResult {
        let prepared = prepare_parallel_shardtree_insertions(
            &test_construction_pool(),
            batches,
            batch_end_height,
            max_committed_heights,
            verified_roots,
        )
        .unwrap();
        apply_prepared_shardtree_insertions_to_trees(sapling_tree, orchard_tree, prepared).unwrap()
    }

    #[test]
    fn parallel_insertion_matches_coalesced_roots_checkpoints_and_witnesses() {
        let batches = sample_batches();
        let roots = VerifiedSubtreeRoots::default();
        let (mut expected_sapling, mut expected_orchard) = empty_trees();
        let (mut actual_sapling, mut actual_orchard) = empty_trees();
        let expected = apply_shardtree_batches_to_trees(
            &mut expected_sapling,
            &mut expected_orchard,
            &batches,
            Some(102),
            CommittedCheckpointHeights::default(),
            &roots,
        )
        .unwrap();
        let actual = apply_parallel_test_batches(
            &mut actual_sapling,
            &mut actual_orchard,
            batches,
            Some(102),
            CommittedCheckpointHeights::default(),
            &roots,
        );

        assert_eq!(
            actual.max_checkpointed_height,
            expected.max_checkpointed_height
        );
        assert_eq!(
            actual.batch_end_checkpointed,
            expected.batch_end_checkpointed
        );
        assert_eq!(
            actual_sapling.marked_positions().unwrap(),
            expected_sapling.marked_positions().unwrap()
        );
        assert_eq!(
            actual_orchard.marked_positions().unwrap(),
            expected_orchard.marked_positions().unwrap()
        );
        for height in 100..=102 {
            let checkpoint_id = BlockHeight::from(height);
            assert_eq!(
                actual_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap(),
                expected_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap()
            );
            assert_eq!(
                actual_orchard
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap(),
                expected_orchard
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap()
            );
        }
        assert_eq!(
            actual_sapling
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap(),
            expected_sapling
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap()
        );
        assert_eq!(
            actual_orchard
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap(),
            expected_orchard
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap()
        );
    }

    #[test]
    fn parallel_insertion_splits_partial_shards_without_changing_witnesses() {
        let prefix_len = 1_000u64;
        let inserted_len = (MAX_PARALLEL_TREE_CHUNK_LEAVES * 2 + 17) as u64;
        let checkpoint_id = BlockHeight::from(500u32);
        let mut batch = ShardtreeBatch::new(500);
        batch.checkpoint_id = Some(checkpoint_id);
        for offset in 0..inserted_len {
            let position = prefix_len + offset;
            let sapling_retention = if offset == 123 {
                Retention::Marked
            } else if offset + 1 == inserted_len {
                checkpoint(500, Marking::None)
            } else {
                Retention::Ephemeral
            };
            append_sapling_leaf(
                &mut batch,
                position,
                SaplingNode::empty_leaf(),
                sapling_retention,
            );
            let ironwood_retention = if offset == 321 {
                Retention::Marked
            } else if offset + 1 == inserted_len {
                checkpoint(500, Marking::None)
            } else {
                Retention::Ephemeral
            };
            append_orchard_leaf(
                &mut batch,
                position,
                MerkleHashOrchard::empty_leaf(),
                ironwood_retention,
            );
        }

        let (mut expected_sapling, mut expected_orchard) = empty_trees();
        let (mut actual_sapling, mut actual_orchard) = empty_trees();
        let sapling_prefix =
            (0..prefix_len).map(|_| (SaplingNode::empty_leaf(), Retention::Ephemeral));
        let orchard_prefix =
            (0..prefix_len).map(|_| (MerkleHashOrchard::empty_leaf(), Retention::Ephemeral));
        expected_sapling
            .batch_insert(Position::from(0), sapling_prefix.clone())
            .unwrap();
        actual_sapling
            .batch_insert(Position::from(0), sapling_prefix)
            .unwrap();
        expected_orchard
            .batch_insert(Position::from(0), orchard_prefix.clone())
            .unwrap();
        actual_orchard
            .batch_insert(Position::from(0), orchard_prefix)
            .unwrap();

        let roots = VerifiedSubtreeRoots::default();
        let expected = apply_shardtree_batches_to_trees(
            &mut expected_sapling,
            &mut expected_orchard,
            std::slice::from_ref(&batch),
            Some(500),
            CommittedCheckpointHeights::default(),
            &roots,
        )
        .unwrap();
        let actual = apply_parallel_test_batches(
            &mut actual_sapling,
            &mut actual_orchard,
            vec![batch],
            Some(500),
            CommittedCheckpointHeights::default(),
            &roots,
        );

        assert!(actual.sapling_work.prepared_tree_count >= 3);
        assert!(actual.ironwood_work.prepared_tree_count >= 3);
        assert_eq!(actual.sapling_work.commitment_count, inserted_len);
        assert_eq!(actual.ironwood_work.commitment_count, inserted_len);
        assert_eq!(
            actual_sapling
                .root_at_checkpoint_id(&checkpoint_id)
                .unwrap(),
            expected_sapling
                .root_at_checkpoint_id(&checkpoint_id)
                .unwrap()
        );
        assert_eq!(
            actual_orchard
                .root_at_checkpoint_id(&checkpoint_id)
                .unwrap(),
            expected_orchard
                .root_at_checkpoint_id(&checkpoint_id)
                .unwrap()
        );
        for position in [prefix_len + 123, prefix_len + 321] {
            if position == prefix_len + 123 {
                assert_eq!(
                    actual_sapling
                        .witness_at_checkpoint_id(Position::from(position), &checkpoint_id)
                        .unwrap(),
                    expected_sapling
                        .witness_at_checkpoint_id(Position::from(position), &checkpoint_id)
                        .unwrap()
                );
            } else {
                assert_eq!(
                    actual_orchard
                        .witness_at_checkpoint_id(Position::from(position), &checkpoint_id)
                        .unwrap(),
                    expected_orchard
                        .witness_at_checkpoint_id(Position::from(position), &checkpoint_id)
                        .unwrap()
                );
            }
        }
        assert_eq!(
            actual.batch_end_checkpointed,
            expected.batch_end_checkpointed
        );
    }

    #[test]
    fn coalesced_insertion_matches_per_block_roots_checkpoints_and_witnesses() {
        let batches = sample_batches();
        let (mut expected_sapling, mut expected_orchard) = empty_trees();
        let (mut actual_sapling, mut actual_orchard) = empty_trees();

        apply_per_block_reference(&mut expected_sapling, &mut expected_orchard, &batches);
        let result = apply_shardtree_batches_to_trees(
            &mut actual_sapling,
            &mut actual_orchard,
            &batches,
            Some(102),
            CommittedCheckpointHeights::default(),
            &VerifiedSubtreeRoots::default(),
        )
        .unwrap();

        assert_eq!(result.max_checkpointed_height, Some(102));
        assert!(result.batch_end_checkpointed);
        assert_eq!(
            expected_sapling.marked_positions().unwrap(),
            actual_sapling.marked_positions().unwrap()
        );
        assert_eq!(
            expected_orchard.marked_positions().unwrap(),
            actual_orchard.marked_positions().unwrap()
        );
        for height in 100..=102 {
            let checkpoint_id = BlockHeight::from(height);
            assert_eq!(
                expected_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap(),
                actual_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap()
            );
            assert_eq!(
                expected_orchard
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap(),
                actual_orchard
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap()
            );
        }
        assert_eq!(
            expected_sapling
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap(),
            actual_sapling
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap()
        );
        assert_eq!(
            expected_orchard
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap(),
            actual_orchard
                .witness_at_checkpoint_id(Position::from(0), &BlockHeight::from(102))
                .unwrap()
        );
    }

    #[test]
    fn coalesced_insertion_flushes_before_a_position_gap() {
        let mut batches = sample_batches();
        batches[2].sapling_start_position = Some(Position::from(8));
        batches[2].sapling[0].1 = checkpoint(102, Marking::Marked);
        let (mut expected_sapling, mut expected_orchard) = empty_trees();
        let (mut actual_sapling, mut actual_orchard) = empty_trees();

        apply_per_block_reference(&mut expected_sapling, &mut expected_orchard, &batches);
        apply_shardtree_batches_to_trees(
            &mut actual_sapling,
            &mut actual_orchard,
            &batches,
            Some(102),
            CommittedCheckpointHeights::default(),
            &VerifiedSubtreeRoots::default(),
        )
        .unwrap();

        let expected_marks = expected_sapling.marked_positions().unwrap();
        let actual_marks = actual_sapling.marked_positions().unwrap();
        assert_eq!(expected_marks, actual_marks);
        assert!(actual_marks.contains(&Position::from(8)));
    }

    #[test]
    fn coalesced_insertion_matches_incremental_checkpoint_pruning() {
        let batches: Vec<_> = (0..6u32)
            .map(|offset| {
                let height = 200 + offset;
                let mut batch = ShardtreeBatch::new(u64::from(height));
                batch.checkpoint_id = Some(BlockHeight::from(height));
                append_sapling_leaf(
                    &mut batch,
                    u64::from(offset),
                    SaplingNode::empty_leaf(),
                    checkpoint(
                        height,
                        if offset == 0 {
                            Marking::Marked
                        } else {
                            Marking::None
                        },
                    ),
                );
                batch
            })
            .collect();
        let mut expected_sapling: MemorySaplingTree = ShardTree::new(MemoryShardStore::empty(), 2);
        let mut expected_orchard: MemoryOrchardTree = ShardTree::new(MemoryShardStore::empty(), 2);
        let mut actual_sapling: MemorySaplingTree = ShardTree::new(MemoryShardStore::empty(), 2);
        let mut actual_orchard: MemoryOrchardTree = ShardTree::new(MemoryShardStore::empty(), 2);

        apply_per_block_reference(&mut expected_sapling, &mut expected_orchard, &batches);
        apply_shardtree_batches_to_trees(
            &mut actual_sapling,
            &mut actual_orchard,
            &batches,
            Some(205),
            CommittedCheckpointHeights::default(),
            &VerifiedSubtreeRoots::default(),
        )
        .unwrap();

        for height in 200..=205 {
            let checkpoint_id = BlockHeight::from(height);
            assert_eq!(
                expected_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap(),
                actual_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap()
            );
        }
        assert_eq!(
            expected_sapling.marked_positions().unwrap(),
            actual_sapling.marked_positions().unwrap()
        );
        assert!(
            actual_sapling
                .marked_positions()
                .unwrap()
                .contains(&Position::from(0)),
            "an owned-note mark must survive checkpoint pruning"
        );
    }

    #[test]
    fn replay_cutoffs_are_applied_independently_per_pool() {
        let (mut sapling, mut orchard) = empty_trees();
        let mut batch = ShardtreeBatch::new(101);
        batch.checkpoint_id = Some(BlockHeight::from(101));
        append_sapling_leaf(
            &mut batch,
            0,
            SaplingNode::empty_leaf(),
            checkpoint(101, Marking::Marked),
        );
        append_orchard_leaf(
            &mut batch,
            0,
            MerkleHashOrchard::empty_leaf(),
            checkpoint(101, Marking::Marked),
        );

        let result = apply_shardtree_batches_to_trees(
            &mut sapling,
            &mut orchard,
            &[batch],
            Some(101),
            CommittedCheckpointHeights {
                sapling: Some(100),
                ironwood: Some(101),
            },
            &VerifiedSubtreeRoots::default(),
        )
        .unwrap();

        assert_eq!(result.sapling_work.commitment_count, 1);
        assert_eq!(result.ironwood_work.commitment_count, 0);
        assert!(sapling
            .marked_positions()
            .unwrap()
            .contains(&Position::from(0)));
        assert!(orchard.marked_positions().unwrap().is_empty());
    }

    #[test]
    fn batch_end_checkpoint_requires_both_pool_checkpoints() {
        let (mut sapling, mut orchard) = empty_trees();
        let mut batch = ShardtreeBatch::new(101);
        let checkpoint_id = BlockHeight::from(101u32);
        batch.checkpoint_id = Some(checkpoint_id);
        append_sapling_leaf(
            &mut batch,
            0,
            SaplingNode::empty_leaf(),
            checkpoint(101, Marking::Marked),
        );

        let result = apply_shardtree_batches_to_trees(
            &mut sapling,
            &mut orchard,
            &[batch],
            Some(101),
            CommittedCheckpointHeights::default(),
            &VerifiedSubtreeRoots::default(),
        )
        .unwrap();

        assert!(!result.batch_end_checkpointed);

        let (mut sapling, mut orchard) = empty_trees();
        let mut batch = ShardtreeBatch::new(101);
        batch.checkpoint_id = Some(checkpoint_id);
        append_sapling_leaf(
            &mut batch,
            0,
            SaplingNode::empty_leaf(),
            checkpoint(101, Marking::Marked),
        );
        batch.orchard_empty_checkpoint = true;
        let result = apply_shardtree_batches_to_trees(
            &mut sapling,
            &mut orchard,
            &[batch],
            Some(101),
            CommittedCheckpointHeights::default(),
            &VerifiedSubtreeRoots::default(),
        )
        .unwrap();

        assert!(result.batch_end_checkpointed);
    }

    #[test]
    fn verified_roots_do_not_leak_into_earlier_checkpoints() {
        let (mut actual_sapling, mut actual_orchard) = empty_trees();
        let (mut expected_sapling, _expected_orchard) = empty_trees();
        let subtree_root = SaplingNode::empty_root(SAPLING_SHARD_HEIGHT.into());

        let mut before_root = ShardtreeBatch::new(100);
        before_root.checkpoint_id = Some(BlockHeight::from(100));
        before_root.sapling_empty_checkpoint = true;
        before_root.orchard_empty_checkpoint = true;

        let mut completing_block = ShardtreeBatch::new(101);
        completing_block.checkpoint_id = Some(BlockHeight::from(101));
        append_sapling_leaf(
            &mut completing_block,
            SHARD_LEAF_COUNT,
            SaplingNode::empty_leaf(),
            checkpoint(101, Marking::None),
        );
        completing_block.orchard_empty_checkpoint = true;

        expected_sapling.checkpoint(BlockHeight::from(100)).unwrap();
        expected_sapling
            .insert(
                Address::from_parts(SAPLING_SHARD_HEIGHT.into(), 0),
                subtree_root,
            )
            .unwrap();
        expected_sapling
            .batch_insert(
                Position::from(SHARD_LEAF_COUNT),
                [(SaplingNode::empty_leaf(), checkpoint(101, Marking::None))].into_iter(),
            )
            .unwrap();

        let roots = VerifiedSubtreeRoots {
            sapling: vec![VerifiedSubtreeRoot {
                index: 0,
                end_height: 101,
                root: subtree_root,
            }],
            ironwood: Vec::new(),
        };
        apply_shardtree_batches_to_trees(
            &mut actual_sapling,
            &mut actual_orchard,
            &[before_root, completing_block],
            Some(101),
            CommittedCheckpointHeights::default(),
            &roots,
        )
        .unwrap();

        for height in [100u32, 101u32] {
            let checkpoint_id = BlockHeight::from(height);
            assert_eq!(
                actual_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap(),
                expected_sapling
                    .root_at_checkpoint_id(&checkpoint_id)
                    .unwrap()
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
