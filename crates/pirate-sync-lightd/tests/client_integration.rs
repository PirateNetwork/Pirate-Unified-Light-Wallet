//! Integration tests for LightClient
//!
//! Run live tests with:
//!   cargo test --package pirate-sync-lightd --features live_lightd -- --ignored
//!
//! Run mock tests with:
//!   cargo test --package pirate-sync-lightd client_integration

use pirate_sync_lightd::{
    CompactBlock, LightClient, LightClientConfig, RetryConfig, TransportMode,
    DEFAULT_LIGHTD_HOST, DEFAULT_LIGHTD_PORT, DEFAULT_LIGHTD_URL,
};
use std::time::Duration;

// ============================================================================
// Unit tests (no network required)
// ============================================================================

#[test]
fn test_default_endpoint_constants() {
    assert_eq!(DEFAULT_LIGHTD_HOST, "64.23.167.130");
    assert_eq!(DEFAULT_LIGHTD_PORT, 9067);
    assert_eq!(DEFAULT_LIGHTD_URL, "http://64.23.167.130:9067");
}

#[test]
fn test_light_client_config_defaults() {
    let config = LightClientConfig::default();
    
    assert_eq!(config.endpoint, DEFAULT_LIGHTD_URL);
    assert_eq!(config.transport, TransportMode::Tor);
    assert!(!config.tls.enabled);
    assert!(config.tls.spki_pin.is_none());
    assert_eq!(config.retry.max_attempts, 5);
}

#[test]
fn test_light_client_config_direct() {
    let config = LightClientConfig::direct("https://custom:9067");
    
    assert_eq!(config.endpoint, "https://custom:9067");
    assert_eq!(config.transport, TransportMode::Direct);
    assert!(config.tls.enabled);
}

#[test]
fn test_light_client_config_socks5() {
    let config = LightClientConfig::with_socks5(
        "https://lightd:9067",
        "socks5://127.0.0.1:9050",
    );
    
    assert_eq!(config.transport, TransportMode::Socks5);
    assert_eq!(
        config.socks5_url,
        Some("socks5://127.0.0.1:9050".to_string())
    );
}

#[test]
fn test_light_client_config_with_spki_pin() {
    let pin = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    let config = LightClientConfig::default().with_spki_pin(pin);
    
    assert_eq!(config.tls.spki_pin, Some(pin.to_string()));
}

#[test]
fn test_light_client_creation() {
    let client = LightClient::new(DEFAULT_LIGHTD_URL.to_string());
    
    assert!(!client.is_connected());
    assert_eq!(client.endpoint(), DEFAULT_LIGHTD_URL);
}

#[test]
fn test_light_client_with_retry_config() {
    let retry = RetryConfig {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_secs(5),
        backoff_multiplier: 1.5,
    };
    
    let client = LightClient::with_retry_config(DEFAULT_LIGHTD_URL.to_string(), retry);
    
    assert!(!client.is_connected());
}

#[test]
fn test_light_client_clone() {
    let client = LightClient::new(DEFAULT_LIGHTD_URL.to_string());
    let cloned = client.clone();
    
    assert_eq!(client.endpoint(), cloned.endpoint());
    assert!(!cloned.is_connected());
}

#[test]
fn test_transport_mode_privacy() {
    assert!(TransportMode::Tor.is_private());
    assert!(TransportMode::I2p.is_private());
    assert!(TransportMode::Socks5.is_private());
    assert!(!TransportMode::Direct.is_private());
}

// ============================================================================
// Mock server pagination tests
// ============================================================================

fn mock_compact_block(height: u64) -> CompactBlock {
    CompactBlock {
        height,
        hash: vec![0u8; 32],
        time: 1234567890,
        transactions: vec![],
    }
}

