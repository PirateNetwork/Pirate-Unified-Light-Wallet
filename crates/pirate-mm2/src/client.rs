///! MM2 RPC client with v2 API support

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MM2 RPC client
pub struct Mm2Client {
    endpoint: String,
    userpass: String,
    client: reqwest::Client,
}

impl Mm2Client {
    /// Create new MM2 client
    pub fn new(endpoint: String, userpass: String) -> Self {
        Self {
            endpoint,
            userpass,
            client: reqwest::Client::new(),
        }
    }

    /// Call MM2 RPC method
    async fn call<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: T,
    ) -> Result<R> {
        #[derive(Serialize)]
        struct RpcRequest<T> {
            method: String,
            userpass: String,
            #[serde(flatten)]
            params: T,
        }

        let request = RpcRequest {
            method: method.to_string(),
            userpass: self.userpass.clone(),
            params,
        };

        let response = self
            .client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Rpc(format!("HTTP error: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Rpc(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let result: R = response
            .json()
            .await
            .map_err(|e| Error::Rpc(format!("JSON decode error: {}", e)))?;

        Ok(result)
    }

    // Core API methods

    /// Get MM2 version
    pub async fn version(&self) -> Result<String> {
        #[derive(Serialize)]
        struct VersionParams {}

        #[derive(Deserialize)]
        struct VersionResponse {
            result: String,
        }

        let response: VersionResponse = self.call("version", VersionParams {}).await?;
        Ok(response.result)
    }

    /// Stop MM2
    pub async fn stop(&self) -> Result<()> {
        #[derive(Serialize)]
        struct StopParams {}

        #[derive(Deserialize)]
        struct StopResponse {}

        let _: StopResponse = self.call("stop", StopParams {}).await?;
        Ok(())
    }

    /// Get enabled coins
    pub async fn get_enabled_coins(&self) -> Result<Vec<String>> {
        #[derive(Serialize)]
        struct GetEnabledParams {}

        #[derive(Deserialize)]
        struct GetEnabledResponse {
            result: Vec<CoinInfo>,
        }

        #[derive(Deserialize)]
        struct CoinInfo {
            ticker: String,
        }

        let response: GetEnabledResponse = self.call("get_enabled_coins", GetEnabledParams {}).await?;
        Ok(response.result.into_iter().map(|c| c.ticker).collect())
    }

    // Coin enablement

    /// Enable coins
    pub async fn enable_coins(&self, coins: Vec<CoinConfig>) -> Result<Vec<String>> {
        #[derive(Serialize)]
        struct EnableCoinsParams {
            coins: Vec<CoinConfig>,
        }

        #[derive(Deserialize)]
        struct EnableCoinsResponse {
            result: EnableResult,
        }

        #[derive(Deserialize)]
        struct EnableResult {
            enabled: Vec<String>,
            failed: Vec<String>,
        }

        let response: EnableCoinsResponse = self.call(
            "enable_coins",
            EnableCoinsParams { coins },
        ).await?;

        if !response.result.failed.is_empty() {
            return Err(Error::Rpc(format!(
                "Failed to enable coins: {:?}",
                response.result.failed
            )));
        }

        Ok(response.result.enabled)
    }

    // Orderbook API

    /// Get orderbook
    pub async fn orderbook(&self, base: &str, rel: &str) -> Result<Orderbook> {
        #[derive(Serialize)]
        struct OrderbookParams {
            base: String,
            rel: String,
        }

        #[derive(Deserialize)]
        struct OrderbookResponse {
            asks: Vec<Order>,
            bids: Vec<Order>,
        }

        let response: OrderbookResponse = self.call(
            "orderbook",
            OrderbookParams {
                base: base.to_string(),
                rel: rel.to_string(),
            },
        ).await?;

        Ok(Orderbook {
            asks: response.asks,
            bids: response.bids,
        })
    }

    /// Get best orders
    pub async fn best_orders(
        &self,
        coin: &str,
        action: TradeAction,
        volume: &str,
    ) -> Result<Vec<BestOrder>> {
        #[derive(Serialize)]
        struct BestOrdersParams {
            coin: String,
            action: String,
            volume: String,
        }

        #[derive(Deserialize)]
        struct BestOrdersResponse {
            result: Vec<BestOrder>,
        }

        let response: BestOrdersResponse = self.call(
            "best_orders",
            BestOrdersParams {
                coin: coin.to_string(),
                action: action.to_string(),
                volume: volume.to_string(),
            },
        ).await?;

        Ok(response.result)
    }

    // Trading API

    /// Buy (atomic swap)
    pub async fn buy(&self, params: BuyParams) -> Result<SwapResult> {
        #[derive(Deserialize)]
        struct BuyResponse {
            result: SwapResult,
        }

        let response: BuyResponse = self.call("buy", params).await?;
        Ok(response.result)
    }

    /// Sell (atomic swap)
    pub async fn sell(&self, params: SellParams) -> Result<SwapResult> {
        #[derive(Deserialize)]
        struct SellResponse {
            result: SwapResult,
        }

        let response: SellResponse = self.call("sell", params).await?;
        Ok(response.result)
    }

    /// Get swap status
    pub async fn my_swap_status(&self, uuid: &str) -> Result<SwapStatus> {
        #[derive(Serialize)]
        struct SwapStatusParams {
            uuid: String,
        }

        #[derive(Deserialize)]
        struct SwapStatusResponse {
            result: SwapStatus,
        }

        let response: SwapStatusResponse = self.call(
            "my_swap_status",
            SwapStatusParams {
                uuid: uuid.to_string(),
            },
        ).await?;

        Ok(response.result)
    }

    /// Get my recent swaps
    pub async fn my_recent_swaps(&self, limit: u32) -> Result<Vec<SwapSummary>> {
        #[derive(Serialize)]
        struct RecentSwapsParams {
            limit: u32,
        }

        #[derive(Deserialize)]
        struct RecentSwapsResponse {
            result: SwapsResult,
        }

        #[derive(Deserialize)]
        struct SwapsResult {
            swaps: Vec<SwapSummary>,
        }

        let response: RecentSwapsResponse = self.call(
            "my_recent_swaps",
            RecentSwapsParams { limit },
        ).await?;

        Ok(response.result.swaps)
    }
}

// API types

/// Coin configuration for enablement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinConfig {
    pub coin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_history: Option<bool>,
}

