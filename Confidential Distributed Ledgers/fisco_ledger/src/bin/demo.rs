/// ledger_demo — debug build that prints raw tx hex for inspection

use fisco_ledger::{LedgerClient, LedgerConfig};
use fisco_ledger::abi;
use fisco_ledger::transaction::TxSigner;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let cfg = LedgerConfig::load_default()?;
    let client = LedgerClient::new(cfg.clone());

    // 1. Verify getBlockNumber works from Rust
    println!("=== Debug Mode ===\n");
    let block = client.get_block_number().await?;
    println!("Current block number: {}", block);
    println!("Block limit will be: {}\n", block + 600);

    // 2. Print derived sender address
    let signer = TxSigner::from_pem_file(&cfg.private_key_pem)?;
    println!("Derived sender address: 0x{}", hex::encode(&signer.sender));
    println!("Config account:         {}\n", cfg.account_address);

    // 3. Print the raw TARS-encoded transaction hex (first 80 bytes)
    let input = abi::encode_record_type_a(1, b"hello");
    let raw_tx = signer.sign_tx(
        &cfg.chain_id,
        &cfg.group_id,
        block + 600,
        &cfg.loan_ledger_address,
        &input,
    )?;

    println!("Raw TX (first 160 hex chars):");
    println!("{}", &raw_tx[..raw_tx.len().min(162)]);
    println!("Total TX hex length: {}\n", raw_tx.len());

    // Expected TARS header bytes for TransactionData nested in Transaction:
    // [0x0C] StructBegin tag=0
    // [0x0B] Zero tag=0 (version=0)
    // [0x16 0x06 chain0] String1 tag=1 (chain_id)
    // [0x26 0x06 group0] String1 tag=2 (group_id)
    // [0x31 ...] Int16 tag=3 (blockLimit)
    // -> Search for "6368" (=chain) and "6772" (=group) in the hex above

    // 4. Try sending
    println!("=== Attempting sendTransaction ===\n");
    let receipt = client.submit_type_a(1, b"hello").await;
    match receipt {
        Ok(r) => {
            println!("SUCCESS!");
            println!("  tx_hash:      {}", r.tx_hash);
            println!("  block_number: {}", r.block_number);
            println!("  gas_used:     {}", r.gas_used);
        }
        Err(e) => println!("Error: {}", e),
    }

    Ok(())
}
