//! Performance tests for sync engine

use pirate_sync_lightd::{SyncConfig, SyncEngine};
use std::time::Instant;

#[tokio::test]
#[ignore] // Run with: cargo test --test performance -- --ignored
async fn test_sync_performance_small_range() {
    let mut engine = SyncEngine::new("https://lightd.pirate.black:443".to_string(), 4_000_000);

    let start = Instant::now();
    let result = engine.sync_range(4_000_000, Some(4_001_000)).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Sync should succeed");
    assert!(elapsed.as_secs() < 30, "Should complete within 30 seconds");

    let blocks_per_sec = 1_000.0 / elapsed.as_secs_f64();
    println!("Performance: {:.1} blocks/s", blocks_per_sec);

    // Should achieve at least 50 blocks/second
    assert!(
        blocks_per_sec >= 50.0,
        "Performance too low: {:.1} blocks/s",
        blocks_per_sec
    );
}

#[tokio::test]
#[ignore]
async fn test_sync_performance_with_parallelism() {
    let config = SyncConfig {
        batch_size: 500,
        max_parallel_decrypt: 8,
        ..Default::default()
    };

    let mut engine = SyncEngine::with_config(
        "https://lightd.pirate.black:443".to_string(),
        4_000_000,
        config,
    );

    let start = Instant::now();
    let result = engine.sync_range(4_000_000, Some(4_005_000)).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());

    let blocks_per_sec = 5_000.0 / elapsed.as_secs_f64();
    println!("Performance (8 workers): {:.1} blocks/s", blocks_per_sec);
}

#[tokio::test]
#[ignore]
async fn test_checkpoint_overhead() {
    // Test with frequent checkpoints
    let config_frequent = SyncConfig {
        checkpoint_interval: 1_000,
        ..Default::default()
    };

    let mut engine_frequent = SyncEngine::with_config(
        "https://lightd.pirate.black:443".to_string(),
        4_000_000,
        config_frequent,
    );

    let start = Instant::now();
    let result = engine_frequent.sync_range(4_000_000, Some(4_010_000)).await;
    let elapsed_frequent = start.elapsed();

    assert!(result.is_ok());

    // Test with infrequent checkpoints
    let config_infrequent = SyncConfig {
        checkpoint_interval: 10_000,
        ..Default::default()
    };

    let mut engine_infrequent = SyncEngine::with_config(
        "https://lightd.pirate.black:443".to_string(),
        4_000_000,
        config_infrequent,
    );

    let start = Instant::now();
    let result = engine_infrequent
        .sync_range(4_000_000, Some(4_010_000))
        .await;
    let elapsed_infrequent = start.elapsed();

    assert!(result.is_ok());

    println!("Frequent checkpoints (1K): {:?}", elapsed_frequent);
    println!("Infrequent checkpoints (10K): {:?}", elapsed_infrequent);

    // Checkpoint overhead should be minimal (< 10% difference)
    let overhead = (elapsed_frequent.as_secs_f64() / elapsed_infrequent.as_secs_f64()) - 1.0;
    assert!(
        overhead < 0.1,
        "Checkpoint overhead too high: {:.1}%",
        overhead * 100.0
    );
}

#[tokio::test]
#[ignore]
async fn test_batch_size_optimization() {
    let batch_sizes = vec![100, 500, 1000, 2000];
    let mut results = Vec::new();

    for batch_size in batch_sizes {
        let config = SyncConfig {
            batch_size,
            ..Default::default()
        };

        let mut engine = SyncEngine::with_config(
            "https://lightd.pirate.black:443".to_string(),
            4_000_000,
            config,
        );

        let start = Instant::now();
        let result = engine.sync_range(4_000_000, Some(4_005_000)).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());

        let blocks_per_sec = 5_000.0 / elapsed.as_secs_f64();
        results.push((batch_size, blocks_per_sec));

        println!("Batch size {}: {:.1} blocks/s", batch_size, blocks_per_sec);
    }

    // Find optimal batch size
    let optimal = results
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();

    println!(
        "Optimal batch size: {} ({:.1} blocks/s)",
        optimal.0, optimal.1
    );
}

#[tokio::test]
#[ignore]
async fn test_lazy_memo_performance() {
    // Test with lazy memo decoding
    let config_lazy = SyncConfig {
        lazy_memo_decode: true,
        ..Default::default()
    };

    let mut engine_lazy = SyncEngine::with_config(
        "https://lightd.pirate.black:443".to_string(),
        4_000_000,
        config_lazy,
    );

    let start = Instant::now();
    let result = engine_lazy.sync_range(4_000_000, Some(4_005_000)).await;
    let elapsed_lazy = start.elapsed();

    assert!(result.is_ok());

    // Test with eager memo decoding
    let config_eager = SyncConfig {
        lazy_memo_decode: false,
        ..Default::default()
    };

    let mut engine_eager = SyncEngine::with_config(
        "https://lightd.pirate.black:443".to_string(),
        4_000_000,
        config_eager,
    );

    let start = Instant::now();
    let result = engine_eager.sync_range(4_000_000, Some(4_005_000)).await;
    let elapsed_eager = start.elapsed();

    assert!(result.is_ok());

    println!("Lazy memo: {:?}", elapsed_lazy);
    println!("Eager memo: {:?}", elapsed_eager);

    // Lazy should be faster (fewer decryptions)
    assert!(
        elapsed_lazy < elapsed_eager,
        "Lazy memo should be faster than eager"
    );
}

#[tokio::test]
#[ignore]
async fn test_progress_overhead() {
    let mut engine = SyncEngine::new("https://lightd.pirate.black:443".to_string(), 4_000_000);

    let progress = engine.progress();

    let start = Instant::now();
    let result = engine.sync_range(4_000_000, Some(4_010_000)).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok());

    // Verify progress was tracked
    let final_progress = progress.read().await;
    assert!(final_progress.is_complete());
    assert_eq!(final_progress.current_height(), 4_010_000);

    println!("Sync with progress tracking: {:?}", elapsed);

    // Progress overhead should be minimal
    let blocks_per_sec = 10_000.0 / elapsed.as_secs_f64();
    assert!(
        blocks_per_sec >= 50.0,
        "Progress tracking overhead too high"
    );
}
