//! Lightwalletd gRPC sync client
//!
//! Provides efficient blockchain synchronization via lightwalletd
//! with batched trial decryption and rolling checkpoints.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::result_large_err)]

pub mod background;
pub mod background_logger;
mod block_cache;
mod bridge_tree_codec;
pub mod client;
pub mod error;
pub mod frontier;
pub mod orchard;
pub mod orchard_frontier;
pub mod pipeline;
pub mod privacy;
pub mod progress;
pub mod proto_types;
pub mod sapling;
pub mod sync;

pub use background::{
    BackgroundSyncConfig, BackgroundSyncMode, BackgroundSyncOrchestrator, BackgroundSyncResult,
};
pub use background_logger::{BackgroundSyncEvent, BackgroundSyncLogger};
pub use client::{
    bootstrap_transport, fetch_spki_pin, i2p_status, rotate_tor_exit, shutdown_transport,
    tor_status,
    BroadcastResult, CompactBlock, CompactBlockData, CompactOrchardAction, CompactOutput,
    CompactSaplingOutput, CompactSaplingSpend, CompactTx, LightClient, LightClientConfig,
    LightdInfo, RetryConfig, TlsConfig, TransactionStatus, TransportMode, TreeState,
    DEFAULT_LIGHTD_HOST, DEFAULT_LIGHTD_PORT, DEFAULT_LIGHTD_URL,
};
pub use error::{Error, Result};
pub use frontier::{FrontierSnapshot, SaplingCommitment, SaplingFrontier, SAPLING_TREE_DEPTH};
pub use orchard_frontier::{OrchardFrontier, OrchardFrontierSnapshot};
pub use pipeline::{
    DecryptedNote, PerfCounters, PerfSnapshot, PipelineConfig, PipelineResult, SyncPipeline,
    MINI_CHECKPOINT_INTERVAL, PIPELINE_BATCH_SIZE,
};
pub use pirate_net::{I2pStatus, TorStatus};
pub use privacy::{BackgroundSyncTunnelGuard, TunnelConfig, TunnelManager};
pub use progress::{PerfCountersSnapshot, SyncProgress, SyncStage};
pub use sync::{SyncConfig, SyncEngine};
