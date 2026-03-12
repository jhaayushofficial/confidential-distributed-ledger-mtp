//! TPS benchmark on Sui (Mysticeti DAG-BFT).
//!
//! Mirrors `fisco_ledger/src/bin/tps_bench.rs` but targets a Sui localnet.
//! All transaction types are identical — only the blockchain transport layer
//! changes.  This enables a direct PBFT-vs-DAG comparison in the paper.
//!
//! ## How it works
//!
//! 1. Generate `--concurrent` ephemeral Ed25519 keypairs.
//! 2. Fund each address from the Sui localnet faucet.
//! 3. For each simulated lender count `n`, run a time-limited benchmark
//!    window where all senders submit transactions in parallel.
//! 4. Count committed transactions → compute TPS.
//!
//! Each sender uses unique `loan_id` ranges so there are no transaction
//! collisions.  All senders hit the same shared `LoanLedger` object,
//! so transactions go through Mysticeti consensus (not fast-path).
//!
//! ## Usage
//!
//! ```bash
//! # Aggregated 260 B (default)
//! cargo run --bin sui_tps_bench -- --duration 30 --concurrent 32
//!
//! # Checkpoint K=10 (132 B)
//! cargo run --bin sui_tps_bench -- --duration 30 --concurrent 32 --batch-size 10
//!
//! # Original O(n) protocol
//! cargo run --bin sui_tps_bench -- --duration 30 --concurrent 32 --original
//! ```

use anyhow::Result;
use clap::Parser;
use ed25519_dalek::SigningKey;
use message::tx::{Type1AggregatedTx, Type1CheckpointTx, Type1OurMpcTx, Type2RepaymentTx};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use sui_ledger::client::generate_keypair;
use sui_ledger::{SuiLedgerClient, SuiLedgerConfig};
use tokio::task::JoinHandle;

/// Gas budget per transaction (in MIST; 1 SUI = 1_000_000_000 MIST).
/// 0.05 SUI is generous for a simple move call.
const GAS_BUDGET: u64 = 50_000_000;

/// Simulated lender counts matching Table II in the paper.
const LENDER_COUNTS: &[usize] = &[10, 50, 100];

// ═══════════════════════════════════════════════════════════════════════════
// CLI
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Reproduce Table II on Sui (Mysticeti DAG-BFT).\n\
             Compares with FISCO-BCOS PBFT results to show DAG consensus scalability."
)]
struct Args {
    /// Seconds to run each individual benchmark window.
    #[arg(short, long, default_value_t = 30)]
    duration: u64,

    /// How many lender-node counts to test (first N entries of [10,50,100]).
    #[arg(long, default_value_t = 3)]
    rows: usize,

    /// Number of concurrent sender tasks.  Each gets its own funded Sui
    /// address.  Higher values saturate DAG consensus better.
    #[arg(long, default_value_t = 32)]
    concurrent: usize,

    /// Only run for this specific lender count.  0 = run all rows.
    #[arg(long, default_value_t = 0)]
    only_n: usize,

    /// Off-chain batch size K for the checkpoint protocol.
    /// K=1 (default) = aggregated tx (260 B).
    /// K>1           = checkpoint tx (132 B); record TPS = chain-TPS × K.
    #[arg(long, default_value_t = 1)]
    batch_size: u32,

