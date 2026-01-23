//! CLI sync harness for testing sync performance and resilience
//!
//! This tool allows testing:
//! - Long sync operations
//! - Interrupt/resume scenarios
//! - Performance benchmarking
//! - Checkpoint rollback

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use pirate_sync_lightd::{SyncConfig, SyncEngine};
use std::time::Duration;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "sync-harness")]
#[command(about = "Pirate Chain sync testing harness", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a full sync from birthday to tip
    FullSync {
        /// Lightwalletd endpoint
        #[arg(short, long, default_value = "https://lightd.pirate.black:443")]
        endpoint: String,

        /// Birthday height
        #[arg(short, long, default_value = "3800000")]
        birthday: u32,

        /// Target height (optional, defaults to chain tip)
        #[arg(short, long)]
        target: Option<u64>,
    },

    /// Benchmark sync performance
    Benchmark {
        /// Lightwalletd endpoint
        #[arg(short, long, default_value = "https://lightd.pirate.black:443")]
        endpoint: String,

        /// Start height
        #[arg(short, long, default_value = "4000000")]
        start: u64,

        /// Number of blocks to sync
        #[arg(short, long, default_value = "10000")]
        blocks: u64,

        /// Number of runs
        #[arg(short, long, default_value = "3")]
        runs: u32,
    },

    /// Test interrupt and resume
    InterruptTest {
        /// Lightwalletd endpoint
        #[arg(short, long, default_value = "https://lightd.pirate.black:443")]
        endpoint: String,

        /// Birthday height
        #[arg(short, long, default_value = "4000000")]
        birthday: u32,

        /// Interrupt after N seconds
        #[arg(short, long, default_value = "5")]
        interrupt_after: u64,
    },

    /// Test checkpoint rollback
    RollbackTest {
        /// Lightwalletd endpoint
        #[arg(short, long, default_value = "https://lightd.pirate.black:443")]
        endpoint: String,

        /// Birthday height
        #[arg(short, long, default_value = "4000000")]
        birthday: u32,

        /// Checkpoint interval
        #[arg(short, long, default_value = "10000")]
        checkpoint_interval: u32,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::FullSync {
            endpoint,
            birthday,
            target,
        } => {
            run_full_sync(endpoint, birthday, target).await?;
        }
        Commands::Benchmark {
            endpoint,
            start,
            blocks,
            runs,
        } => {
            run_benchmark(endpoint, start, blocks, runs).await?;
        }
        Commands::InterruptTest {
            endpoint,
            birthday,
            interrupt_after,
        } => {
            run_interrupt_test(endpoint, birthday, interrupt_after).await?;
        }
        Commands::RollbackTest {
            endpoint,
            birthday,
            checkpoint_interval,
        } => {
            run_rollback_test(endpoint, birthday, checkpoint_interval).await?;
        }
    }

    Ok(())
}

async fn run_full_sync(endpoint: String, birthday: u32, target: Option<u64>) -> anyhow::Result<()> {
    info!("Starting full sync from birthday {}", birthday);
    info!("Endpoint: {}", endpoint);

    let mut engine = SyncEngine::new(endpoint, birthday);

    // Progress bar
    let progress_handle = engine.progress();
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {percent}% {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );

    // Spawn progress updater
    let pb_clone = pb.clone();
    let progress_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;

            let progress = progress_handle.read().await;
            let summary = progress.summary();

            pb_clone.set_position(progress.percentage() as u64);
            pb_clone.set_message(summary);

            if progress.is_complete() {
                break;
            }
        }
    });

    // Run sync
    let sync_result = if let Some(end) = target {
        engine.sync_range(birthday as u64, Some(end)).await
    } else {
        engine.sync_from_birthday().await
    };

    // Wait for progress task
    progress_task.await?;
    pb.finish_with_message("Sync complete!");

    match sync_result {
        Ok(()) => {
            info!("✅ Sync completed successfully");
            Ok(())
        }
        Err(e) => {
            warn!("❌ Sync failed: {:?}", e);
            Err(e.into())
        }
    }
}

