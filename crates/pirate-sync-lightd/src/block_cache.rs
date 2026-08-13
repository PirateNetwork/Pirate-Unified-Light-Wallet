//! Shared compact block cache for multi-wallet sync.

use crate::client::CompactBlockData;
use crate::{Error, Result};
use directories::ProjectDirs;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use prost::Message;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

const CACHE_RECORD_MAGIC: &[u8] = b"PWCB\x01";

pub struct BlockCache {
    path: PathBuf,
}

pub(crate) struct LoadedBlockRange {
    pub(crate) blocks: Vec<CompactBlockData>,
    pub(crate) legacy_heights: Vec<u64>,
    pub(crate) encoded_bytes: u64,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct RangeKey {
    endpoint: String,
    start: u64,
    end: u64,
}

static INFLIGHT_RANGES: Lazy<Mutex<HashMap<RangeKey, std::sync::Arc<InflightState>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static INITIALIZED_CACHE_PATHS: Lazy<Mutex<HashSet<PathBuf>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

pub enum InflightLease {
    Leader(InflightToken),
    Follower(InflightWaiter),
}

struct InflightState {
    completed: watch::Sender<bool>,
}

pub struct InflightWaiter {
    completed: watch::Receiver<bool>,
}

impl InflightWaiter {
    pub async fn wait(mut self) {
        while !*self.completed.borrow() {
            if self.completed.changed().await.is_err() {
                return;
            }
        }
    }
}

pub struct InflightToken {
    key: RangeKey,
    state: std::sync::Arc<InflightState>,
    completed: bool,
}

impl InflightToken {
    pub fn complete(mut self) {
        self.completed = true;
        finish_inflight(&self.key, &self.state);
    }
}

impl Drop for InflightToken {
    fn drop(&mut self) {
        if !self.completed {
            finish_inflight(&self.key, &self.state);
        }
    }
}

fn finish_inflight(key: &RangeKey, state: &std::sync::Arc<InflightState>) {
    let mut map = INFLIGHT_RANGES.lock();
    map.remove(key);
    state.completed.send_replace(true);
}

pub fn acquire_inflight(endpoint: &str, start: u64, end: u64) -> InflightLease {
    let key = RangeKey {
        endpoint: endpoint.to_string(),
        start,
        end,
    };
    let mut map = INFLIGHT_RANGES.lock();
    if let Some(existing) = find_overlap_locked(&map, endpoint, start, end) {
        return InflightLease::Follower(InflightWaiter {
            completed: existing.completed.subscribe(),
        });
    }
    let (completed, _receiver) = watch::channel(false);
    let state = std::sync::Arc::new(InflightState { completed });
    map.insert(key.clone(), state.clone());
    InflightLease::Leader(InflightToken {
        key,
        state,
        completed: false,
    })
}

fn find_overlap_locked(
    map: &HashMap<RangeKey, std::sync::Arc<InflightState>>,
    endpoint: &str,
    start: u64,
    end: u64,
) -> Option<std::sync::Arc<InflightState>> {
    for (key, state) in map.iter() {
        if key.endpoint == endpoint && start <= key.end && end >= key.start {
            return Some(state.clone());
        }
    }
    None
}

impl BlockCache {
    pub fn for_endpoint(endpoint: &str) -> Result<Self> {
        let path = cache_path_for_endpoint(endpoint)?;
        Self::new(path)
    }

    #[cfg(test)]
    fn load_range(&self, start: u64, end: u64) -> Result<Vec<CompactBlockData>> {
        self.load_range_for_upgrade(start, end)
            .map(|range| range.blocks)
    }

