/// query_demo — Step 7 roundtrip demonstration
///
/// Demonstrates the full store → retrieve → verify cycle for both
/// Type A (LoanLedger) and Type B (RepaymentLedger).
///
/// Run with:
///   cargo run -p fisco_ledger --bin query_demo
///
/// Requires:
///   • FISCO BCOS nodes running (WSL)
///   • ledger_config.json in the working directory

use fisco_ledger::{verify_roundtrip, LedgerClient, LedgerConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    let cfg    = LedgerConfig::load_default()?;
    let client = LedgerClient::new(cfg);

    // ── Sanity check ─────────────────────────────────────────────────────────
    let block = client.get_block_number().await?;
    println!("=== Step 7: Roundtrip Demo ===");
    println!("Current block: {}\n", block);

    // ── Type A (LoanLedger) ──────────────────────────────────────────────────
    let loan_id_a: u64    = 8001;
    let payload_a: &[u8]  = b"type-a-demo-payload-step7";

    println!("--- Type A ---");
    println!("Submitting {} bytes for loanId={}…", payload_a.len(), loan_id_a);
    let receipt_a = client.submit_type_a(loan_id_a, payload_a).await?;
    println!("  tx_hash:      {}", receipt_a.tx_hash);
    println!("  block_number: {}", receipt_a.block_number);
    println!("  gas_used:     {}", receipt_a.gas_used);

    println!("Querying loanId={}…", loan_id_a);
    let retrieved_a = client.query_type_a(loan_id_a).await?;
    println!("  retrieved {} bytes", retrieved_a.len());

    let result_a = verify_roundtrip(payload_a, &retrieved_a);
    println!(
        "  payload_len_ok = {}  hash_ok = {}  → {}",
        result_a.payload_len_ok,
        result_a.hash_ok,
        if result_a.is_ok() { "✓ PASS" } else { "✗ FAIL" }
    );

    // ── Type B (RepaymentLedger) ─────────────────────────────────────────────
    let loan_id_b: u64    = 8002;
    let payload_b: &[u8]  = b"type-b-demo-payload-step7";

    println!("\n--- Type B ---");
    println!("Submitting {} bytes for loanId={}…", payload_b.len(), loan_id_b);
    let receipt_b = client.submit_type_b(loan_id_b, payload_b).await?;
    println!("  tx_hash:      {}", receipt_b.tx_hash);
    println!("  block_number: {}", receipt_b.block_number);
    println!("  gas_used:     {}", receipt_b.gas_used);

    println!("Querying loanId={}…", loan_id_b);
    let retrieved_b = client.query_type_b(loan_id_b).await?;
    println!("  retrieved {} bytes", retrieved_b.len());

    let result_b = verify_roundtrip(payload_b, &retrieved_b);
    println!(
        "  payload_len_ok = {}  hash_ok = {}  → {}",
        result_b.payload_len_ok,
        result_b.hash_ok,
        if result_b.is_ok() { "✓ PASS" } else { "✗ FAIL" }
    );

    // ── Summary ──────────────────────────────────────────────────────────────
    println!("\n=== Summary ===");
    println!(
        "Type A: {}",
        if result_a.is_ok() { "store → retrieve → verify  ✓ PASS" }
        else                { "store → retrieve → verify  ✗ FAIL" }
    );
    println!(
        "Type B: {}",
        if result_b.is_ok() { "store → retrieve → verify  ✓ PASS" }
        else                { "store → retrieve → verify  ✗ FAIL" }
    );

    if result_a.is_ok() && result_b.is_ok() {
        println!("\nStep 7 complete ✓");
    } else {
        anyhow::bail!("Step 7 verification failed");
    }

    Ok(())
}
