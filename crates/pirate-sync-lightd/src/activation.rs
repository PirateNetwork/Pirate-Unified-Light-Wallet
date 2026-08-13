//! Timestamp-based Ironwood activation-height resolution.

use crate::client::CompactBlockData;
use crate::{Error, LightClient, Result};
use pirate_core::transaction::PirateNetwork;
use pirate_params::{Network, NetworkType};
use std::time::{SystemTime, UNIX_EPOCH};
use zcash_protocol::consensus::{BlockHeight, BranchId};

const FULL_NODE_LOOKBACK_BLOCKS: u32 = 30;
const FULL_NODE_SEARCH_WINDOW_SECONDS: u32 = 24 * 60 * 60;
const SEARCH_CHUNK_BLOCKS: u32 = 2_048;

/// Resolve the active Ironwood height for a network and server tip.
///
/// Fixed-height networks return their configured height. Mainnet retains a
/// previously resolved height when it agrees with the server branch, otherwise
/// it derives the height from canonical block timestamps.
pub async fn resolve_ironwood_activation_height(
    client: &LightClient,
    network_type: NetworkType,
    tip_height: u64,
    server_branch_id: &str,
    mut known_activation_height: Option<u32>,
) -> Result<Option<u32>> {
    let network = Network::from_type(network_type);
    if network.ironwood_activation_height.is_some() {
        return Ok(network.ironwood_activation_height);
    }
    let Some(timestamp) = network.ironwood_activation_timestamp else {
        return Ok(known_activation_height);
    };
    let tip_height = u32::try_from(tip_height)
        .map_err(|_| Error::Network(format!("Server height out of range: {tip_height}")))?;

    if let Some(known_height) = known_activation_height {
        if branch_matches(
            network_type,
            tip_height,
            server_branch_id,
            Some(known_height),
        ) && known_activation_is_canonical(client, tip_height, &network, known_height).await?
        {
            return Ok(Some(known_height));
        }
        known_activation_height = None;
    }

    let server_is_ironwood = parse_branch_id(server_branch_id) == Some(u32::from(BranchId::Nu6_3));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let probe_window = u64::from(timestamp.saturating_sub(FULL_NODE_SEARCH_WINDOW_SECONDS));
    if !server_is_ironwood && now < probe_window {
        return Ok(known_activation_height);
    }

    let resolved = discover_mainnet_activation_height(client, tip_height, &network).await?;
    if server_is_ironwood && resolved.is_none() {
        return Err(Error::Network(format!(
            "Server reports Ironwood at height {tip_height}, but the mainnet timestamp transition could not be resolved"
        )));
    }
    Ok(resolved)
}

async fn known_activation_is_canonical(
    client: &LightClient,
    tip_height: u32,
    network: &Network,
    activation_height: u32,
) -> Result<bool> {
    let Some(timestamp) = network.ironwood_activation_timestamp else {
        return Ok(network.ironwood_activation_height == Some(activation_height));
    };
    let Some(crossing_height) = activation_height.checked_sub(network.ironwood_activation_delay)
    else {
        return Ok(false);
    };

    // A lagging server cannot validate a transition it has not reached. The
    // opaque branch check above still proves it agrees with the pre-activation
    // schedule at its own tip.
    if tip_height < crossing_height {
        return Ok(true);
    }

    let end_exclusive = crossing_height
        .checked_add(1)
        .ok_or_else(|| Error::Network("Ironwood crossing height overflow".to_string()))?;
    let blocks = load_transition_window(client, network, end_exclusive, timestamp).await?;
    Ok(derive_activation_from_blocks(network, &blocks)? == Some(activation_height))
}

async fn discover_mainnet_activation_height(
    client: &LightClient,
    tip_height: u32,
    network: &Network,
) -> Result<Option<u32>> {
    let Some(timestamp) = network.ironwood_activation_timestamp else {
        return Ok(network.ironwood_activation_height);
    };
    if tip_height <= FULL_NODE_LOOKBACK_BLOCKS {
        return Ok(None);
    }

    // The full node starts its transition search 30 blocks behind the tip.
    // Requiring that block to have crossed the timestamp prevents one isolated
    // future-dated block from selecting the activation height.
    let stable_height = tip_height - FULL_NODE_LOOKBACK_BLOCKS;
    let stable_block = get_block_at(client, stable_height).await?;
    if stable_block.time <= timestamp {
        return Ok(None);
    }

    // Locate the timestamp neighborhood without replaying years of compact
    // blocks. The bounded backward scan below performs the exact transition
    // comparison and tolerates local timestamp reversals around the boundary.
    let mut low = network.sapling_activation_height;
    let mut high = stable_height;
    while low < high {
        let midpoint = low + (high - low) / 2;
        if get_block_at(client, midpoint).await?.time > timestamp {
            high = midpoint;
        } else {
            low = midpoint.saturating_add(1);
        }
    }

    let crossing_neighborhood = get_block_at(client, low).await?;
    if crossing_neighborhood.time <= timestamp {
        return Ok(None);
    }

    let mut blocks = load_transition_window(client, network, low, timestamp).await?;
    if blocks
        .last()
        .is_none_or(|block| block.height + 1 != crossing_neighborhood.height)
    {
        return Err(Error::Network(
            "Timestamp activation search returned a non-contiguous boundary".to_string(),
        ));
    }
    blocks.push(crossing_neighborhood);

    derive_activation_from_blocks(network, &blocks)
}