    pub(crate) fn load_range_for_upgrade(&self, start: u64, end: u64) -> Result<LoadedBlockRange> {
        if start > end {
            return Ok(LoadedBlockRange {
                blocks: Vec::new(),
                legacy_heights: Vec::new(),
                encoded_bytes: 0,
            });
        }

        let conn = self.open_conn()?;
        let mut stmt = conn.prepare(
            "SELECT height, data FROM blocks WHERE height BETWEEN ?1 AND ?2 ORDER BY height ASC",
        ).map_err(|e| Error::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![start as i64, end as i64], |row| {
                let height: i64 = row.get(0)?;
                let data: Vec<u8> = row.get(1)?;
                Ok((height, data))
            })
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut blocks = Vec::new();
        let mut legacy_heights = Vec::new();
        let mut encoded_bytes = 0u64;
        for row in rows {
            let (height, data) = row.map_err(|e| Error::Storage(e.to_string()))?;
            let height = u64::try_from(height)
                .map_err(|_| Error::Storage("Negative block-cache height".to_string()))?;
            if !data.starts_with(CACHE_RECORD_MAGIC) {
                legacy_heights.push(height);
            }
            encoded_bytes = encoded_bytes.saturating_add(data.len() as u64);
            blocks.push(decode_block(&data)?);
        }

        Ok(LoadedBlockRange {
            blocks,
            legacy_heights,
            encoded_bytes,
        })
    }

    /// Load the largest contiguous prefix whose encoded cache rows fit the byte
    /// budget. A single oversized block is returned by itself.
    pub(crate) fn load_bounded_range_for_upgrade(
        &self,
        start: u64,
        end: u64,
        max_encoded_bytes: u64,
    ) -> Result<LoadedBlockRange> {
        if start > end {
            return Ok(LoadedBlockRange {
                blocks: Vec::new(),
                legacy_heights: Vec::new(),
                encoded_bytes: 0,
            });
        }

        let conn = self.open_conn()?;
        let end = match Self::coverage_end(&conn, start)? {
            Some(covered_end) => end.min(covered_end),
            None if Self::has_recorded_coverage(&conn)? => {
                return Ok(LoadedBlockRange {
                    blocks: Vec::new(),
                    legacy_heights: Vec::new(),
                    encoded_bytes: 0,
                });
            }
            None => end,
        };
        let mut stmt = conn
            .prepare(
                "SELECT height, data FROM blocks WHERE height BETWEEN ?1 AND ?2 ORDER BY height ASC",
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut rows = stmt
            .query(params![start as i64, end as i64])
            .map_err(|e| Error::Storage(e.to_string()))?;
        let max_encoded_bytes = max_encoded_bytes.max(1);
        let mut expected = start;
        let mut blocks = Vec::new();
        let mut legacy_heights = Vec::new();
        let mut encoded_bytes = 0u64;

        while let Some(row) = rows.next().map_err(|e| Error::Storage(e.to_string()))? {
            let height_i64: i64 = row.get(0).map_err(|e| Error::Storage(e.to_string()))?;
            let height = u64::try_from(height_i64)
                .map_err(|_| Error::Storage("Negative block-cache height".to_string()))?;
            if height != expected {
                break;
            }
            let data: Vec<u8> = row.get(1).map_err(|e| Error::Storage(e.to_string()))?;
            let row_bytes = data.len() as u64;
            if !blocks.is_empty() && encoded_bytes.saturating_add(row_bytes) > max_encoded_bytes {
                break;
            }
            if !data.starts_with(CACHE_RECORD_MAGIC) {
                legacy_heights.push(height);
            }
            blocks.push(decode_block(&data)?);
            encoded_bytes = encoded_bytes.saturating_add(row_bytes);
            expected = expected.saturating_add(1);
        }

        Ok(LoadedBlockRange {
            blocks,
            legacy_heights,
            encoded_bytes,
        })
    }

    pub(crate) fn contiguous_end(&self, start: u64, end: u64) -> Result<Option<u64>> {
        if start > end {
            return Ok(None);
        }

        let conn = self.open_conn()?;
        if let Some(covered_end) = Self::coverage_end(&conn, start)? {
            return Ok(Some(end.min(covered_end)));
        }

        // Once coverage metadata exists, rows outside it are leftovers from an
        // interrupted replacement or an older cache generation. They must be
        // fetched again rather than joined to a validated chain interval.
        if Self::has_recorded_coverage(&conn)? {
            return Ok(None);
        }

        // Caches created before coverage metadata existed pay this ordered scan
        // once. The discovered interval is then retained for indexed lookups.
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut stmt = tx
            .prepare("SELECT height FROM blocks WHERE height BETWEEN ?1 AND ?2 ORDER BY height ASC")
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![start as i64, end as i64], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut expected = start;

        for row in rows {
            let height = u64::try_from(row.map_err(|e| Error::Storage(e.to_string()))?)
                .map_err(|_| Error::Storage("Negative block-cache height".to_string()))?;
            if height != expected {
                break;
            }
            if height == end {
                expected = end.saturating_add(1);
                break;
            }
            expected = expected.saturating_add(1);
        }

        let contiguous_end = (expected > start).then_some(expected - 1);
        drop(stmt);
        if let Some(contiguous_end) = contiguous_end {
            Self::record_coverage_tx(&tx, start, contiguous_end)?;
        }
        tx.commit().map_err(|e| Error::Storage(e.to_string()))?;

        Ok(contiguous_end)
    }

    pub fn store_blocks(&self, blocks: &[CompactBlockData]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let conn = self.open_conn()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Storage(e.to_string()))?;
        {
            let mut stmt = tx
                .prepare("INSERT OR REPLACE INTO blocks (height, data) VALUES (?1, ?2)")
                .map_err(|e| Error::Storage(e.to_string()))?;

            for block in blocks {
                let encoded = encode_block(block)?;
                stmt.execute(params![block.height as i64, encoded])
                    .map_err(|e| Error::Storage(e.to_string()))?;
            }
        }
        let mut heights = blocks.iter().map(|block| block.height).collect::<Vec<_>>();
        heights.sort_unstable();
        heights.dedup();
        if let Some(mut range_start) = heights.first().copied() {
            let mut range_end = range_start;
            for height in heights.into_iter().skip(1) {
                if height == range_end.saturating_add(1) {
                    range_end = height;
                } else {
                    Self::record_coverage_tx(&tx, range_start, range_end)?;
                    range_start = height;
                    range_end = height;
                }
            }
            Self::record_coverage_tx(&tx, range_start, range_end)?;
        }
        tx.commit().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    /// Rewrite canonical legacy JSON rows to the compact protobuf codec.
    ///
    /// The caller supplies only rows whose decoded block range has already been
    /// validated against the server. The conditional update avoids replacing a
    /// row that another wallet process upgraded concurrently.
    pub(crate) fn upgrade_legacy_rows(
        &self,
        blocks: &[CompactBlockData],
        legacy_heights: &[u64],
    ) -> Result<usize> {
        if legacy_heights.is_empty() {
            return Ok(0);
        }

        let legacy_heights: HashSet<u64> = legacy_heights.iter().copied().collect();
        let conn = self.open_conn()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut upgraded = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE blocks
                     SET data = ?1
                     WHERE height = ?2 AND substr(data, 1, 5) != ?3",
                )
                .map_err(|e| Error::Storage(e.to_string()))?;
            for block in blocks {
                if !legacy_heights.contains(&block.height) {
                    continue;
                }
                let encoded = encode_block(block)?;
                upgraded = upgraded.saturating_add(
                    stmt.execute(params![encoded, block.height as i64, CACHE_RECORD_MAGIC])
                        .map_err(|e| Error::Storage(e.to_string()))?,
                );
            }
        }
        tx.commit().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(upgraded)
    }