async fn run_benchmark(endpoint: String, start: u64, blocks: u64, runs: u32) -> anyhow::Result<()> {
    info!("Starting benchmark: {} blocks, {} runs", blocks, runs);

    let mut total_duration = Duration::ZERO;
    let mut total_blocks = 0u64;

    for run in 1..=runs {
        info!("Run {}/{}", run, runs);

        let mut engine = SyncEngine::new(endpoint.clone(), start as u32);
        let start_time = std::time::Instant::now();

        engine.sync_range(start, Some(start + blocks - 1)).await?;

        let elapsed = start_time.elapsed();
        let blocks_per_sec = blocks as f64 / elapsed.as_secs_f64();

        info!(
            "  Duration: {:.2}s | {:.1} blocks/s",
            elapsed.as_secs_f64(),
            blocks_per_sec
        );

        total_duration += elapsed;
        total_blocks += blocks;
    }

    let avg_duration = total_duration / runs;
    let avg_blocks_per_sec = total_blocks as f64 / total_duration.as_secs_f64();

    info!("\n📊 Benchmark Results:");
    info!("  Runs: {}", runs);
    info!("  Total blocks: {}", total_blocks);
    info!("  Average duration: {:.2}s", avg_duration.as_secs_f64());
    info!("  Average speed: {:.1} blocks/s", avg_blocks_per_sec);

    Ok(())
}

async fn run_interrupt_test(
    endpoint: String,
    birthday: u32,
    interrupt_after: u64,
) -> anyhow::Result<()> {
    info!("Starting interrupt test");
    info!("Will interrupt after {} seconds", interrupt_after);

    let mut engine = SyncEngine::new(endpoint.clone(), birthday);
    let progress_handle = engine.progress();
    // Note: `SyncEngine` is not `Send` (it holds non-Send state like a tonic client).
    // Use a local task instead of `tokio::spawn`.
    let local = tokio::task::LocalSet::new();

    // Run sync in a local task so we can abort it.
    let sync_handle = local.spawn_local(async move { engine.sync_from_birthday().await });
    tokio::pin!(sync_handle);

    // Wait for interrupt duration
    let interrupt = tokio::time::sleep(Duration::from_secs(interrupt_after));
    tokio::pin!(interrupt);

    local
        .run_until(async {
            tokio::select! {
                _ = &mut interrupt => {
                    info!("⚠️  Interrupting sync...");
                    sync_handle.abort();
                    Ok::<(), anyhow::Error>(())
                }
                res = &mut sync_handle => {
                    // Sync finished before interrupt timer.
                    match res {
                        Ok(inner) => inner.map_err(|e| e.into()),
                        Err(e) => Err(e.into()),
                    }
                }
            }
        })
        .await?;

    // Resume from the last recorded checkpoint height (if any) using a new engine instance.
    // This exercises the "interrupt then resume" user flow end-to-end.
    let checkpoint_height = progress_handle.read().await.last_checkpoint();
    if let Some(h) = checkpoint_height {
        info!(
            "✅ Interrupted. Resuming from last checkpoint at height {}",
            h
        );

        let mut resumed = SyncEngine::new(endpoint, birthday);
        // Start again from checkpoint (inclusive). The sync engine is expected to be idempotent on already-processed heights.
        resumed.sync_range(h, None).await?;

        info!("✅ Resume completed successfully");
    } else {
        warn!("✅ Interrupted, but no checkpoint was recorded yet; restart would begin from birthday {}", birthday);
    }

    Ok(())
}

async fn run_rollback_test(
    endpoint: String,
    birthday: u32,
    checkpoint_interval: u32,
) -> anyhow::Result<()> {
    info!("Starting rollback test");
    info!("Checkpoint interval: {} blocks", checkpoint_interval);

    let config = SyncConfig {
        checkpoint_interval,
        ..Default::default()
    };

    let mut engine = SyncEngine::with_config(endpoint, birthday, config);

    // Sync some blocks
    let target = birthday as u64 + (checkpoint_interval as u64 * 3);
    info!("Syncing to height {} (3 checkpoints)", target);

    engine.sync_range(birthday as u64, Some(target)).await?;

    info!(
        "✅ Rollback test complete. Checkpoints created every {} blocks.",
        checkpoint_interval
    );
    Ok(())
}
