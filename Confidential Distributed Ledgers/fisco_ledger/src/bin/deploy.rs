/// deploy — deploys LoanLedger and RepaymentLedger contracts to FISCO-BCOS.
///
/// Uses the pre-compiled .bin bytecode from the console contracts directory.
/// Prints the deployed addresses so they can be updated in ledger_config.json.

use fisco_ledger::{LedgerClient, LedgerConfig};
use std::fs;

const LOAN_LEDGER_BIN: &str =
    "/mnt/c/MTP/console/contracts/.compiled/LoanLedger.bin";
const REPAYMENT_LEDGER_BIN: &str =
    "/mnt/c/MTP/console/contracts/.compiled/RepaymentLedger.bin";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    let cfg = LedgerConfig::load_default()?;
    let client = LedgerClient::new(cfg.clone());

    // Current state
    let block = client.get_block_number().await?;
    println!("Current block number: {}", block);

    // Deploy LoanLedger
    println!("\n=== Deploying LoanLedger ===");
    let loan_bytecode = fs::read_to_string(LOAN_LEDGER_BIN)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", LOAN_LEDGER_BIN, e))?;
    let loan_addr = client.deploy_and_get_address(&loan_bytecode).await?;
    println!("LoanLedger deployed at: {}", loan_addr);

    // Deploy RepaymentLedger
    println!("\n=== Deploying RepaymentLedger ===");
    let repay_bytecode = fs::read_to_string(REPAYMENT_LEDGER_BIN)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", REPAYMENT_LEDGER_BIN, e))?;
    let repay_addr = client.deploy_and_get_address(&repay_bytecode).await?;
    println!("RepaymentLedger deployed at: {}", repay_addr);

    println!("\n=== Update ledger_config.json ===");
    println!("  \"loan_ledger_address\": \"{}\",", loan_addr);
    println!("  \"repayment_ledger_address\": \"{}\",", repay_addr);

    // Auto-update ledger_config.json
    let config_path = "/mnt/c/MTP/Confidential Distributed Ledgers/fisco_ledger/ledger_config.json";
    let config_str = fs::read_to_string(config_path)?;
    let mut config: serde_json::Value = serde_json::from_str(&config_str)?;
    config["loan_ledger_address"] = serde_json::Value::String(loan_addr.clone());
    config["repayment_ledger_address"] = serde_json::Value::String(repay_addr.clone());
    let new_config = serde_json::to_string_pretty(&config)?;
    fs::write(config_path, new_config)?;
    println!("\nledger_config.json updated automatically.");

    Ok(())
}