    pub fn delete_range(&self, start: u64, end: u64) -> Result<usize> {
        if start > end {
            return Ok(0);
        }

        let conn = self.open_conn()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let deleted = tx
            .execute(
                "DELETE FROM blocks WHERE height BETWEEN ?1 AND ?2",
                params![start as i64, end as i64],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        Self::remove_coverage_tx(&tx, start, end)?;
        tx.commit().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(deleted)
    }

    pub fn delete_above(&self, height: u64) -> Result<usize> {
        let conn = self.open_conn()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Storage(e.to_string()))?;
        let deleted = tx
            .execute(
                "DELETE FROM blocks WHERE height > ?1",
                params![height as i64],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        tx.execute(
            "DELETE FROM block_coverage WHERE range_start > ?1",
            params![height as i64],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        tx.execute(
            "UPDATE block_coverage SET range_end = ?1
             WHERE range_start <= ?1 AND range_end > ?1",
            params![height as i64],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        tx.commit().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(deleted)
    }

    fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Storage(e.to_string()))?;
        }
        let cache = Self { path };
        let mut initialized = INITIALIZED_CACHE_PATHS.lock();
        if initialized.contains(&cache.path) && cache.path.exists() {
            return Ok(cache);
        }
        let conn = cache.open_conn()?;
        // This database is rebuildable; WAL avoids reader/writer contention and
        // NORMAL avoids full-durability fsyncs on disposable cache writes.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| Error::Storage(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS blocks (
                height INTEGER PRIMARY KEY,
                data BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS block_coverage (
                range_start INTEGER PRIMARY KEY,
                range_end INTEGER NOT NULL CHECK(range_end >= range_start)
             );
             DROP INDEX IF EXISTS idx_blocks_height;",
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        initialized.insert(cache.path.clone());
        Ok(cache)
    }