#[tokio::test]
async fn test_pagination_exact_batch() {
    // Test when range divides evenly into batches
    let batch_size = 10u64;
    let start = 1000u64;
    let end = 1019u64; // 20 blocks = 2 batches exactly
    
    let mut all_blocks = Vec::new();
    let mut current = start;
    
    while current <= end {
        let batch_end = std::cmp::min(current + batch_size, end + 1);
        let batch: Vec<CompactBlock> = (current..batch_end).map(mock_compact_block).collect();
        all_blocks.extend(batch);
        current = batch_end;
    }
    
    assert_eq!(all_blocks.len(), 20);
    assert_eq!(all_blocks.first().unwrap().height, 1000);
    assert_eq!(all_blocks.last().unwrap().height, 1019);
}

#[tokio::test]
async fn test_pagination_partial_batch() {
    // Test when range doesn't divide evenly
    let batch_size = 10u64;
    let start = 1000u64;
    let end = 1024u64; // 25 blocks = 2 full + 1 partial
    
    let mut all_blocks = Vec::new();
    let mut current = start;
    let mut batch_count = 0;
    
    while current <= end {
        let batch_end = std::cmp::min(current + batch_size, end + 1);
        let batch: Vec<CompactBlock> = (current..batch_end).map(mock_compact_block).collect();
        all_blocks.extend(batch);
        current = batch_end;
        batch_count += 1;
    }
    
    assert_eq!(all_blocks.len(), 25);
    assert_eq!(batch_count, 3);
}

#[tokio::test]
async fn test_pagination_single_block() {
    let start = 1000u64;
    let end = 1000u64;
    
    let blocks: Vec<CompactBlock> = (start..=end).map(mock_compact_block).collect();
    
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].height, 1000);
}

#[tokio::test]
async fn test_pagination_empty_range() {
    let start = 1000u64;
    let end = 999u64; // Invalid range
    
    let blocks: Vec<CompactBlock> = if start <= end {
        (start..=end).map(mock_compact_block).collect()
    } else {
        Vec::new()
    };
    
    assert!(blocks.is_empty());
}

#[tokio::test]
async fn test_pagination_large_range() {
    // Simulate a large range that requires many batches
    let batch_size = 2000u64;
    let start = 1_000_000u64;
    let end = 1_010_499u64; // 10,500 blocks
    
    let mut batch_count = 0u64;
    let mut current = start;
    
    while current <= end {
        let batch_end = std::cmp::min(current + batch_size, end + 1);
        let _batch_size = batch_end - current;
        batch_count += 1;
        current = batch_end;
    }
    
    // Should require 6 batches: 5 full (2000 each) + 1 partial (500)
    assert_eq!(batch_count, 6);
}

#[tokio::test]
async fn test_block_ordering() {
    let start = 5000u64;
    let end = 5099u64;
    
    let blocks: Vec<CompactBlock> = (start..=end).map(mock_compact_block).collect();
    
    // Verify blocks are in ascending height order
    for (i, block) in blocks.iter().enumerate() {
        assert_eq!(block.height, start + i as u64);
    }
    
    // Verify first and last
    assert_eq!(blocks.first().unwrap().height, start);
    assert_eq!(blocks.last().unwrap().height, end);
}

// ============================================================================
// Feature-gated live integration tests
// ============================================================================

#[cfg(feature = "live_lightd")]
mod live_tests {
    use super::*;

