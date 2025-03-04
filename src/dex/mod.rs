use anyhow::Result;
use chrono::{DateTime, Local};
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;
use async_trait::async_trait;

pub mod raydium;
pub mod meteora;
pub mod orca;

use raydium::RaydiumDex;
use meteora::MeteoraDex;
use orca::OrcaDex;

#[async_trait]
pub trait DexProtocol: Send + Sync {
    fn name(&self) -> &str;
    fn clone_box(&self) -> Box<dyn DexProtocol + Send + Sync>;
    async fn get_token_price(&self, rpc_client: Arc<RpcClient>, token_mint: &str) -> Result<Option<f64>>;
}

#[derive(Debug, Clone)]
pub struct TokenPrice {
    pub token_address: String,
    pub dex_name: String,
    pub price: f64,
    pub timestamp: DateTime<Local>,
}

#[derive(Clone)]
pub enum DexType {
    Raydium(RaydiumDex),
    Meteora(MeteoraDex),
    Orca(OrcaDex),
}

impl DexType {
    pub fn name(&self) -> &str {
        match self {
            DexType::Raydium(_) => "Raydium",
            DexType::Meteora(_) => "Meteora",
            DexType::Orca(_) => "Orca",
        }
    }

    pub async fn get_token_price(&self, rpc_client: Arc<RpcClient>, token_mint: &str) -> Result<Option<f64>> {
        match self {
            DexType::Raydium(dex) => dex.get_token_price(rpc_client, token_mint).await,
            DexType::Meteora(dex) => dex.get_token_price(rpc_client, token_mint).await,
            DexType::Orca(dex) => dex.get_token_price(rpc_client, token_mint).await,
        }
    }
}

#[derive(Debug)]
pub struct ArbitrageOpportunity {
    pub token_address: String,
    pub token_name: Option<String>,
    pub raydium_price: f64,
    pub meteora_price: f64,
    pub price_difference_percent: f64,
    pub timestamp: DateTime<Local>,
}

impl ArbitrageOpportunity {
    pub fn new(
        token_address: String, 
        token_name: Option<String>,
        raydium_price: f64, 
        meteora_price: f64
    ) -> Self {
        let price_difference_percent = ((raydium_price - meteora_price).abs() / 
            meteora_price.min(raydium_price)) * 100.0;
            
        Self {
            token_address,
            token_name,
            raydium_price,
            meteora_price, 
            price_difference_percent,
            timestamp: Local::now(),
        }
    }
} 
