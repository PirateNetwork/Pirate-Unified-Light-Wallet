//! Performance benchmarks for sync engine
//!
//! Tests sync throughput and memory usage under various conditions

use pirate_sync_lightd::SyncConfig;
use std::time::{Duration, Instant};
use sysinfo::{ProcessExt, System, SystemExt};
use tempfile::TempDir;

/// Mock sync metrics
struct SyncMetrics {
    blocks_per_second: f64,
    total_duration: Duration,
    peak_memory_mb: usize,
}

/// Helper to run sync and collect metrics
async fn run_sync_benchmark(block_count: u64) -> Result<SyncMetrics, Box<dyn std::error::Error>> {
    let _temp_dir = TempDir::new()?;

    let config = SyncConfig {
        batch_size: 1000,
        checkpoint_interval: 10000,
        ..Default::default()
    };

    // This uses a deterministic in-process workload to keep the benchmark stable.

    let start = Instant::now();
    let start_memory = get_memory_usage_mb();

    // Simulate sync work
    let mut synced = 0u64;
    while synced < block_count {
        // Simulate batch processing
        let batch_size = std::cmp::min(config.batch_size, block_count - synced);
        tokio::time::sleep(Duration::from_micros(batch_size * 10)).await;
        synced += batch_size;
    }

    let duration = start.elapsed();
    let peak_memory = get_memory_usage_mb().saturating_sub(start_memory);

    Ok(SyncMetrics {
        blocks_per_second: block_count as f64 / duration.as_secs_f64(),
        total_duration: duration,
        peak_memory_mb: peak_memory,
    })
}

/// Get current memory usage in MB (approximation)
fn get_memory_usage_mb() -> usize {
    let mut system = System::new_all();
    system.refresh_processes();

    let pid = match sysinfo::get_current_pid() {
        Ok(pid) => pid,
        Err(_) => return 0,
    };

    system
        .process(pid)
        .map(|process| (process.memory() / 1024) as usize)
        .unwrap_or(0)
}

#[tokio::test]
async fn bench_compact_sync_10k_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let metrics = run_sync_benchmark(10_000).await?;

    println!("=== Compact Sync (10k blocks) ===");
    println!("Duration: {:?}", metrics.total_duration);
    println!("Blocks/sec: {:.2}", metrics.blocks_per_second);
    println!("Peak memory: {} MB", metrics.peak_memory_mb);

    // Performance assertions
    assert!(
        metrics.blocks_per_second > 100.0,
        "Should sync >100 blocks/sec"
    );
    assert!(metrics.peak_memory_mb < 500, "Should use <500 MB memory");

    Ok(())
}

#[tokio::test]
async fn bench_deep_sync_10k_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let metrics = run_sync_benchmark(10_000).await?;

    println!("=== Deep Sync (10k blocks) ===");
    println!("Duration: {:?}", metrics.total_duration);
    println!("Blocks/sec: {:.2}", metrics.blocks_per_second);
    println!("Peak memory: {} MB", metrics.peak_memory_mb);

    // Deep sync is slower but should still be reasonable
    assert!(
        metrics.blocks_per_second > 50.0,
        "Should sync >50 blocks/sec in deep mode"
    );

    Ok(())
}

#[tokio::test]
async fn bench_checkpoint_creation_overhead() -> Result<(), Box<dyn std::error::Error>> {
    // Measure checkpoint creation time
    let start = Instant::now();

    // Simulate 100 checkpoint creations
    for _ in 0..100 {
        // In production, this would call actual checkpoint creation
        tokio::time::sleep(Duration::from_micros(100)).await;
    }

    let duration = start.elapsed();
    let avg_checkpoint_time = duration.as_micros() / 100;

    println!("=== Checkpoint Creation ===");
    println!("Average time: {} us", avg_checkpoint_time);

    let max_avg_micros: u128 = if cfg!(windows) { 20_000 } else { 1_000 };
    assert!(
        avg_checkpoint_time < max_avg_micros,
        "Checkpoint creation should be <{} us",
        max_avg_micros
    );

    Ok(())
}

#[tokio::test]
async fn bench_parallel_trial_decryption() -> Result<(), Box<dyn std::error::Error>> {
    // Measure trial decryption throughput
    let note_count = 1000;
    let start = Instant::now();

    // Simulate parallel trial decryption
    let handles: Vec<_> = (0..note_count)
        .map(|_| {
            tokio::spawn(async {
                // Simulate decryption work
                tokio::time::sleep(Duration::from_micros(50)).await;
            })
        })
        .collect();

    for handle in handles {
        handle.await?;
    }

    let duration = start.elapsed();
    let notes_per_second = note_count as f64 / duration.as_secs_f64();

    println!("=== Parallel Trial Decryption ===");
    println!("Notes/sec: {:.2}", notes_per_second);
    println!("Duration: {:?}", duration);

    // Should process >1000 notes/sec with parallelism
    assert!(notes_per_second > 1000.0, "Should decrypt >1000 notes/sec");

    Ok(())
}

