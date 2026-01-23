//! Shared compact block cache for multi-wallet sync.

use crate::client::CompactBlockData;
use crate::{Error, Result};
use directories::ProjectDirs;
use once_cell::sync::Lazy;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::{Mutex, Notify};

pub struct BlockCache {
    path: PathBuf,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct RangeKey {
    endpoint: String,
    start: u64,
    end: u64,
}

static INFLIGHT_RANGES: Lazy<Mutex<HashMap<RangeKey, std::sync::Arc<Notify>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub enum InflightLease {
    Leader(InflightToken),
    Follower(std::sync::Arc<Notify>),
}

pub struct InflightToken {
    key: RangeKey,
    notify: std::sync::Arc<Notify>,
}

impl InflightToken {
    pub async fn complete(self) {
        let mut map = INFLIGHT_RANGES.lock().await;
        map.remove(&self.key);
        self.notify.notify_waiters();
    }
}

pub async fn acquire_inflight(endpoint: &str, start: u64, end: u64) -> InflightLease {
    let key = RangeKey {
        endpoint: endpoint.to_string(),
        start,
        end,
    };
    let mut map = INFLIGHT_RANGES.lock().await;
    if let Some(existing) = find_overlap_locked(&map, endpoint, start, end) {
        return InflightLease::Follower(existing);
    }
    let notify = std::sync::Arc::new(Notify::new());
    map.insert(key.clone(), notify.clone());
    InflightLease::Leader(InflightToken { key, notify })
}

fn find_overlap_locked(
    map: &HashMap<RangeKey, std::sync::Arc<Notify>>,
    endpoint: &str,
    start: u64,
    end: u64,
) -> Option<std::sync::Arc<Notify>> {
    for (key, notify) in map.iter() {
        if key.endpoint == endpoint && start <= key.end && end >= key.start {
            return Some(notify.clone());
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