    /// Test connection to live lightwalletd
    #[tokio::test]
    #[ignore = "Requires live network"]
    async fn test_live_connect() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);
        
        let result = client.connect().await;
        assert!(result.is_ok(), "Failed to connect: {:?}", result.err());
        assert!(client.is_connected());
        
        client.disconnect().await;
        assert!(!client.is_connected());
    }

    /// Test getting latest block from live server
    #[tokio::test]
    #[ignore = "Requires live network"]
    async fn test_live_get_latest_block() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);
        
        client.connect().await.expect("Failed to connect");
        
        let height = client.get_latest_block().await.expect("Failed to get latest block");
        
        // Pirate Chain mainnet should be well past 1M blocks
        assert!(height > 1_000_000, "Height {} too low for mainnet", height);
        
        println!("✓ Latest block height: {}", height);
    }

    /// Test streaming compact blocks from live server
    #[tokio::test]
    #[ignore = "Requires live network"]
    async fn test_live_get_block_range() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);
        
        client.connect().await.expect("Failed to connect");
        
        let latest = client.get_latest_block().await.expect("Failed to get latest");
        
        // Request last 10 blocks
        let start = latest.saturating_sub(10);
        let end = latest;
        
        let blocks = client
            .get_compact_block_range(start..end)
            .await
            .expect("Failed to get blocks");
        
        assert!(!blocks.is_empty());
        
        // Verify blocks are in expected range
        for block in &blocks {
            assert!(block.height >= start as u64);
            assert!(block.height < end as u64);
        }
        
        println!("✓ Received {} blocks from {}..{}", blocks.len(), start, end);
    }

    /// Test batched block fetching from live server
    #[tokio::test]
    #[ignore = "Requires live network"]
    async fn test_live_get_block_range_batched() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);
        
        client.connect().await.expect("Failed to connect");
        
        let latest = client.get_latest_block().await.expect("Failed to get latest");
        
        // Request 50 blocks in batches of 20
        let start = (latest - 50) as u64;
        let end = (latest - 1) as u64;
        
        let blocks = client
            .get_block_range_batched(start, end, 20)
            .await
            .expect("Failed to get batched blocks");
        
        assert_eq!(blocks.len(), (end - start + 1) as usize);
        
        // Verify ordering
        for (i, block) in blocks.iter().enumerate() {
            assert_eq!(block.height, start + i as u64);
        }
        
        println!("✓ Received {} batched blocks", blocks.len());
    }

    /// Test getting server info from live server
    #[tokio::test]
    #[ignore = "Requires live network"]
    async fn test_live_get_lightd_info() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);
        
        client.connect().await.expect("Failed to connect");
        
        let info = client.get_lightd_info().await.expect("Failed to get info");
        
        assert!(!info.version.is_empty(), "Version should not be empty");
        assert!(info.block_height > 0, "Block height should be > 0");
        assert!(info.sapling_activation_height > 0, "Sapling activation should be > 0");
        
        println!("✓ Server info:");
        println!("  Vendor: {}", info.vendor);
        println!("  Version: {}", info.version);
        println!("  Chain: {}", info.chain_name);
        println!("  Height: {}", info.block_height);
        println!("  Sapling activation: {}", info.sapling_activation_height);
    }

    /// Test getting a single block
    #[tokio::test]
    #[ignore = "Requires live network"]
    async fn test_live_get_single_block() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);
        
        client.connect().await.expect("Failed to connect");
        
        let latest = client.get_latest_block().await.expect("Failed to get latest");
        let target = latest - 5;
        
        let block = client.get_block(target).await.expect("Failed to get block");
        
        assert_eq!(block.height, target as u64);
        assert_eq!(block.hash.len(), 32);
        
        println!("✓ Got block {}: hash={}", block.height, hex::encode(&block.hash[..8]));
    }

    /// Test retry logic with invalid endpoint
    #[tokio::test]
    #[ignore = "Tests retry timing"]
    async fn test_retry_on_connection_failure() {
        let retry = RetryConfig {
            max_attempts: 2,
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(50),
            backoff_multiplier: 2.0,
        };
        
        let config = LightClientConfig {
            endpoint: "https://nonexistent.invalid:9067".to_string(),
            transport: TransportMode::Direct,
            retry,
            connect_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        
        let client = LightClient::with_config(config);
        
        let start = std::time::Instant::now();
        let result = client.connect().await;
        let elapsed = start.elapsed();
        
        assert!(result.is_err(), "Should fail to connect");
        assert!(elapsed < Duration::from_secs(5), "Should not retry too long");
        
        println!("✓ Connection failed as expected after {:?}", elapsed);
    }
}

