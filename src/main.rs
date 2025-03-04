mod dex;
mod price_fetcher;

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::time;
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;
use dotenv::dotenv;
use std::time::Instant;
use crate::price_fetcher::PriceFetcher;
use crate::dex::raydium::RaydiumDex;
use crate::dex::DexType;
use crate::dex::meteora::MeteoraDex;
use crate::dex::orca::OrcaDex;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    
    println!("Starting multi-DEX arbitrage finder...");
    
    let rpc_url = std::env::var("RPC_URL").context("RPC_URL not set")?;
    let rpc_client = Arc::new(RpcClient::new(rpc_url));
    
    let mut price_fetcher = PriceFetcher::new(rpc_client);
    
    // Add DEXes
    let raydium_program_id = std::env::var("RAYDIUM_PROGRAM_ID")
        .context("RAYDIUM_PROGRAM_ID not set")?;
    price_fetcher.add_dex(DexType::Raydium(RaydiumDex::new(&raydium_program_id)?));

    let meteora_program_id = std::env::var("METEORA_PROGRAM_ID")
        .ok()
        .map(|s| s.to_string());
    let meteora_program_id = meteora_program_id.as_deref();
    price_fetcher.add_dex(DexType::Meteora(MeteoraDex::new(meteora_program_id)?));

    // Add Orca DEX - simplified initialization
    price_fetcher.add_dex(DexType::Orca(OrcaDex::new()));

    let update_interval = std::env::var("UPDATE_INTERVAL")
        .unwrap_or_else(|_| "300".to_string())
        .parse::<u64>()
        .context("Failed to parse UPDATE_INTERVAL")?;

    let min_price_difference = std::env::var("MIN_PRICE_DIFFERENCE")
        .unwrap_or_else(|_| "1.0".to_string())
        .parse::<f64>()
        .context("Failed to parse MIN_PRICE_DIFFERENCE")?;

    let data_dir = Path::new("data");
    fs::create_dir_all(data_dir).context("Failed to create data directory")?;

    let mut interval = time::interval(Duration::from_secs(update_interval));

    loop {
        let start = Instant::now();
        interval.tick().await;
        
        match price_fetcher.find_arbitrage_opportunities(min_price_difference).await {
            Ok(opportunities) => {
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let arb_file = data_dir.join(format!("arbitrage_opportunities_{}.log", timestamp));
                
                if let Err(e) = PriceFetcher::save_arbitrage_opportunities(&opportunities, &arb_file).await {
                    eprintln!("Error saving arbitrage opportunities: {}", e);
                }

                let duration = start.elapsed();
                println!("Found {} arbitrage opportunities in {:?}", opportunities.len(), duration);
            }
            Err(e) => {
                eprintln!("Error finding arbitrage opportunities: {}", e);
            }
        }
    }
}
