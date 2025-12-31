//! Lightwalletd gRPC proto type definitions
//!
//! These types mirror the proto definitions in `proto/service.proto` for the
//! Zcash/Pirate Chain lightwalletd CompactTxStreamer service.
//!
//! ## Design Decision: Manual vs Generated
//!
//! We use manually-defined proto types instead of tonic-build generation because:
//!
//! 1. **No protoc dependency** - Builds work on any system without requiring
//!    protobuf compiler installation
//!
//! 2. **Stable protocol** - The lightwalletd compact block protocol is a Zcash
//!    ecosystem standard that hasn't changed materially in years
//!
//! 3. **Complete implementation** - We include all methods (GetLatestBlock,
//!    GetBlockRange, SendTransaction, GetLightdInfo) that some generated versions omit
//!
//! 4. **Reproducible builds** - No external tool dependency at build time
//!
//! ## Regeneration
//!
//! If the proto changes, regenerate with:
//! ```bash
//! # Install protoc first
//! protoc --rust_out=. proto/service.proto
//! # Or use tonic-build (requires tonic-build crate)
//! ```
//!
//! ## Protocol Reference
//!
//! - Proto source: `proto/service.proto`
//! - Service: `pirate.wallet.sdk.rpc.CompactTxStreamer`
//! - Wire format: Protocol Buffers v3

#![allow(missing_docs)] // Proto fields don't need individual docs

use prost::Message;

/// Compact block format for efficient sync.
/// Contains only the data needed for trial decryption.
#[derive(Clone, PartialEq, Message)]
pub struct CompactBlock {
    #[prost(uint32, tag = "1")]
    pub proto_version: u32,
    #[prost(uint64, tag = "2")]
    pub height: u64,
    #[prost(bytes = "vec", tag = "3")]
    pub hash: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    pub prev_hash: Vec<u8>,
    #[prost(uint32, tag = "5")]
    pub time: u32,
    #[prost(bytes = "vec", tag = "6")]
    pub header: Vec<u8>,
    #[prost(message, repeated, tag = "7")]
    pub vtx: Vec<CompactTx>,
}

/// Compact transaction containing only shielded outputs.
#[derive(Clone, PartialEq, Message)]
pub struct CompactTx {
    #[prost(uint64, tag = "1")]
    pub index: u64,
    #[prost(bytes = "vec", tag = "2")]
    pub hash: Vec<u8>,
    #[prost(uint32, tag = "3")]
    pub fee: u32,
    #[prost(message, repeated, tag = "4")]
    pub spends: Vec<CompactSaplingSpend>,
    #[prost(message, repeated, tag = "5")]
    pub outputs: Vec<CompactSaplingOutput>,
    #[prost(message, repeated, tag = "6")]
    pub actions: Vec<CompactOrchardAction>,
}

/// Compact Sapling spend (nullifier only).
#[derive(Clone, PartialEq, Message)]
pub struct CompactSaplingSpend {
    #[prost(bytes = "vec", tag = "1")]
    pub nf: Vec<u8>,
}

/// Compact Sapling output for trial decryption.
/// Contains note commitment, ephemeral key, and first 52 bytes of ciphertext.
#[derive(Clone, PartialEq, Message)]
pub struct CompactSaplingOutput {
    #[prost(bytes = "vec", tag = "1")]
    pub cmu: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub ephemeral_key: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub ciphertext: Vec<u8>,
}

/// Compact Orchard action for trial decryption.
/// Contains nullifier, commitment, ephemeral key, and ciphertexts.
#[derive(Clone, PartialEq, Message)]
pub struct CompactOrchardAction {
    #[prost(bytes = "vec", tag = "1")]
    pub nullifier: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub cmx: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub ephemeral_key: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    pub ciphertext: Vec<u8>,
}

/// Block identifier by height and/or hash.
#[derive(Clone, PartialEq, Message)]
pub struct BlockId {
    #[prost(uint64, tag = "1")]
    pub height: u64,
    #[prost(bytes = "vec", tag = "2")]
    pub hash: Vec<u8>,
}

/// Block range request (inclusive on both ends).
#[derive(Clone, PartialEq, Message)]
pub struct BlockRange {
    #[prost(message, optional, tag = "1")]
    pub start: Option<BlockId>,
    #[prost(message, optional, tag = "2")]
    pub end: Option<BlockId>,
}

/// Transaction filter for GetTransaction RPC.
#[derive(Clone, PartialEq, Message)]
pub struct TxFilter {
    #[prost(message, optional, tag = "1")]
    pub block: Option<BlockId>,
    #[prost(uint64, tag = "2")]
    pub index: u64,
    #[prost(bytes = "vec", tag = "3")]
    pub hash: Vec<u8>,
}

