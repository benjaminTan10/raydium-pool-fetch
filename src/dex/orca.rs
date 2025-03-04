use super::DexProtocol;
use anyhow::Result;
use async_trait::async_trait;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::str::FromStr;
use solana_client::rpc_filter::{RpcFilterType, Memcmp};
use solana_client::rpc_config::RpcProgramAccountsConfig;

const ORCA_PROGRAM_ID: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";

#[derive(Clone)]
pub struct OrcaDex;

impl OrcaDex {
    pub fn new() -> Self {
        Self
    }

    async fn get_pool_price(
        &self,
        rpc_client: Arc<RpcClient>,
        token_mint: &str,
    ) -> anyhow::Result<Option<f64>> {
        let program_id = Pubkey::from_str(ORCA_PROGRAM_ID)?;
        let token_mint_pubkey = Pubkey::from_str(token_mint)?;

        // Filter for pools containing our token
        let filters = vec![
            RpcFilterType::DataSize(1328), // Whirlpool account size
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                8, // offset for token A mint
                token_mint_pubkey.to_bytes().to_vec(),
            )),
        ];

        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                ..Default::default()
            },
            ..Default::default()
        };

        let accounts = rpc_client.get_program_accounts_with_config(&program_id, config)?;

        let mut best_pool = None;
        let mut max_liquidity = 0u128;

        for (_, account) in accounts {
            if account.data.len() >= 1328 {
                // Parse liquidity from bytes
                let liquidity = u128::from_le_bytes(account.data[1224..1240].try_into()?);
                
                if liquidity > max_liquidity {
                    max_liquidity = liquidity;
                    best_pool = Some(account);
                }
            }
        }

        if let Some(pool) = best_pool {
            // Parse sqrt_price from bytes
            let sqrt_price = u128::from_le_bytes(pool.data[1240..1256].try_into()?);
            let price = (sqrt_price as f64 * sqrt_price as f64) / (1u128 << 128) as f64;
            
            // Get token decimals
            let token_a_decimals = pool.data[1264] as i32;
            let token_b_decimals = pool.data[1265] as i32;
            let decimal_adjustment = 10f64.powi(token_b_decimals - token_a_decimals);
            
            Ok(Some(price * decimal_adjustment))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl DexProtocol for OrcaDex {
    fn name(&self) -> &str {
        "Orca"
    }

    fn clone_box(&self) -> Box<dyn DexProtocol + Send + Sync> {
        Box::new(self.clone())
    }

    async fn get_token_price(&self, rpc_client: Arc<RpcClient>, token_mint: &str) -> anyhow::Result<Option<f64>> {
        self.get_pool_price(rpc_client, token_mint).await
    }
} 