/// Orderbook
#[derive(Debug, Clone)]
pub struct Orderbook {
    pub asks: Vec<Order>,
    pub bids: Vec<Order>,
}

/// Order in orderbook
#[derive(Debug, Clone, Deserialize)]
pub struct Order {
    pub coin: String,
    pub address: String,
    pub price: String,
    pub max_volume: String,
    pub min_volume: String,
    pub uuid: String,
}

/// Best order for trading
#[derive(Debug, Clone, Deserialize)]
pub struct BestOrder {
    pub coin: String,
    pub address: String,
    pub price: String,
    pub max_volume: String,
    pub min_volume: String,
}

/// Trade action
#[derive(Debug, Clone, Copy)]
pub enum TradeAction {
    Buy,
    Sell,
}

impl ToString for TradeAction {
    fn to_string(&self) -> String {
        match self {
            TradeAction::Buy => "buy".to_string(),
            TradeAction::Sell => "sell".to_string(),
        }
    }
}

/// Buy parameters
#[derive(Debug, Clone, Serialize)]
pub struct BuyParams {
    pub base: String,
    pub rel: String,
    pub volume: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
}

/// Sell parameters
#[derive(Debug, Clone, Serialize)]
pub struct SellParams {
    pub base: String,
    pub rel: String,
    pub volume: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
}

/// Swap result after initiating trade
#[derive(Debug, Clone, Deserialize)]
pub struct SwapResult {
    pub uuid: String,
    pub action: String,
    pub base: String,
    pub base_amount: String,
    pub rel: String,
    pub rel_amount: String,
    pub method: String,
    pub sender_pubkey: String,
    pub dest_pub_key: String,
}

/// Swap status
#[derive(Debug, Clone, Deserialize)]
pub struct SwapStatus {
    pub uuid: String,
    #[serde(rename = "type")]
    pub swap_type: String,
    pub my_info: Option<SwapPartyInfo>,
    pub events: Vec<SwapEvent>,
}

/// Swap party information
#[derive(Debug, Clone, Deserialize)]
pub struct SwapPartyInfo {
    pub my_coin: String,
    pub other_coin: String,
    pub my_amount: String,
    pub other_amount: String,
    pub started_at: u64,
}

/// Swap event
#[derive(Debug, Clone, Deserialize)]
pub struct SwapEvent {
    pub event: SwapEventType,
    pub timestamp: u64,
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}

/// Swap event type
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SwapEventType {
    Started,
    Negotiated,
    TakerFeeSent,
    MakerPaymentReceived,
    MakerPaymentWaitConfirmStarted,
    MakerPaymentValidatedAndConfirmed,
    TakerPaymentSent,
    TakerPaymentSpent,
    MakerPaymentSpent,
    Finished,
    StartFailed,
    NegotiateFailed,
    TakerFeeSendFailed,
    MakerPaymentValidateFailed,
    TakerPaymentTransactionFailed,
    TakerPaymentWaitConfirmFailed,
    TakerPaymentSpendFailed,
    MakerPaymentWaitRefundStarted,
    MakerPaymentRefunded,
    MakerPaymentRefundFailed,
}

/// Swap summary
#[derive(Debug, Clone, Deserialize)]
pub struct SwapSummary {
    pub uuid: String,
    #[serde(rename = "type")]
    pub swap_type: String,
    pub my_coin: String,
    pub other_coin: String,
    pub my_amount: String,
    pub other_amount: String,
    pub started_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_action_string() {
        assert_eq!(TradeAction::Buy.to_string(), "buy");
        assert_eq!(TradeAction::Sell.to_string(), "sell");
    }
}