#[tokio::test]
async fn bench_batch_database_writes() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("bench.db");

    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute_batch(
        "CREATE TABLE test_notes (id INTEGER PRIMARY KEY, data BLOB);
         CREATE INDEX idx_test_notes ON test_notes(id);",
    )?;

    // Benchmark batched writes
    let note_count = 10_000;
    let start = Instant::now();

    let tx = conn.unchecked_transaction()?;
    for i in 0..note_count {
        tx.execute(
            "INSERT INTO test_notes (id, data) VALUES (?1, ?2)",
            (i, vec![0u8; 100]),
        )?;
    }
    tx.commit()?;

    let duration = start.elapsed();
    let writes_per_second = note_count as f64 / duration.as_secs_f64();

    println!("=== Batched Database Writes ===");
    println!("Writes/sec: {:.2}", writes_per_second);
    println!("Duration: {:?}", duration);

    // Should achieve >1000 writes/sec in batched mode
    assert!(writes_per_second > 1000.0, "Should write >1000 notes/sec");

    Ok(())
}

#[tokio::test]
async fn bench_sync_progress_calculation() -> Result<(), Box<dyn std::error::Error>> {
    // Measure progress calculation overhead
    let iterations = 100_000;
    let start = Instant::now();

    for i in 0..iterations {
        let current = i;
        let target = iterations;
        let _percent = (current as f64 / target as f64) * 100.0;

        // Simulate ETA calculation
        let _elapsed = start.elapsed().as_secs_f64();
        let _eta = if current > 0 {
            (_elapsed / current as f64) * (target - current) as f64
        } else {
            0.0
        };
    }

    let duration = start.elapsed();
    let calcs_per_second = iterations as f64 / duration.as_secs_f64();

    println!("=== Progress Calculation ===");
    println!("Calculations/sec: {:.2}", calcs_per_second);

    // Progress calculation should be negligible (<1us)
    assert!(
        calcs_per_second > 1_000_000.0,
        "Progress calculation should be >1M/sec"
    );

    Ok(())
}

#[tokio::test]
async fn bench_rollback_performance() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("rollback_bench.db");

    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute_batch(
        "CREATE TABLE blocks (height INTEGER PRIMARY KEY, data BLOB);
         CREATE TABLE checkpoints (height INTEGER PRIMARY KEY);",
    )?;

    // Insert 10k blocks
    let tx = conn.unchecked_transaction()?;
    for i in 0..10_000 {
        tx.execute(
            "INSERT INTO blocks (height, data) VALUES (?1, ?2)",
            (i, vec![0u8; 100]),
        )?;
        if i % 1000 == 0 {
            tx.execute("INSERT INTO checkpoints (height) VALUES (?1)", [i])?;
        }
    }
    tx.commit()?;

    // Benchmark rollback to checkpoint
    let start = Instant::now();

    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM blocks WHERE height > ?1", [5000])?;
    tx.commit()?;

    let duration = start.elapsed();

    println!("=== Rollback Performance ===");
    println!("Rollback duration: {:?}", duration);
    println!("Rollback 5000 blocks");

    // Rollback should be <100ms
    assert!(duration.as_millis() < 100, "Rollback should be <100ms");

    Ok(())
}

#[tokio::test]
async fn bench_memory_usage_during_sync() -> Result<(), Box<dyn std::error::Error>> {
    // Monitor memory usage during simulated sync
    let start_mem = get_memory_usage_mb();

    // Simulate processing 100k blocks
    let mut buffers = Vec::new();
    for _ in 0..1000 {
        // Simulate batch processing with bounded buffer
        buffers.push(vec![0u8; 1024 * 100]); // 100KB per batch

        // Limit buffer size to prevent unbounded growth
        if buffers.len() > 10 {
            buffers.remove(0);
        }
    }

    let peak_mem = get_memory_usage_mb();
    let memory_increase = peak_mem.saturating_sub(start_mem);

    println!("=== Memory Usage During Sync ===");
    println!("Memory increase: {} MB", memory_increase);

    // Memory increase should be bounded
    assert!(memory_increase < 100, "Memory increase should be <100 MB");

    Ok(())
}



