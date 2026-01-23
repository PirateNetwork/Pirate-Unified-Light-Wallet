use bridgetree::{BridgeTree, Checkpoint, MerkleBridge};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Cursor, Read, Write};
use zcash_primitives::merkle_tree::{
    read_address, read_nonempty_frontier_v1, read_position, write_address,
    write_nonempty_frontier_v1, write_position, HashSer,
};

use crate::{Error, Result};

const BRIDGE_TREE_VERSION: u8 = 1;

fn write_u8<W: Write>(mut writer: W, value: u8) -> Result<()> {
    writer
        .write_all(&[value])
        .map_err(|e| Error::Sync(format!("Failed to write u8: {}", e)))
}

fn write_u32<W: Write>(mut writer: W, value: u32) -> Result<()> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|e| Error::Sync(format!("Failed to write u32: {}", e)))
}

fn write_u64<W: Write>(mut writer: W, value: u64) -> Result<()> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|e| Error::Sync(format!("Failed to write u64: {}", e)))
}

fn read_u8<R: Read>(mut reader: R) -> Result<u8> {
    let mut buf = [0u8; 1];
    reader
        .read_exact(&mut buf)
        .map_err(|e| Error::Sync(format!("Failed to read u8: {}", e)))?;
    Ok(buf[0])
}

fn read_u32<R: Read>(mut reader: R) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .map_err(|e| Error::Sync(format!("Failed to read u32: {}", e)))?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64<R: Read>(mut reader: R) -> Result<u64> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|e| Error::Sync(format!("Failed to read u64: {}", e)))?;
    Ok(u64::from_le_bytes(buf))
}

fn write_bridge<H, W>(mut writer: W, bridge: &MerkleBridge<H>) -> Result<()>
where
    H: HashSer + Clone,
    W: Write,
{
    match bridge.prior_position() {
        Some(pos) => {
            write_u8(&mut writer, 1)?;
            write_position(&mut writer, pos)
                .map_err(|e| Error::Sync(format!("Failed to write bridge position: {}", e)))?;
        }
        None => write_u8(&mut writer, 0)?,
    }

    let tracking = bridge.tracking();
    write_u32(&mut writer, tracking.len() as u32)?;
    for addr in tracking {
        write_address(&mut writer, *addr)
            .map_err(|e| Error::Sync(format!("Failed to write bridge tracking address: {}", e)))?;
    }

    let ommers = bridge.ommers();
    write_u32(&mut writer, ommers.len() as u32)?;
    for (addr, node) in ommers {
        write_address(&mut writer, *addr)
            .map_err(|e| Error::Sync(format!("Failed to write bridge ommer address: {}", e)))?;
        node.write(&mut writer)
            .map_err(|e| Error::Sync(format!("Failed to write bridge ommer: {}", e)))?;
    }

    write_nonempty_frontier_v1(&mut writer, bridge.frontier())
        .map_err(|e| Error::Sync(format!("Failed to write bridge frontier: {}", e)))
}

fn read_bridge<H, R>(mut reader: R) -> Result<MerkleBridge<H>>
where
    H: HashSer + Clone + Ord,
    R: Read,
{
    let has_prior = read_u8(&mut reader)?;
    let prior_position = if has_prior == 1 {
        Some(
            read_position(&mut reader)
                .map_err(|e| Error::Sync(format!("Failed to read bridge prior position: {}", e)))?,
        )
    } else {
        None
    };

    let tracking_len = read_u32(&mut reader)? as usize;
    let mut tracking = BTreeSet::new();
    for _ in 0..tracking_len {
        let addr = read_address(&mut reader)
            .map_err(|e| Error::Sync(format!("Failed to read bridge tracking address: {}", e)))?;
        tracking.insert(addr);
    }

    let ommers_len = read_u32(&mut reader)? as usize;
    let mut ommers = BTreeMap::new();
    for _ in 0..ommers_len {
        let addr = read_address(&mut reader)
            .map_err(|e| Error::Sync(format!("Failed to read bridge ommer address: {}", e)))?;
        let node = H::read(&mut reader)
            .map_err(|e| Error::Sync(format!("Failed to read bridge ommer: {}", e)))?;
        ommers.insert(addr, node);
    }

    let frontier = read_nonempty_frontier_v1(&mut reader)
        .map_err(|e| Error::Sync(format!("Failed to read bridge frontier: {}", e)))?;

    Ok(MerkleBridge::from_parts(
        prior_position,
        tracking,
        ommers,
        frontier,
    ))
}

fn write_checkpoint<W: Write>(mut writer: W, checkpoint: &Checkpoint<u32>) -> Result<()> {
    write_u32(&mut writer, *checkpoint.id())?;
    write_u64(&mut writer, checkpoint.bridges_len() as u64)?;

    let marked = checkpoint.marked();
    write_u32(&mut writer, marked.len() as u32)?;
    for pos in marked {
        write_position(&mut writer, *pos).map_err(|e| {
            Error::Sync(format!("Failed to write checkpoint marked position: {}", e))
        })?;
    }

    let forgotten = checkpoint.forgotten();
    write_u32(&mut writer, forgotten.len() as u32)?;
    for pos in forgotten {
        write_position(&mut writer, *pos).map_err(|e| {
            Error::Sync(format!(
                "Failed to write checkpoint forgotten position: {}",
                e
            ))
        })?;
    }

    Ok(())
}