/// Empty request/response message.
#[derive(Clone, Copy, PartialEq, Message)]
pub struct Empty {}

/// Chain specification for network selection.
#[derive(Clone, PartialEq, Message)]
pub struct ChainSpec {
    #[prost(string, tag = "1")]
    pub network: String,
}

/// Raw transaction data for broadcasting.
#[derive(Clone, PartialEq, Message)]
pub struct RawTransaction {
    #[prost(bytes = "vec", tag = "1")]
    pub data: Vec<u8>,
    #[prost(uint64, tag = "2")]
    pub height: u64,
}

/// Response from SendTransaction RPC.
#[derive(Clone, PartialEq, Message)]
pub struct SendResponse {
    #[prost(int32, tag = "1")]
    pub error_code: i32,
    #[prost(string, tag = "2")]
    pub error_message: String,
}

/// Lightwalletd server information.
#[derive(Clone, PartialEq, Message)]
pub struct LightdInfo {
    #[prost(string, tag = "1")]
    pub version: String,
    #[prost(string, tag = "2")]
    pub vendor: String,
    #[prost(bool, tag = "3")]
    pub taddr_support: bool,
    #[prost(string, tag = "4")]
    pub chain_name: String,
    #[prost(uint64, tag = "5")]
    pub sapling_activation_height: u64,
    #[prost(string, tag = "6")]
    pub consensus_branch_id: String,
    #[prost(uint64, tag = "7")]
    pub block_height: u64,
    #[prost(string, tag = "8")]
    pub git_commit: String,
    #[prost(string, tag = "9")]
    pub branch: String,
    #[prost(string, tag = "10")]
    pub build_date: String,
    #[prost(string, tag = "11")]
    pub build_user: String,
    #[prost(uint64, tag = "12")]
    pub estimated_height: u64,
    #[prost(string, tag = "13")]
    pub zcashd_build: String,
    #[prost(string, tag = "14")]
    pub zcashd_subversion: String,
}

/// Tree state for Sapling and Orchard note commitment trees.
#[derive(Clone, PartialEq, Message)]
pub struct TreeState {
    #[prost(string, tag = "1")]
    pub network: String,
    #[prost(uint64, tag = "2")]
    pub height: u64,
    #[prost(string, tag = "3")]
    pub hash: String,
    #[prost(uint32, tag = "4")]
    pub time: u32,
    #[prost(string, tag = "5")]
    pub sapling_tree: String,
    #[prost(string, tag = "6")]
    pub sapling_frontier: String,
    #[prost(string, tag = "7")]
    pub orchard_tree: String,
}

// ============================================================================
// gRPC Client Implementation
// ============================================================================

/// Generated-equivalent client for CompactTxStreamer service.
pub mod compact_tx_streamer_client {
    #![allow(unused_variables, dead_code, clippy::wildcard_imports, clippy::let_unit_value)]
    
    use super::*;
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;

    /// CompactTxStreamer gRPC client.
    ///
    /// Provides methods for syncing compact blocks and broadcasting transactions.
    #[derive(Debug, Clone)]
    pub struct CompactTxStreamerClient<T> {
        inner: tonic::client::Grpc<T>,
    }

    impl CompactTxStreamerClient<tonic::transport::Channel> {
        /// Create a new client from a channel.
        pub fn new(channel: tonic::transport::Channel) -> Self {
            let inner = tonic::client::Grpc::new(channel);
            Self { inner }
        }

        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }

    impl<T> CompactTxStreamerClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        /// Create client with a custom transport.
        pub fn with_inner(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }

        /// Create client with origin URI.
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }

        /// Get the latest block ID (height + hash).
        ///
        /// This is the primary method for getting the current chain tip.
        pub async fn get_latest_block(
            &mut self,
            request: impl tonic::IntoRequest<ChainSpec>,
        ) -> std::result::Result<tonic::Response<BlockId>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetLatestBlock",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetLatestBlock",
            ));
            self.inner.unary(req, path, codec).await
        }

        /// Get a single compact block by height or hash.
        pub async fn get_block(
            &mut self,
            request: impl tonic::IntoRequest<BlockId>,
        ) -> std::result::Result<tonic::Response<CompactBlock>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetBlock",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetBlock",
            ));
            self.inner.unary(req, path, codec).await
        }

        /// Stream compact blocks in a range (inclusive on both ends).
        ///
        /// This is the primary method for syncing - it efficiently streams
        /// all blocks in the specified range.
        pub async fn get_block_range(
            &mut self,
            request: impl tonic::IntoRequest<BlockRange>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<CompactBlock>>,
            tonic::Status,
        > {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetBlockRange",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetBlockRange",
            ));
            self.inner.server_streaming(req, path, codec).await
        }

        /// Get full transaction by hash (for memo decryption).
        ///
        /// Uses TxFilter where hash field is the transaction hash.
        /// Returns RawTransaction containing the complete transaction data
        /// with full 580-byte ciphertexts for memo decryption.
        pub async fn get_transaction(
            &mut self,
            request: impl tonic::IntoRequest<TxFilter>,
        ) -> std::result::Result<tonic::Response<RawTransaction>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetTransaction",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetTransaction",
            ));
            self.inner.unary(req, path, codec).await
        }

        /// Broadcast a raw transaction to the network.
        ///
        /// Returns SendResponse with error_code=0 on success.
        pub async fn send_transaction(
            &mut self,
            request: impl tonic::IntoRequest<RawTransaction>,
        ) -> std::result::Result<tonic::Response<SendResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/SendTransaction",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "SendTransaction",
            ));
            self.inner.unary(req, path, codec).await
        }

        /// Get lightwalletd server information.
        ///
        /// Returns version, chain name, current height, etc.
        pub async fn get_lightd_info(
            &mut self,
            request: impl tonic::IntoRequest<Empty>,
        ) -> std::result::Result<tonic::Response<LightdInfo>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetLightdInfo",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetLightdInfo",
            ));
            self.inner.unary(req, path, codec).await
        }

        /// Get tree state (Sapling and Orchard anchors) at a specific block height.
        ///
        /// If BlockID.height is 0, returns latest tree state.
        /// Returns TreeState with saplingTree and orchardTree (hex-encoded strings).
        /// Uses legacy z_gettreestatelegacy RPC for backward compatibility.
        pub async fn get_tree_state(
            &mut self,
            request: impl tonic::IntoRequest<BlockId>,
        ) -> std::result::Result<tonic::Response<TreeState>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetTreeState",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetTreeState",
            ));
            self.inner.unary(req, path, codec).await
        }

        /// Get tree state with bridge tree support (improved long-range sync performance).
        ///
        /// Uses updated z_gettreestate RPC with bridge trees format.
        /// The block can be specified by either height or hash.
        /// Returns TreeState with saplingTree and orchardTree in bridge tree format.
        pub async fn get_bridge_tree_state(
            &mut self,
            request: impl tonic::IntoRequest<BlockId>,
        ) -> std::result::Result<tonic::Response<TreeState>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetBridgeTreeState",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetBridgeTreeState",
            ));
            self.inner.unary(req, path, codec).await
        }

        /// Get optimal block group end height for sync batching.
        ///
        /// Groups blocks into ~4MB chunks for efficient sync.
        /// Returns the last block in a group starting from the given height.
        /// This helps optimize sync by using server-provided optimal batch sizes.
        pub async fn get_lite_wallet_block_group(
            &mut self,
            request: impl tonic::IntoRequest<BlockId>,
        ) -> std::result::Result<tonic::Response<BlockId>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::unknown(format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/pirate.wallet.sdk.rpc.CompactTxStreamer/GetLiteWalletBlockGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new(
                "pirate.wallet.sdk.rpc.CompactTxStreamer",
                "GetLiteWalletBlockGroup",
            ));
            self.inner.unary(req, path, codec).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_block_encoding() {
        let block = CompactBlock {
            proto_version: 1,
            height: 1000,
            hash: vec![0u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1234567890,
            header: vec![0u8; 32],
            vtx: vec![],
        };
        
        let encoded = block.encode_to_vec();
        let decoded = CompactBlock::decode(&encoded[..]).unwrap();
        
        assert_eq!(block, decoded);
    }

    #[test]
    fn test_block_range_encoding() {
        let range = BlockRange {
            start: Some(BlockId { height: 1000, hash: vec![] }),
            end: Some(BlockId { height: 2000, hash: vec![] }),
        };
        
        let encoded = range.encode_to_vec();
        let decoded = BlockRange::decode(&encoded[..]).unwrap();
        
        assert_eq!(range, decoded);
    }

    #[test]
    fn test_lightd_info_encoding() {
        let info = LightdInfo {
            version: "1.0.0".to_string(),
            vendor: "pirate".to_string(),
            taddr_support: false,
            chain_name: "ARRR".to_string(),
            sapling_activation_height: 1,
            consensus_branch_id: "test".to_string(),
            block_height: 1000000,
            git_commit: "abc123".to_string(),
            branch: "main".to_string(),
            build_date: "2024-01-01".to_string(),
            build_user: "builder".to_string(),
            estimated_height: 1000010,
            zcashd_build: "v4.0.0".to_string(),
            zcashd_subversion: "pirate".to_string(),
        };
        
        let encoded = info.encode_to_vec();
        let decoded = LightdInfo::decode(&encoded[..]).unwrap();
        
        assert_eq!(info, decoded);
    }
}