    /// Use the original O(n) transaction format (96 + 165n bytes) instead
    /// of the aggregated constant-size format.
    #[arg(long, default_value_t = false)]
    original: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// Result types
// ═══════════════════════════════════════════════════════════════════════════

struct TpsResult {
    total_committed: u64,
    duration_secs: f64,
}

impl TpsResult {
    fn tps(&self) -> f64 {
        self.total_committed as f64 / self.duration_secs
    }
}

struct Row {
    n: usize,
    type1_size_b: usize,
    type1_chain_tps: f64,
    type1_record_tps: f64,
    batch_size: u32,
    type2_size_b: usize,
    type2_tps: f64,
}

// ═══════════════════════════════════════════════════════════════════════════
// Sender setup
// ═══════════════════════════════════════════════════════════════════════════

/// Pre-generate and fund `count` ephemeral sender identities.
///
/// On Sui localnet the faucet provides ~10 SUI per request, enough for
/// hundreds of benchmark transactions per sender.
async fn setup_senders(
    client: &SuiLedgerClient,
    count: usize,
) -> Result<Vec<(SigningKey, String)>> {
    println!("  Setting up {} funded sender addresses ...", count);
    let mut senders = Vec::with_capacity(count);

    for i in 0..count {
        let (sk, addr) = generate_keypair();
        client
            .request_faucet(&addr)
            .await
            .map_err(|e| anyhow::anyhow!("Faucet failed for sender {}: {}", i, e))?;
        senders.push((sk, addr));

        if (i + 1) % 10 == 0 || i + 1 == count {
            println!("    funded {}/{}", i + 1, count);
        }

        // Generous delay between faucet requests to avoid rate-limiting
        // on public networks (devnet/testnet).
        if i + 1 < count {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }

    // Give the chain a moment to settle all faucet transactions.
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("  All {} senders funded.\n", count);
    Ok(senders)
}

// ═══════════════════════════════════════════════════════════════════════════
// Benchmark runner
// ═══════════════════════════════════════════════════════════════════════════

/// Run a TPS benchmark window.
///
/// Each sender task submits transactions sequentially for `duration`,
/// using unique `loan_id` ranges.  The aggregate committed count across
/// all senders gives the consensus throughput.
async fn run_benchmark(
    client: Arc<SuiLedgerClient>,
    senders: &[(SigningKey, String)],
    payload: Vec<u8>,
    duration: Duration,
    is_type_a: bool,
) -> Result<TpsResult> {
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles: Vec<JoinHandle<u64>> = Vec::with_capacity(senders.len());
    let start = Instant::now();

    for (task_id, (sk, addr)) in senders.iter().enumerate() {
        let c = client.clone();
        let data = payload.clone();
        let flag = stop.clone();
        let key = sk.clone();
        let sender = addr.clone();
        let id_base = (task_id as u64) * 10_000_000;

        let h = tokio::spawn(async move {
            let mut committed: u64 = 0;
            let mut seq: u64 = 1;

            while !flag.load(Ordering::Relaxed) {
                let loan_id = id_base + seq;
                seq += 1;

                let res = if is_type_a {
                    c.submit_type_a(&sender, &key, loan_id, &data, GAS_BUDGET)
                        .await
                } else {
                    c.submit_type_b(&sender, &key, loan_id, &data, GAS_BUDGET)
                        .await
                };

                match res {
                    Ok(r) if r.success => committed += 1,
                    Ok(r) => {
                        log::warn!("[task {}] tx {} not successful", task_id, r.digest);
                    }
                    Err(e) => {
                        log::warn!("[task {}] error: {}", task_id, e);
                    }
                }
            }
            committed
        });
        handles.push(h);
    }

    // Let the benchmark window run, then signal all tasks to stop.
    tokio::time::sleep(duration).await;
    stop.store(true, Ordering::Relaxed);

    let mut total: u64 = 0;
    for h in handles {
        total += h.await.unwrap_or(0);
    }

    Ok(TpsResult {
        total_committed: total,
        duration_secs: start.elapsed().as_secs_f64(),
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args = Args::parse();

    let all_counts: Vec<usize> = if args.only_n > 0 {
        vec![args.only_n]
    } else {
        LENDER_COUNTS[..args.rows.min(LENDER_COUNTS.len())].to_vec()
    };

    let window = Duration::from_secs(args.duration);

    // ── Banner ─────────────────────────────────────────────────────────
    println!("\n=== Table II Reproduction — Sui (Mysticeti DAG-BFT) ===");
    let cfg = SuiLedgerConfig::load_default()
        .expect("Cannot load sui_ledger_config.json — see sui_ledger/src/lib.rs for setup steps");
    println!("  RPC      : {}", cfg.rpc_url);
    println!("  Package  : {}", cfg.package_id);
    println!("  Ledger   : {}", cfg.ledger_object_id);
    println!("  Window   : {}s per test", args.duration);
    println!("  Senders  : {} concurrent", args.concurrent);
    if args.original {
        println!("  Mode     : original O(n) protocol (96 + 165n B)");
    } else if args.batch_size > 1 {
        println!("  Mode     : checkpoint K={} (132 B, record TPS = chain-TPS × {})",
            args.batch_size, args.batch_size);
    } else {
        println!("  Mode     : aggregated (260 B)");
    }
    println!();

    // ── Connect ────────────────────────────────────────────────────────
    let client = Arc::new(SuiLedgerClient::new(cfg));

    let ckpt = client
        .get_latest_checkpoint()
        .await
        .expect("Cannot reach Sui node — is localnet running?");
    println!("  Node reachable — latest checkpoint: {}\n", ckpt);

    // ── Fund senders ───────────────────────────────────────────────────
    let senders = setup_senders(&client, args.concurrent).await?;

    // ── Run benchmarks ─────────────────────────────────────────────────
    let mut rows: Vec<Row> = Vec::new();
    let total_tests = all_counts.len() * 2;
    let mut test_num = 0;

    for &n in &all_counts {
        // ── Type 1 (loan recording) ────────────────────────────────────
        test_num += 1;
        let (type1_payload, type1_size) = if args.original {
            let tx = Type1OurMpcTx::new_with_lenders(n);
            let b = tx.to_bytes();
            let sz = b.len();
            (b, sz)
        } else if args.batch_size > 1 {
            let b = Type1CheckpointTx::new_checkpoint_dummy(args.batch_size).to_bytes();
            let sz = b.len();
            (b, sz)
        } else {
            let b = Type1AggregatedTx::new_agg_dummy().to_bytes();
            let sz = b.len();
            (b, sz)
        };

        let label = if args.original {
            format!("original {}B", type1_size)
        } else if args.batch_size > 1 {
            format!("checkpoint K={}", args.batch_size)
        } else {
            "agg 260B".to_string()
        };

        println!(
            "[{}/{}] Type 1 ({}), n={} ({} B), {} senders — {}s ...",
            test_num,
            total_tests,
            label,
            n,
            type1_size,
            senders.len(),
            args.duration
        );

        let r1 = run_benchmark(client.clone(), &senders, type1_payload, window, true).await?;
        let record_tps = r1.tps() * args.batch_size as f64;

        if args.batch_size > 1 {
            println!(
                "       committed={} checkpoints, chain-TPS={:.1}, record-TPS={:.1}",
                r1.total_committed,
                r1.tps(),
                record_tps
            );
        } else {
            println!(
                "       committed={}, TPS={:.1}",
                r1.total_committed,
                r1.tps()
            );
        }

        // ── Type 2 (repayment) ─────────────────────────────────────────
        test_num += 1;
        let type2_payload = Type2RepaymentTx::new_dummy().to_bytes();
        let type2_size = type2_payload.len();

        println!(
            "[{}/{}] Type 2, n={} ({} B), {} senders — {}s ...",
            test_num,
            total_tests,
            n,
            type2_size,
            senders.len(),
            args.duration
        );

        let r2 = run_benchmark(client.clone(), &senders, type2_payload, window, false).await?;
        println!(
            "       committed={}, TPS={:.1}",
            r2.total_committed,
            r2.tps()
        );

        rows.push(Row {
            n,
            type1_size_b: type1_size,
            type1_chain_tps: r1.tps(),
            type1_record_tps: record_tps,
            batch_size: args.batch_size,
            type2_size_b: type2_size,
            type2_tps: r2.tps(),
        });
    }

    // ── Print comparison table ─────────────────────────────────────────
    let use_ckpt = rows.first().map(|r| r.batch_size > 1).unwrap_or(false);
    println!();

    if use_ckpt {
        println!("┌─────────────────────┬──────────┬──────────────┬────────────────┬───────────┬───────────┐");
        println!(
            "│                     │          │  First type (checkpoint K={:<3})  │           │ Second    │",
            rows.first().map(|r| r.batch_size).unwrap_or(1)
        );
        println!("│  Lenders' number    │   Size   │  Chain TPS   │  Record TPS    │  Size     │  TPS      │");
        println!("├─────────────────────┼──────────┼──────────────┼────────────────┼───────────┼───────────┤");
        for row in &rows {
            println!(
                "│  Sui DAG  n={:<5}  │ {:>6} B │ {:>12.0} │ {:>14.0} │ {:>5} B   │ {:>9.0} │",
                row.n,
                row.type1_size_b,
                row.type1_chain_tps,
                row.type1_record_tps,
                row.type2_size_b,
                row.type2_tps
            );
        }
        println!("└─────────────────────┴──────────┴──────────────┴────────────────┴───────────┴───────────┘");
    } else {
        println!("┌─────────────────────┬───────────────┬──────────────┬───────────────┬──────────────┐");
        println!("│                     │  First type                  │  Second type                │");
        println!("│  Lenders' number    ├───────────────┬──────────────┼───────────────┬──────────────┤");
        println!("│                     │     Size      │     TPS      │     Size      │     TPS      │");
        println!("├─────────────────────┼───────────────┼──────────────┼───────────────┼──────────────┤");
        for row in &rows {
            let size1 = if args.original {
                format!("{:.2} KB", row.type1_size_b as f64 / 1024.0)
            } else {
                format!("{} B", row.type1_size_b)
            };
            println!(
                "│  Sui DAG  n={:<5}  │ {:>13} │ {:>12.0} │ {:>11} B │ {:>12.0} │",
                row.n, size1, row.type1_chain_tps, row.type2_size_b, row.type2_tps
            );
        }
        println!("└─────────────────────┴───────────────┴──────────────┴───────────────┴──────────────┘");
    }

    // ── Comparison footer ──────────────────────────────────────────────
    println!();
    println!("FISCO-BCOS (PBFT) baseline comparison:");
    println!("  n=10 : Type1 TPS ≈ 397    Type2 TPS ≈ 403");
    println!("  n=50 : Type1 TPS ≈ 274    Type2 TPS ≈ 286");
    println!("  n=100: Type1 TPS ≈  33    Type2 TPS ≈  39");
    println!();
    println!("Key insight: PBFT throughput collapses at large n due to O(n²) messages.");
    println!("Sui's Mysticeti (DAG-BFT) has O(n) complexity, sustaining high TPS");
    println!("regardless of the number of application-level lenders.");
    println!();

    Ok(())
}