fn read_checkpoint<R: Read>(mut reader: R) -> Result<Checkpoint<u32>> {
    let id = read_u32(&mut reader)?;
    let bridges_len = read_u64(&mut reader)?;
    let bridges_len = usize::try_from(bridges_len)
        .map_err(|_| Error::Sync("Checkpoint bridges_len exceeds usize".to_string()))?;

    let marked_len = read_u32(&mut reader)? as usize;
    let mut marked = BTreeSet::new();
    for _ in 0..marked_len {
        let pos = read_position(&mut reader).map_err(|e| {
            Error::Sync(format!("Failed to read checkpoint marked position: {}", e))
        })?;
        marked.insert(pos);
    }

    let forgotten_len = read_u32(&mut reader)? as usize;
    let mut forgotten = BTreeSet::new();
    for _ in 0..forgotten_len {
        let pos = read_position(&mut reader).map_err(|e| {
            Error::Sync(format!(
                "Failed to read checkpoint forgotten position: {}",
                e
            ))
        })?;
        forgotten.insert(pos);
    }

    Ok(Checkpoint::from_parts(id, bridges_len, marked, forgotten))
}

pub fn serialize_bridge_tree<H, const DEPTH: u8>(
    tree: &BridgeTree<H, u32, DEPTH>,
) -> Result<Vec<u8>>
where
    H: bridgetree::Hashable + HashSer + Clone + Ord,
{
    let mut buf = Vec::new();
    write_u8(&mut buf, BRIDGE_TREE_VERSION)?;
    write_u32(&mut buf, tree.max_checkpoints() as u32)?;

    let prior_bridges = tree.prior_bridges();
    write_u32(&mut buf, prior_bridges.len() as u32)?;
    for bridge in prior_bridges {
        write_bridge(&mut buf, bridge)?;
    }

    match tree.current_bridge() {
        Some(bridge) => {
            write_u8(&mut buf, 1)?;
            write_bridge(&mut buf, bridge)?;
        }
        None => write_u8(&mut buf, 0)?,
    }

    let saved = tree.marked_indices();
    write_u32(&mut buf, saved.len() as u32)?;
    for (pos, idx) in saved {
        write_position(&mut buf, *pos)
            .map_err(|e| Error::Sync(format!("Failed to write saved position: {}", e)))?;
        write_u64(&mut buf, *idx as u64)?;
    }

    let checkpoints = tree.checkpoints();
    write_u32(&mut buf, checkpoints.len() as u32)?;
    for checkpoint in checkpoints {
        write_checkpoint(&mut buf, checkpoint)?;
    }

    Ok(buf)
}

pub fn deserialize_bridge_tree<H, const DEPTH: u8>(
    bytes: &[u8],
) -> Result<BridgeTree<H, u32, DEPTH>>
where
    H: bridgetree::Hashable + HashSer + Clone + Ord,
{
    let mut cursor = Cursor::new(bytes);
    let version = read_u8(&mut cursor)?;
    if version != BRIDGE_TREE_VERSION {
        return Err(Error::Sync(format!(
            "Unsupported BridgeTree version: {}",
            version
        )));
    }

    let max_checkpoints = read_u32(&mut cursor)? as usize;

    let prior_len = read_u32(&mut cursor)? as usize;
    let mut prior_bridges = Vec::with_capacity(prior_len);
    for _ in 0..prior_len {
        prior_bridges.push(read_bridge(&mut cursor)?);
    }

    let has_current = read_u8(&mut cursor)?;
    let current_bridge = if has_current == 1 {
        Some(read_bridge(&mut cursor)?)
    } else {
        None
    };

    let saved_len = read_u32(&mut cursor)? as usize;
    let mut saved = BTreeMap::new();
    for _ in 0..saved_len {
        let pos = read_position(&mut cursor)
            .map_err(|e| Error::Sync(format!("Failed to read saved position: {}", e)))?;
        let idx = read_u64(&mut cursor)?;
        let idx = usize::try_from(idx)
            .map_err(|_| Error::Sync("Saved index exceeds usize".to_string()))?;
        saved.insert(pos, idx);
    }

    let checkpoint_len = read_u32(&mut cursor)? as usize;
    let mut checkpoints = VecDeque::with_capacity(checkpoint_len);
    for _ in 0..checkpoint_len {
        checkpoints.push_back(read_checkpoint(&mut cursor)?);
    }

    BridgeTree::from_parts(
        prior_bridges,
        current_bridge,
        saved,
        checkpoints,
        max_checkpoints,
    )
    .map_err(|e| Error::Sync(format!("Failed to restore BridgeTree: {:?}", e)))
}
