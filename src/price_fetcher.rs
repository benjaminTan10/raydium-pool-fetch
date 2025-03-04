use crate::dex::{DexType, TokenPrice, ArbitrageOpportunity};
use anyhow::{Context, Result};
use serde_json::Value;
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;
use std::path::Path;
use tokio::task;
use futures::future::join_all;
use std::fs;
use std::io::Write;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;

pub struct PriceFetcher {
    dexes: Vec<DexType>,
    rpc_client: Arc<RpcClient>,
}

impl PriceFetcher {
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            dexes: Vec::new(),
            rpc_client,
        }
    }

    pub fn add_dex(&mut self, dex: DexType) {
        self.dexes.push(dex);
    }

    pub async fn fetch_tokens() -> Result<Value> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://tokens.jup.ag/tokens?tags=birdeye-trending")
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .context("Failed to send request to Jupiter API")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch tokens: {}", response.status());
        }

        let tokens: Value = response.json().await.context("Failed to parse JSON response")?;
        Ok(tokens)
    }

    pub async fn fetch_all_prices(&self) -> Result<Vec<TokenPrice>> {
        let tokens = Self::fetch_tokens().await?;
        let tokens_array = tokens.as_array().context("Expected tokens array")?;

        let pb = ProgressBar::new((tokens_array.len() * self.dexes.len()) as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .expect("Failed to set progress bar style"));

        let mut all_tasks = Vec::new();

        for token in tokens_array {
            if let Some(address) = token["address"].as_str() {
                let dex_tasks: Vec<_> = self.dexes.iter().map(|dex| {
                    let rpc_client = self.rpc_client.clone();
                    let dex_name = dex.name().to_string();
                    let address = address.to_string();
                    let pb = pb.clone();
                    let dex = dex.clone();

                    task::spawn(async move {
                        let result = dex.get_token_price(rpc_client, &address).await;
                        pb.inc(1);
                        (address, dex_name, result)
                    })
                }).collect();

                all_tasks.extend(dex_tasks);
            }
        }

        let results = join_all(all_tasks).await;
        let mut prices = Vec::new();

        for result in results {
            if let Ok((address, dex_name, Ok(Some(price)))) = result {
                prices.push(TokenPrice {
                    token_address: address,
                    dex_name,
                    price,
                    timestamp: chrono::Local::now(),
                });
            }
        }

        pb.finish_with_message("Completed price fetching");
        Ok(prices)
    }

    pub async fn save_price_data(prices: &[TokenPrice], file_path: &Path) -> Result<()> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).context("Failed to create directory")?;
        }
    
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .context("Failed to open price log file")?;
    
        for price in prices {
            let log_entry = format!(
                "[{}] {} on {}: {} SOL\n",
                price.timestamp.format("%Y-%m-%d %H:%M:%S"),
                price.token_address,
                price.dex_name,
                price.price
            );
            file.write_all(log_entry.as_bytes())?;
            println!("{}", log_entry.trim());
        }
    
        Ok(())
    }

    pub async fn find_arbitrage_opportunities(&self, min_difference: f64) -> Result<Vec<ArbitrageOpportunity>> {
        let tokens = Self::fetch_tokens().await?;
        let tokens_array = tokens.as_array().context("Expected tokens array")?;

        let pb = ProgressBar::new(tokens_array.len() as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .expect("Failed to set progress bar style"));

        let mut opportunities = Vec::new();
        let mut price_map: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for token in tokens_array {
            if let Some(address) = token["address"].as_str() {
                let token_name = token["name"].as_str().map(String::from);
                let mut token_prices = HashMap::new();

                for dex in &self.dexes {
                    if let Ok(Some(price)) = dex.get_token_price(self.rpc_client.clone(), address).await {
                        token_prices.insert(dex.name().to_string(), price);
                    }
                }

                if token_prices.len() >= 2 {
                    price_map.insert(address.to_string(), token_prices);
                }
            }
            pb.inc(1);
        }

        for (token_address, prices) in price_map {
            if let (Some(raydium_price), Some(meteora_price)) = (
                prices.get("Raydium"),
                prices.get("Meteora")
            ) {
                let price_diff_percent = ((*raydium_price - *meteora_price).abs() / 
                    (*meteora_price).min(*raydium_price)) * 100.0;

                if price_diff_percent >= min_difference {
                    let token_name = tokens_array.iter()
                        .find(|t| t["address"].as_str() == Some(&token_address))
                        .and_then(|t| t["name"].as_str().map(String::from));

                    opportunities.push(ArbitrageOpportunity::new(
                        token_address.clone(),
                        token_name,
                        *raydium_price,
                        *meteora_price
                    ));
                }
            }
        }

        opportunities.sort_by(|a, b| b.price_difference_percent.partial_cmp(&a.price_difference_percent).unwrap());

        pb.finish_with_message("Completed arbitrage analysis");
        Ok(opportunities)
    }

    pub async fn save_arbitrage_opportunities(opportunities: &[ArbitrageOpportunity], file_path: &Path) -> Result<()> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).context("Failed to create directory")?;
        }

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .context("Failed to open arbitrage log file")?;

        for opp in opportunities {
            let token_name = opp.token_name.as_deref().unwrap_or("Unknown");
            let log_entry = format!(
                "[{}] Token: {} ({})\n\tRaydium: {} SOL\n\tMeteora: {} SOL\n\tDifference: {:.2}%\n",
                opp.timestamp.format("%Y-%m-%d %H:%M:%S"),
                token_name,
                opp.token_address,
                opp.raydium_price,
                opp.meteora_price,
                opp.price_difference_percent
            );
            file.write_all(log_entry.as_bytes())?;
            println!("{}", log_entry.trim());
        }

        Ok(())
    }
} 