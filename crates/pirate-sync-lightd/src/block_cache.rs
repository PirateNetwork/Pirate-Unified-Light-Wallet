//! Shared compact block cache for multi-wallet sync.

use crate::client::CompactBlockData;
use crate::{Error, Result};
use directories::ProjectDirs;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::Notify;

pub struct BlockCache {
    path: PathBuf,
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

    pub fn load_range(&self, start: u64, end: u64) -> Result<Vec<CompactBlockData>> {
        if start > end {
            return Ok(Vec::new());
        }

        let conn = self.open_conn()?;
        let mut stmt = conn.prepare(
            "SELECT height, data FROM blocks WHERE height BETWEEN ?1 AND ?2 ORDER BY height ASC",
        ).map_err(|e| Error::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![start as i64, end as i64], |row| {
                let data: Vec<u8> = row.get(1)?;
                Ok(data)
            })
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut blocks = Vec::new();
        for row in rows {
            let data = row.map_err(|e| Error::Storage(e.to_string()))?;
            blocks.push(decode_block(&data)?);
        }

        Ok(blocks)
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
        tx.commit().map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn delete_range(&self, start: u64, end: u64) -> Result<usize> {
        if start > end {
            return Ok(0);
        }

        let conn = self.open_conn()?;
        conn.execute(
            "DELETE FROM blocks WHERE height BETWEEN ?1 AND ?2",
            params![start as i64, end as i64],
        )
        .map_err(|e| Error::Storage(e.to_string()))
    }

    pub fn delete_above(&self, height: u64) -> Result<usize> {
        let conn = self.open_conn()?;
        conn.execute(
            "DELETE FROM blocks WHERE height > ?1",
            params![height as i64],
        )
        .map_err(|e| Error::Storage(e.to_string()))
    }

    fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Storage(e.to_string()))?;
        }
        let cache = Self { path };
        let conn = cache.open_conn()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS blocks (
                height INTEGER PRIMARY KEY,
                data BLOB NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_blocks_height ON blocks(height);",
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(cache)
    }

    fn open_conn(&self) -> Result<Connection> {
        Connection::open(&self.path).map_err(|e| Error::Storage(e.to_string()))
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
    // Use serde for serialization to avoid prost version conflicts
    serde_json::to_vec(block).map_err(|e| Error::Storage(e.to_string()))
}

fn decode_block(bytes: &[u8]) -> Result<CompactBlockData> {
    serde_json::from_slice(bytes).map_err(|e| Error::Storage(e.to_string()))
}
