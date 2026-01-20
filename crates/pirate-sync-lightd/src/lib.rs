//! Lightwalletd gRPC sync client
//!
//! Provides efficient blockchain synchronization via lightwalletd
//! with batched trial decryption and rolling checkpoints.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod background;
pub mod background_logger;
mod bridge_tree_codec;
mod block_cache;
pub mod client;
pub mod error;
pub mod frontier;
pub mod orchard_frontier;
pub mod pipeline;
pub mod privacy;
pub mod progress;
pub mod proto_types;
pub mod sync;
pub mod sapling;
pub mod orchard;

pub use background::{BackgroundSyncMode, BackgroundSyncOrchestrator, BackgroundSyncConfig, BackgroundSyncResult};
pub use background_logger::{BackgroundSyncLogger, BackgroundSyncEvent};
pub use client::{
    LightClient, LightClientConfig, RetryConfig, TransportMode, TlsConfig,
    CompactBlock, CompactTx, CompactSaplingSpend, CompactSaplingOutput, CompactOrchardAction, CompactOutput, BroadcastResult, LightdInfo,
    TransactionStatus, CompactBlockData, TreeState,
    DEFAULT_LIGHTD_HOST, DEFAULT_LIGHTD_PORT, DEFAULT_LIGHTD_URL,
    bootstrap_transport, tor_status, rotate_tor_exit, i2p_status, shutdown_transport,
};
pub use error::{Error, Result};
pub use pirate_net::{TorStatus, I2pStatus};
pub use frontier::{SaplingFrontier, SaplingCommitment, FrontierSnapshot, SAPLING_TREE_DEPTH};
pub use orchard_frontier::{OrchardFrontier, OrchardFrontierSnapshot};
pub use pipeline::{
    SyncPipeline, PipelineConfig, PipelineResult, PerfCounters, PerfSnapshot, DecryptedNote,
    PIPELINE_BATCH_SIZE, MINI_CHECKPOINT_INTERVAL,
};
pub use privacy::{TunnelConfig, TunnelManager, BackgroundSyncTunnelGuard};
pub use progress::{SyncProgress, SyncStage, PerfCountersSnapshot};
pub use sync::{SyncConfig, SyncEngine};