async fn load_transition_window(
    client: &LightClient,
    network: &Network,
    end_exclusive: u32,
    timestamp: u32,
) -> Result<Vec<CompactBlockData>> {
    let cutoff = timestamp.saturating_sub(FULL_NODE_SEARCH_WINDOW_SECONDS);
    let mut cursor = end_exclusive;
    let mut segments = Vec::new();
    while cursor > network.sapling_activation_height {
        let start = cursor
            .saturating_sub(SEARCH_CHUNK_BLOCKS)
            .max(network.sapling_activation_height);
        let mut segment = client.get_compact_block_range(start..cursor).await?;
        validate_contiguous_segment(start, cursor, &segment)?;

        if let Some(stop_index) = segment.iter().rposition(|block| block.time < cutoff) {
            segment.drain(..stop_index);
            segments.push(segment);
            break;
        }

        segments.push(segment);
        if start == network.sapling_activation_height {
            break;
        }
        cursor = start;
    }

    segments.reverse();
    Ok(segments.into_iter().flatten().collect())
}

async fn get_block_at(client: &LightClient, height: u32) -> Result<CompactBlockData> {
    let block = client.get_block(height).await?;
    if block.height != u64::from(height) {
        return Err(Error::Network(format!(
            "Timestamp activation search requested height {height}, server returned {}",
            block.height
        )));
    }
    Ok(block)
}

fn validate_contiguous_segment(
    start: u32,
    end_exclusive: u32,
    blocks: &[CompactBlockData],
) -> Result<()> {
    let expected_len = end_exclusive.saturating_sub(start) as usize;
    if blocks.len() != expected_len {
        return Err(Error::Network(format!(
            "Timestamp activation search returned {} blocks for {start}..{end_exclusive}, expected {expected_len}",
            blocks.len()
        )));
    }
    for (offset, block) in blocks.iter().enumerate() {
        let expected_height = u64::from(start) + offset as u64;
        if block.height != expected_height {
            return Err(Error::Network(format!(
                "Timestamp activation search returned height {}, expected {expected_height}",
                block.height
            )));
        }
    }
    Ok(())
}

fn derive_activation_from_blocks(
    network: &Network,
    blocks: &[CompactBlockData],
) -> Result<Option<u32>> {
    for pair in blocks.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        if previous.height + 1 != current.height {
            return Err(Error::Network(format!(
                "Non-contiguous timestamp transition at heights {} and {}",
                previous.height, current.height
            )));
        }
        let current_height = u32::try_from(current.height).map_err(|_| {
            Error::Network(format!("Block height out of range: {}", current.height))
        })?;
        if let Some(height) =
            network.derive_ironwood_activation_height(previous.time, current_height, current.time)
        {
            return Ok(Some(height));
        }
    }
    Ok(None)
}

fn branch_matches(
    network_type: NetworkType,
    height: u32,
    server_branch_id: &str,
    activation_height: Option<u32>,
) -> bool {
    let params = PirateNetwork::with_ironwood_activation_height(network_type, activation_height);
    let expected = u32::from(BranchId::for_height(&params, BlockHeight::from_u32(height)));
    parse_branch_id(server_branch_id) == Some(expected)
}

fn parse_branch_id(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    let normalized = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u32::from_str_radix(normalized, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirate_params::network::{
        MAINNET_IRONWOOD_ACTIVATION_DELAY, MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP,
    };

    fn block(height: u64, time: u32) -> CompactBlockData {
        CompactBlockData {
            proto_version: 1,
            height,
            hash: vec![height as u8; 32],
            prev_hash: vec![height.saturating_sub(1) as u8; 32],
            time,
            header: Vec::new(),
            transactions: Vec::new(),
        }
    }

    #[test]
    fn derives_the_first_strict_transition_in_the_full_node_window() {
        let timestamp = MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP;
        let blocks = vec![
            block(99, timestamp - 1),
            block(100, timestamp + 1),
            block(101, timestamp),
            block(102, timestamp + 2),
        ];

        assert_eq!(
            derive_activation_from_blocks(&Network::mainnet(), &blocks).unwrap(),
            Some(100 + MAINNET_IRONWOOD_ACTIVATION_DELAY)
        );
    }

    #[test]
    fn timestamp_equality_does_not_activate_ironwood() {
        let timestamp = MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP;
        let blocks = vec![block(99, timestamp - 1), block(100, timestamp)];

        assert_eq!(
            derive_activation_from_blocks(&Network::mainnet(), &blocks).unwrap(),
            None
        );
    }

    #[test]
    fn rejects_non_contiguous_transition_data() {
        let timestamp = MAINNET_IRONWOOD_ACTIVATION_TIMESTAMP;
        let blocks = vec![block(99, timestamp - 1), block(101, timestamp + 1)];

        assert!(derive_activation_from_blocks(&Network::mainnet(), &blocks).is_err());
    }
}
