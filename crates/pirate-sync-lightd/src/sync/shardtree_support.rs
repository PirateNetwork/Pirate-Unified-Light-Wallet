use super::*;

const SHARD_LEAF_COUNT: u64 = 1u64 << SAPLING_SHARD_HEIGHT;

struct HistoricalSubtreeBuffer<H> {
    subtree_index: u64,
    expected_end_height: u64,
    buffered_leaves: Vec<(u64, u64, H, Retention<BlockHeight>)>,
}

pub(super) struct HistoricalSubtreeSkipState<H> {
    pub(super) roots_by_index: HashMap<u64, u64>,
    current_buffer: Option<HistoricalSubtreeBuffer<H>>,
    passthrough_subtree: Option<u64>,
}

impl<H> HistoricalSubtreeSkipState<H> {
    pub(super) fn new(roots_by_index: HashMap<u64, u64>) -> Self {
        Self {
            roots_by_index,
            current_buffer: None,
            passthrough_subtree: None,
        }
    }
}

pub(super) struct HistoricalPrefillState {
    pub(super) sapling: HistoricalSubtreeSkipState<SaplingNode>,
    pub(super) orchard: HistoricalSubtreeSkipState<MerkleHashOrchard>,
    pub(super) sapling_prefetched: usize,
    pub(super) orchard_prefetched: usize,
}

impl HistoricalPrefillState {
    pub(super) fn prefetched_any(&self) -> bool {
        self.sapling_prefetched > 0 || self.orchard_prefetched > 0
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
        crate::proto_types::ShieldedProtocol::Orchard,
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