    fn open_conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path).map_err(|e| Error::Storage(e.to_string()))?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| Error::Storage(e.to_string()))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| Error::Storage(e.to_string()))?;
        conn.pragma_update(None, "temp_store", "MEMORY")
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(conn)
    }

    fn coverage_end(conn: &Connection, start: u64) -> Result<Option<u64>> {
        let end: Option<i64> = conn
            .query_row(
                "SELECT range_end
                 FROM block_coverage
                 WHERE range_start <= ?1 AND range_end >= ?1
                 ORDER BY range_start DESC
                 LIMIT 1",
                params![start as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::Storage(e.to_string()))?;
        end.map(|height| {
            u64::try_from(height)
                .map_err(|_| Error::Storage("Negative block-cache coverage height".to_string()))
        })
        .transpose()
    }

    fn has_recorded_coverage(conn: &Connection) -> Result<bool> {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM block_coverage LIMIT 1)",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| Error::Storage(e.to_string()))
    }

    fn record_coverage_tx(tx: &Transaction<'_>, start: u64, end: u64) -> Result<()> {
        if start > end {
            return Ok(());
        }
        let adjacent_start = start.saturating_sub(1) as i64;
        let adjacent_end = end.saturating_add(1) as i64;
        let overlap: (Option<i64>, Option<i64>) = tx
            .query_row(
                "SELECT MIN(range_start), MAX(range_end)
                 FROM block_coverage
                 WHERE range_end >= ?1 AND range_start <= ?2",
                params![adjacent_start, adjacent_end],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        let merged_start = overlap.0.map_or(start, |value| start.min(value as u64));
        let merged_end = overlap.1.map_or(end, |value| end.max(value as u64));
        tx.execute(
            "DELETE FROM block_coverage WHERE range_end >= ?1 AND range_start <= ?2",
            params![adjacent_start, adjacent_end],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        tx.execute(
            "INSERT INTO block_coverage (range_start, range_end) VALUES (?1, ?2)",
            params![merged_start as i64, merged_end as i64],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    fn remove_coverage_tx(tx: &Transaction<'_>, start: u64, end: u64) -> Result<()> {
        let mut stmt = tx
            .prepare(
                "SELECT range_start, range_end
                 FROM block_coverage
                 WHERE range_start <= ?2 AND range_end >= ?1",
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        let ranges = stmt
            .query_map(params![start as i64, end as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| Error::Storage(e.to_string()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Storage(e.to_string()))?;
        drop(stmt);

        for (range_start, range_end) in ranges {
            tx.execute(
                "DELETE FROM block_coverage WHERE range_start = ?1",
                params![range_start],
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
            let range_start = u64::try_from(range_start)
                .map_err(|_| Error::Storage("Negative block-cache coverage height".to_string()))?;
            let range_end = u64::try_from(range_end)
                .map_err(|_| Error::Storage("Negative block-cache coverage height".to_string()))?;
            if range_start < start {
                Self::record_coverage_tx(tx, range_start, start.saturating_sub(1))?;
            }
            if range_end > end {
                Self::record_coverage_tx(tx, end.saturating_add(1), range_end)?;
            }
        }
        Ok(())
    }
}

fn cache_base_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("PIRATE_BLOCK_CACHE_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    if let Ok(dir) = std::env::var("PIRATE_WALLET_DB_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    if let Ok(path) = std::env::var("PIRATE_WALLET_DB_PATH") {
        if path.contains("{wallet_id}") {
            let parent = Path::new(&path).parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }

        let parsed = PathBuf::from(&path);
        if parsed.extension().is_some() {
            let parent = parsed.parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }
        return Ok(parsed);
    }

    let base = ProjectDirs::from("com", "Pirate", "PirateWallet")
        .map(|dirs| dirs.data_local_dir().join("cache"))
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base)
}

fn cache_path_for_endpoint(endpoint: &str) -> Result<PathBuf> {
    let base = cache_base_dir()?;
    let hash = Sha256::digest(endpoint.as_bytes());
    let short = hex::encode(&hash[..8]);
    Ok(base.join(format!("block_cache_{}.db", short)))
}

fn encode_block(block: &CompactBlockData) -> Result<Vec<u8>> {
    let cached = CachedCompactBlock::from(block);
    let mut encoded = Vec::with_capacity(CACHE_RECORD_MAGIC.len() + cached.encoded_len());
    encoded.extend_from_slice(CACHE_RECORD_MAGIC);
    cached
        .encode(&mut encoded)
        .map_err(|e| Error::Storage(e.to_string()))?;
    Ok(encoded)
}

fn decode_block(bytes: &[u8]) -> Result<CompactBlockData> {
    if let Some(payload) = bytes.strip_prefix(CACHE_RECORD_MAGIC) {
        return CachedCompactBlock::decode(payload)
            .map(CompactBlockData::from)
            .map_err(|e| Error::Storage(e.to_string()));
    }

    // Cache files created before the binary codec remain readable. Newly fetched
    // ranges overwrite these rows in the compact representation.
    serde_json::from_slice(bytes).map_err(|e| Error::Storage(e.to_string()))
}

#[derive(Clone, PartialEq, Message)]
struct CachedCompactBlock {
    #[prost(uint32, tag = "1")]
    proto_version: u32,
    #[prost(uint64, tag = "2")]
    height: u64,
    #[prost(bytes = "vec", tag = "3")]
    hash: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    prev_hash: Vec<u8>,
    #[prost(uint32, tag = "5")]
    time: u32,
    #[prost(bytes = "vec", tag = "6")]
    header: Vec<u8>,
    #[prost(message, repeated, tag = "7")]
    transactions: Vec<CachedCompactTx>,
}

#[derive(Clone, PartialEq, Message)]
struct CachedCompactTx {
    #[prost(uint64, optional, tag = "1")]
    index: Option<u64>,
    #[prost(bytes = "vec", tag = "2")]
    hash: Vec<u8>,
    #[prost(uint32, optional, tag = "3")]
    fee: Option<u32>,
    #[prost(message, repeated, tag = "4")]
    spends: Vec<CachedSaplingSpend>,
    #[prost(message, repeated, tag = "5")]
    outputs: Vec<CachedSaplingOutput>,
    #[prost(message, repeated, tag = "6")]
    actions: Vec<CachedIronwoodAction>,
}

#[derive(Clone, PartialEq, Message)]
struct CachedSaplingSpend {
    #[prost(bytes = "vec", tag = "1")]
    nf: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct CachedSaplingOutput {
    #[prost(bytes = "vec", tag = "1")]
    cmu: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    ephemeral_key: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    ciphertext: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct CachedIronwoodAction {
    #[prost(bytes = "vec", tag = "1")]
    nullifier: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    cmx: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    ephemeral_key: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    enc_ciphertext: Vec<u8>,
    #[prost(bytes = "vec", tag = "5")]
    out_ciphertext: Vec<u8>,
}

impl From<&CompactBlockData> for CachedCompactBlock {
    fn from(block: &CompactBlockData) -> Self {
        Self {
            proto_version: block.proto_version,
            height: block.height,
            hash: block.hash.clone(),
            prev_hash: block.prev_hash.clone(),
            time: block.time,
            header: block.header.clone(),
            transactions: block
                .transactions
                .iter()
                .map(|tx| CachedCompactTx {
                    index: tx.index,
                    hash: tx.hash.clone(),
                    fee: tx.fee,
                    spends: tx
                        .spends
                        .iter()
                        .map(|spend| CachedSaplingSpend {
                            nf: spend.nf.clone(),
                        })
                        .collect(),
                    outputs: tx
                        .outputs
                        .iter()
                        .map(|output| CachedSaplingOutput {
                            cmu: output.cmu.clone(),
                            ephemeral_key: output.ephemeral_key.clone(),
                            ciphertext: output.ciphertext.clone(),
                        })
                        .collect(),
                    actions: tx
                        .actions
                        .iter()
                        .map(|action| CachedIronwoodAction {
                            nullifier: action.nullifier.clone(),
                            cmx: action.cmx.clone(),
                            ephemeral_key: action.ephemeral_key.clone(),
                            enc_ciphertext: action.enc_ciphertext.clone(),
                            out_ciphertext: action.out_ciphertext.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<CachedCompactBlock> for CompactBlockData {
    fn from(block: CachedCompactBlock) -> Self {
        Self {
            proto_version: block.proto_version,
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            time: block.time,
            header: block.header,
            transactions: block
                .transactions
                .into_iter()
                .map(|tx| crate::client::CompactTx {
                    index: tx.index,
                    hash: tx.hash,
                    fee: tx.fee,
                    spends: tx
                        .spends
                        .into_iter()
                        .map(|spend| crate::client::CompactSaplingSpend { nf: spend.nf })
                        .collect(),
                    outputs: tx
                        .outputs
                        .into_iter()
                        .map(|output| crate::client::CompactSaplingOutput {
                            cmu: output.cmu,
                            ephemeral_key: output.ephemeral_key,
                            ciphertext: output.ciphertext,
                        })
                        .collect(),
                    actions: tx
                        .actions
                        .into_iter()
                        .map(|action| crate::client::CompactIronwoodAction {
                            nullifier: action.nullifier,
                            cmx: action.cmx,
                            ephemeral_key: action.ephemeral_key,
                            enc_ciphertext: action.enc_ciphertext,
                            out_ciphertext: action.out_ciphertext,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{
        CompactIronwoodAction, CompactSaplingOutput, CompactSaplingSpend, CompactTx,
    };

    fn sample_block() -> CompactBlockData {
        CompactBlockData {
            proto_version: 2,
            height: 42,
            hash: vec![1; 32],
            prev_hash: vec![2; 32],
            time: 1_700_000_000,
            header: vec![3; 80],
            transactions: vec![CompactTx {
                index: None,
                hash: vec![4; 32],
                fee: None,
                spends: vec![CompactSaplingSpend { nf: vec![5; 32] }],
                outputs: vec![CompactSaplingOutput {
                    cmu: vec![6; 32],
                    ephemeral_key: vec![7; 32],
                    ciphertext: vec![8; 52],
                }],
                actions: vec![CompactIronwoodAction {
                    nullifier: vec![9; 32],
                    cmx: vec![10; 32],
                    ephemeral_key: vec![11; 32],
                    enc_ciphertext: vec![12; 52],
                    out_ciphertext: vec![13; 80],
                }],
            }],
        }
    }

    fn assert_same_block(expected: &CompactBlockData, actual: &CompactBlockData) {
        assert_eq!(
            serde_json::to_value(expected).unwrap(),
            serde_json::to_value(actual).unwrap()
        );
    }

    #[test]
    fn binary_codec_round_trips_all_cached_fields() {
        let block = sample_block();
        let encoded = encode_block(&block).unwrap();
        let decoded = decode_block(&encoded).unwrap();

        assert!(encoded.starts_with(CACHE_RECORD_MAGIC));
        assert_same_block(&block, &decoded);
        assert!(encoded.len() < serde_json::to_vec(&block).unwrap().len());
    }

    #[test]
    #[ignore = "manual compact-block cache segment benchmark"]
    fn benchmark_cache_segment_sizes() {
        const BLOCK_COUNT: u64 = 65_536;
        const SEGMENT_SIZES: [usize; 6] = [128, 256, 512, 1_024, 2_048, 4_096];

        let blocks = (0..BLOCK_COUNT)
            .map(|offset| {
                let mut block = sample_block();
                block.height = 3_500_000 + offset;
                let hash = block.height.to_le_bytes().repeat(4);
                let prev_hash = block.height.saturating_sub(1).to_le_bytes().repeat(4);
                block.hash = hash;
                block.prev_hash = prev_hash;
                block
            })
            .collect::<Vec<_>>();

        let mut totals = [Duration::ZERO; SEGMENT_SIZES.len()];
        for run in 0..3 {
            let mut order = SEGMENT_SIZES;
            match run {
                1 => order.reverse(),
                2 => order.rotate_left(3),
                _ => {}
            }
            for segment_size in order {
                let dir = tempfile::tempdir().unwrap();
                let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
                let started = std::time::Instant::now();
                for segment in blocks.chunks(segment_size) {
                    cache.store_blocks(segment).unwrap();
                }
                let elapsed = started.elapsed();
                assert_eq!(
                    cache
                        .contiguous_end(blocks[0].height, blocks.last().unwrap().height)
                        .unwrap(),
                    Some(blocks.last().unwrap().height)
                );
                let index = SEGMENT_SIZES
                    .iter()
                    .position(|candidate| *candidate == segment_size)
                    .unwrap();
                totals[index] += elapsed;
            }
        }

        for (index, segment_size) in SEGMENT_SIZES.into_iter().enumerate() {
            let elapsed = totals[index] / 3;
            println!(
                "cache segment benchmark: blocks={BLOCK_COUNT}, segment={segment_size:>4}, transactions={:>4}, average={:.3}s, throughput={:.0} blocks/s",
                blocks.len().div_ceil(segment_size),
                elapsed.as_secs_f64(),
                BLOCK_COUNT as f64 / elapsed.as_secs_f64()
            );
        }
    }

    #[test]
    fn legacy_json_cache_rows_remain_readable() {
        let block = sample_block();
        let legacy = serde_json::to_vec(&block).unwrap();
        let decoded = decode_block(&legacy).unwrap();

        assert_same_block(&block, &decoded);
    }

    #[test]
    fn canonical_legacy_rows_are_upgraded_transactionally() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let block = sample_block();
        let legacy = serde_json::to_vec(&block).unwrap();
        cache
            .open_conn()
            .unwrap()
            .execute(
                "INSERT INTO blocks (height, data) VALUES (?1, ?2)",
                params![block.height as i64, legacy],
            )
            .unwrap();

        let loaded = cache
            .load_range_for_upgrade(block.height, block.height)
            .unwrap();
        assert_eq!(loaded.legacy_heights, vec![block.height]);
        assert_same_block(&block, &loaded.blocks[0]);
        assert_eq!(
            cache
                .upgrade_legacy_rows(&loaded.blocks, &loaded.legacy_heights)
                .unwrap(),
            1
        );

        let encoded: Vec<u8> = cache
            .open_conn()
            .unwrap()
            .query_row(
                "SELECT data FROM blocks WHERE height = ?1",
                [block.height as i64],
                |row| row.get(0),
            )
            .unwrap();
        assert!(encoded.starts_with(CACHE_RECORD_MAGIC));
        assert_eq!(
            cache
                .upgrade_legacy_rows(&loaded.blocks, &loaded.legacy_heights)
                .unwrap(),
            0
        );
    }

    #[test]
    fn cache_persists_binary_rows_without_a_redundant_height_index() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let block = sample_block();

        cache.store_blocks(std::slice::from_ref(&block)).unwrap();
        let loaded = cache.load_range(block.height, block.height).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_same_block(&block, &loaded[0]);

        let conn = cache.open_conn().unwrap();
        let encoded: Vec<u8> = conn
            .query_row(
                "SELECT data FROM blocks WHERE height = ?1",
                [block.height as i64],
                |row| row.get(0),
            )
            .unwrap();
        let redundant_index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_blocks_height'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(encoded.starts_with(CACHE_RECORD_MAGIC));
        assert_eq!(redundant_index_count, 0);
    }

    #[test]
    fn contiguous_end_stops_before_the_first_cache_gap() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let mut blocks = Vec::new();
        for height in [42, 43, 44, 46] {
            let mut block = sample_block();
            block.height = height;
            blocks.push(block);
        }
        cache.store_blocks(&blocks).unwrap();

        assert_eq!(cache.contiguous_end(42, 50).unwrap(), Some(44));
        assert_eq!(cache.contiguous_end(45, 50).unwrap(), None);
        assert_eq!(cache.contiguous_end(46, 50).unwrap(), Some(46));
    }

    #[test]
    fn bounded_cache_reads_stop_on_encoded_bytes_and_resume_contiguously() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let blocks = (42..=46)
            .map(|height| {
                let mut block = sample_block();
                block.height = height;
                block.hash = vec![height as u8; 32];
                block
            })
            .collect::<Vec<_>>();
        cache.store_blocks(&blocks).unwrap();
        let one_row_bytes = encode_block(&blocks[0]).unwrap().len() as u64;

        let first = cache
            .load_bounded_range_for_upgrade(42, 46, one_row_bytes * 2)
            .unwrap();
        assert_eq!(
            first
                .blocks
                .iter()
                .map(|block| block.height)
                .collect::<Vec<_>>(),
            vec![42, 43]
        );
        assert!(first.encoded_bytes <= one_row_bytes * 2);

        let second = cache
            .load_bounded_range_for_upgrade(44, 46, one_row_bytes * 2)
            .unwrap();
        assert_eq!(
            second
                .blocks
                .iter()
                .map(|block| block.height)
                .collect::<Vec<_>>(),
            vec![44, 45]
        );
    }

    #[test]
    fn bounded_cache_reads_allow_one_oversized_record() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let block = sample_block();
        cache.store_blocks(std::slice::from_ref(&block)).unwrap();

        let loaded = cache
            .load_bounded_range_for_upgrade(block.height, block.height, 1)
            .unwrap();
        assert_eq!(loaded.blocks.len(), 1);
        assert!(loaded.encoded_bytes > 1);
    }

    #[test]
    fn coverage_metadata_excludes_untracked_legacy_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let tracked = (42..=44)
            .map(|height| {
                let mut block = sample_block();
                block.height = height;
                block
            })
            .collect::<Vec<_>>();
        cache.store_blocks(&tracked).unwrap();

        let conn = cache.open_conn().unwrap();
        for height in 45..=47 {
            let mut stale = sample_block();
            stale.height = height;
            conn.execute(
                "INSERT INTO blocks (height, data) VALUES (?1, ?2)",
                params![height as i64, serde_json::to_vec(&stale).unwrap()],
            )
            .unwrap();
        }
        drop(conn);

        let tracked_prefix = cache
            .load_bounded_range_for_upgrade(42, 47, u64::MAX)
            .unwrap();
        assert_eq!(
            tracked_prefix
                .blocks
                .iter()
                .map(|block| block.height)
                .collect::<Vec<_>>(),
            vec![42, 43, 44]
        );
        assert!(cache
            .load_bounded_range_for_upgrade(45, 47, u64::MAX)
            .unwrap()
            .blocks
            .is_empty());
        assert_eq!(cache.contiguous_end(45, 47).unwrap(), None);
    }

    #[test]
    fn contiguous_end_memoizes_coverage_for_legacy_caches() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let conn = cache.open_conn().unwrap();
        for height in 42..=46 {
            conn.execute(
                "INSERT INTO blocks (height, data) VALUES (?1, ?2)",
                params![height, vec![0u8]],
            )
            .unwrap();
        }
        drop(conn);

        assert_eq!(cache.contiguous_end(42, 46).unwrap(), Some(46));
        let coverage: (i64, i64) = cache
            .open_conn()
            .unwrap()
            .query_row(
                "SELECT range_start, range_end FROM block_coverage",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(coverage, (42, 46));
    }

    #[test]
    fn deleting_cached_blocks_splits_coverage() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::new(dir.path().join("blocks.db")).unwrap();
        let blocks = (42..=50)
            .map(|height| {
                let mut block = sample_block();
                block.height = height;
                block
            })
            .collect::<Vec<_>>();
        cache.store_blocks(&blocks).unwrap();

        assert_eq!(cache.delete_range(45, 47).unwrap(), 3);
        assert_eq!(cache.contiguous_end(42, 50).unwrap(), Some(44));
        assert_eq!(cache.contiguous_end(45, 50).unwrap(), None);
        assert_eq!(cache.contiguous_end(48, 50).unwrap(), Some(50));

        assert_eq!(cache.delete_above(49).unwrap(), 1);
        assert_eq!(cache.contiguous_end(48, 50).unwrap(), Some(49));
    }

    #[tokio::test]
    async fn follower_observes_completion_that_happened_before_waiting() {
        let endpoint = "test://lost-completion";
        let leader = match acquire_inflight(endpoint, 100, 200) {
            InflightLease::Leader(token) => token,
            InflightLease::Follower(_) => panic!("first lease must lead"),
        };
        let follower = match acquire_inflight(endpoint, 150, 250) {
            InflightLease::Follower(waiter) => waiter,
            InflightLease::Leader(_) => panic!("overlapping lease must follow"),
        };

        leader.complete();

        tokio::time::timeout(Duration::from_millis(100), follower.wait())
            .await
            .expect("completion must not be lost");
    }
}
