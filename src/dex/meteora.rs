use super::DexProtocol;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::str::FromStr;
use solana_client::rpc_filter::{RpcFilterType, Memcmp};
use solana_client::rpc_config::{RpcProgramAccountsConfig, RpcAccountInfoConfig};
use solana_account_decoder::UiAccountEncoding;
use solana_sdk::program_pack::Pack;
use spl_token::state::Mint;

const METEORA_PROGRAM_ID: &str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";
const POOL_STATE_SIZE: usize = 396; // Size of Meteora pool state account

#[derive(Clone)]
pub struct MeteoraDex {
    program_id: Pubkey,
}

#[derive(Debug)]
struct PoolState {
    token_mint_a: Pubkey,
    token_mint_b: Pubkey,
    token_vault_a: Pubkey,
    token_vault_b: Pubkey,
    reserve_a: u64,
    reserve_b: u64,
}

impl MeteoraDex {
    pub fn new(program_id: Option<&str>) -> Result<Self> {
        Ok(Self {
            program_id: Pubkey::from_str(
                program_id.unwrap_or(METEORA_PROGRAM_ID)
            )?,
        })
    }

    async fn get_pool_price(
        &self,
        rpc_client: Arc<RpcClient>,
        token_mint: &str,
    ) -> Result<Option<f64>> {
        // First try USDC pool
        if let Some(price) = Self::get_price_from_pool(
            self,
            rpc_client.clone(),
            token_mint,
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", // USDC mint
        ).await? {
            return Ok(Some(price));
        }

        // Then try SOL pool
        if let Some(price) = Self::get_price_from_pool(
            self,
            rpc_client,
            token_mint,
            "So111111111111111111111111111111111111111123", // SOL mint
        ).await? {
            return Ok(Some(price));
        }

        Ok(None)
    }

    async fn get_price_from_pool(
        &self,
        rpc_client: Arc<RpcClient>,
        token_a_mint: &str,
        token_b_mint: &str,
    ) -> Result<Option<f64>> {
        let token_a = Pubkey::from_str(token_a_mint)?;
        let token_b = Pubkey::from_str(token_b_mint)?;

        let filters = vec![
            RpcFilterType::DataSize(POOL_STATE_SIZE as u64),
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                8, // After discriminator
                token_a.to_bytes().to_vec(),
            )),
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                40, // After discriminator + token_a
                token_b.to_bytes().to_vec(),
            )),
        ];

        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                ..Default::default()
            },
            ..Default::default()
        };

        let accounts = rpc_client.get_program_accounts_with_config(&self.program_id, config)?;

        if accounts.is_empty() {
            return Ok(None);
        }

        let mut highest_liquidity = 0u64;
        let mut best_pool = None;

        for (_, account) in accounts {
            if let Ok(pool) = Self::deserialize_pool_state(&account.data) {
                let liquidity = pool.reserve_a.saturating_add(pool.reserve_b);
                if liquidity > highest_liquidity {
                    highest_liquidity = liquidity;
                    best_pool = Some(pool);
                }
            }
        }

        let pool = best_pool.ok_or_else(|| anyhow!("Failed to deserialize any valid pools"))?;

        // Calculate price from reserves
        let price = pool.reserve_b as f64 / pool.reserve_a as f64;

        // Adjust for decimals
        let token_a_decimals = Self::get_token_decimals(&rpc_client, &token_a).await?;
        let token_b_decimals = Self::get_token_decimals(&rpc_client, &token_b).await?;
        let decimal_adjustment = 10_f64.powi(token_b_decimals as i32 - token_a_decimals as i32);

        Ok(Some(price * decimal_adjustment))
    }

    async fn get_token_decimals(rpc_client: &RpcClient, mint: &Pubkey) -> Result<u8> {
        let account = rpc_client.get_account(mint)?;
        let mint_data = Mint::unpack_from_slice(&account.data)?;
        Ok(mint_data.decimals)
    }

    fn deserialize_pool_state(data: &[u8]) -> Result<PoolState> {
        if data.len() < POOL_STATE_SIZE {
            return Err(anyhow!("Data length too short for Pool State account"));
        }

        // Skip 8 bytes discriminator
        let data = &data[8..];

        Ok(PoolState {
            token_mint_a: Pubkey::try_from(&data[0..32]).unwrap(),
            token_mint_b: Pubkey::try_from(&data[32..64]).unwrap(),
            token_vault_a: Pubkey::try_from(&data[64..96]).unwrap(),
            token_vault_b: Pubkey::try_from(&data[96..128]).unwrap(),
            reserve_a: u64::from_le_bytes(data[128..136].try_into()?),
            reserve_b: u64::from_le_bytes(data[136..144].try_into()?),
        })
    }
}

#[async_trait]
impl DexProtocol for MeteoraDex {
    fn name(&self) -> &str {
        "Meteora"
    }

    fn clone_box(&self) -> Box<dyn DexProtocol + Send + Sync> {
        Box::new(self.clone())
    }

    async fn get_token_price(&self, rpc_client: Arc<RpcClient>, token_mint: &str) -> Result<Option<f64>> {
        Self::get_pool_price(self, rpc_client, token_mint).await
    }
}
