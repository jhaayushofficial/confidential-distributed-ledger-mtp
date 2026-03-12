use anyhow::Result;
use clap::Parser;
use fisco_ledger::{LedgerClient, LedgerConfig};
use message::tx::{Type1AggregatedTx, Type1CheckpointTx, Type2RepaymentTx};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Lender counts to test — matches Table II rows in the paper.
const LENDER_COUNTS: &[usize] = &[10, 50, 100];

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Reproduce Table II (Our MPC) from the paper.\n\
             Runs TPS benchmarks for n=10,50,100 lenders on a local FISCO BCOS node."
)]
struct Args {
    /// Seconds to run each individual benchmark window.
    #[arg(short, long, default_value_t = 30)]
    duration: u64,

    /// How many lender-node counts to test (first N entries of [10,50,100]).
    /// Default 3 = run all rows of Table II.
    #[arg(long, default_value_t = 3)]
    rows: usize,

    /// Number of concurrent sender tasks used to saturate the blockchain.
    /// In the paper, each lender IS a consensus node, so the blockchain has
    /// n consensus nodes but many transactions submitted from outside.
    /// Set high (e.g. 200) to ensure the txpool is always full, so the
    /// blockchain's PBFT throughput is the bottleneck, not sender count.
    /// Default 0 means "use n" (backward-compatible single-machine mode).
    #[arg(long, default_value_t = 0)]
    concurrent: usize,

    /// Only run for this specific lender count (used by the multi-node driver
    /// script to run one row at a time on a freshly-built n-node network).
    /// 0 = run all rows (default).
    #[arg(long, default_value_t = 0)]
    only_n: usize,

    /// Off-chain batch size K for the checkpoint protocol.
    /// K=1 (default) = submit one aggregated tx per record (existing behaviour, 260 B).
    /// K>1           = each on-chain checkpoint commits K records (132 B on-chain);
    ///                 effective record TPS = chain-TPS × K.
    #[arg(long, default_value_t = 1)]
    batch_size: u32,
}

struct TpsResult {
    total_committed: u64,
    duration_secs: f64,
}

impl TpsResult {
    fn tps(&self) -> f64 {
        self.total_committed as f64 / self.duration_secs
    }
}

/// One row of Table II output.
struct Row {
    n: usize,
    type1_size_b: usize,
    /// Chain-level TPS (checkpoint submissions per second).
    type1_chain_tps: f64,
    /// Effective record TPS = chain_tps × batch_size.
    type1_record_tps: f64,
    /// K value used for this row.
    batch_size: u32,
    type2_size_b: usize,
    type2_tps: f64,
}

/// Run a TPS benchmark:
/// - `n_concurrent` tasks each loop-submit transactions for `duration`.
/// - Uses an AtomicBool stop flag so tasks exit cleanly after the window.
/// - Each task uses a unique incrementing loan_id to avoid any replay filtering.
/// - `block_limit` is fetched once before the run to avoid per-tx RPC overhead.
async fn run_benchmark(
    client: Arc<LedgerClient>,
    tx_bytes: Vec<u8>,
    duration: Duration,
    n_concurrent: usize,
    is_type_a: bool,
    block_limit: i64,
) -> Result<TpsResult> {
    let stop = Arc::new(AtomicBool::new(false));
    let mut join_handles: Vec<JoinHandle<u64>> = Vec::with_capacity(n_concurrent);
    let start_time = Instant::now();

    for task_id in 0..n_concurrent {
        let client_clone = client.clone();
        let payload = tx_bytes.clone();
        let stop_flag = stop.clone();
        // Each task starts with a different base loan_id offset so IDs never collide.
        let id_base: u64 = (task_id as u64) * 10_000_000;

        let handle = tokio::spawn(async move {
            let mut committed: u64 = 0;
            let mut seq: u64 = 1; // Start from 1 — contracts require loanId > 0
            while !stop_flag.load(Ordering::Relaxed) {
                let loan_id = id_base + seq;
                seq += 1;
                let res: anyhow::Result<_> = if is_type_a {
                    client_clone.submit_type_a_with_limit(loan_id, &payload, block_limit).await
                } else {
                    client_clone.submit_type_b_with_limit(loan_id, &payload, block_limit).await
                };
                match res {
                    Ok(_) => committed += 1,
                    Err(e) => eprintln!("[task {}] submit error: {}", task_id, e),
                }
            }
            committed
        });
        join_handles.push(handle);
    }

    // Let the benchmark window run, then signal all tasks to stop.
    tokio::time::sleep(duration).await;
    stop.store(true, Ordering::Relaxed);

    let mut total_committed: u64 = 0;
    for handle in join_handles {
        total_committed += handle.await.unwrap_or(0);
    }

    let actual_duration = start_time.elapsed().as_secs_f64();
    Ok(TpsResult {
        total_committed,
        duration_secs: actual_duration,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Suppress noisy info logs — only show warnings and errors during the run
    // so the table output is clean. Override with RUST_LOG=debug if needed.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args = Args::parse();

    // Which lender counts to test
    let all_counts: Vec<usize> = if args.only_n > 0 {
        vec![args.only_n]
    } else {
        LENDER_COUNTS[..args.rows.min(LENDER_COUNTS.len())].to_vec()
    };

    let window = Duration::from_secs(args.duration);

    println!("\n=== Table II Reproduction — Our MPC (FISCO BCOS) ===");
    println!("  RPC : {}",
        LedgerConfig::load_default().map(|c| c.rpc_url).unwrap_or_else(|_| "ledger_config.json not found".to_string())
    );
    println!("  Window per test  : {}s", args.duration);
    let concurrent_mode = if args.concurrent > 0 { args.concurrent } else { 0 };
    if concurrent_mode > 0 {
        println!("  Concurrent tasks : {} (fixed — saturates the n-node PBFT network)", concurrent_mode);
    } else {
        println!("  Concurrent tasks : n (one per simulated lender — backward-compat mode)");
    }
    if args.batch_size > 1 {
        println!("  Checkpoint batch  : K={} (off-chain ordering + on-chain checkpoint, 132 B/tx)", args.batch_size);
        println!("  Effective record TPS = chain-TPS × K = chain-TPS × {}", args.batch_size);
    } else {
        println!("  Checkpoint batch  : K=1 (standard aggregated tx, 260 B/tx)");
    }
    println!();

    let cfg = LedgerConfig::load_default()
        .expect("Cannot load ledger_config.json — make sure you run from the 'Confidential Distributed Ledgers' directory");
    let client = Arc::new(LedgerClient::new(cfg));

    // Confirm the node is reachable before running long benchmarks.
    let block = client.get_block_number().await
        .expect("Cannot reach FISCO BCOS node — is it running? Check ledger_config.json");
    println!("  Node reachable — current block: {}\n", block);

    // Pre-fetch block limit once. The limit is valid for ~600 blocks (~10 min).
    // Refreshed before each benchmark row to stay within the valid window.
    let mut block_limit = block + 600;

    let mut rows: Vec<Row> = Vec::new();

    let total_tests = all_counts.len() * 2;
    let mut test_num = 0;

    for &n in &all_counts {
        // n_senders: if --concurrent given, use that; else fall back to n
        // (the paper uses high concurrency to always saturate the n-node PBFT network)
        let n_senders = if args.concurrent > 0 { args.concurrent } else { n };

        // Refresh block limit before each row in case the chain has advanced
        block_limit = client.get_block_number().await.unwrap_or(block) + 600;

        // ── Type 1 (loan recording) ─────────────────────────────────────────────
        // K=1  → standard aggregated tx (260 B, one record per chain tx).
        // K>1  → checkpoint tx (132 B, K records committed per chain tx);
        //         effective record TPS = chain-TPS × K.
        test_num += 1;
        let (type1_bytes, type1_size_b) = if args.batch_size > 1 {
            let b = Type1CheckpointTx::new_checkpoint_dummy(args.batch_size).to_bytes();
            let sz = b.len();
            (b, sz)
        } else {
            let b = Type1AggregatedTx::new_agg_dummy().to_bytes();
            let sz = b.len();
            (b, sz)
        };
        let type1_label = if args.batch_size > 1 {
            format!("checkpoint K={}", args.batch_size)
        } else {
            "agg".to_string()
        };
        println!("[{}/{}] Type 1 ({}), n={} lenders ({} B), {} senders — running {}s ...",
            test_num, total_tests, type1_label, n, type1_size_b, n_senders, args.duration);
        let r1 = run_benchmark(client.clone(), type1_bytes, window, n_senders, true, block_limit).await?;
        let type1_record_tps = r1.tps() * args.batch_size as f64;
        if args.batch_size > 1 {
            println!("       committed={} checkpoints, chain-TPS={:.1}, record-TPS={:.1} (×{})",
                r1.total_committed, r1.tps(), type1_record_tps, args.batch_size);
        } else {
            println!("       committed={}, TPS={:.1}", r1.total_committed, r1.tps());
        }

        // Refresh block limit again before Type 2
        block_limit = client.get_block_number().await.unwrap_or(block) + 600;

        // ── Type 2 (repayment) ──────────────────────────────────────────────
        test_num += 1;
        let type2_bytes = Type2RepaymentTx::new_dummy().to_bytes();
        let type2_size_b = type2_bytes.len();
        println!("[{}/{}] Type 2, n={} ({} B), {} senders — running {}s ...",
            test_num, total_tests, n, type2_size_b, n_senders, args.duration);
        let r2 = run_benchmark(client.clone(), type2_bytes, window, n_senders, false, block_limit).await?;
        println!("       committed={}, TPS={:.1}", r2.total_committed, r2.tps());

        rows.push(Row {
            n,
            type1_size_b,
            type1_chain_tps: r1.tps(),
            type1_record_tps,
            batch_size: args.batch_size,
            type2_size_b,
            type2_tps: r2.tps(),
        });
    }

    // ── Print Table II ──────────────────────────────────────────────────────
    let use_checkpoint = rows.first().map(|r| r.batch_size > 1).unwrap_or(false);
    println!();
    if use_checkpoint {
        println!("┌─────────────────────┬─────────────────────────────────────────┬─────────────────────────────┐");
        println!("│                     │  First type (checkpoint K={:<3})           │        Second type          │",
            rows.first().map(|r| r.batch_size).unwrap_or(1));
        println!("│  Lenders' number    ├──────────┬──────────────┬────────────────┼──────────────┬──────────────┤");
        println!("│                     │   Size   │  Chain TPS   │  Record TPS    │     Size     │     TPS      │");
        println!("├─────────────────────┼──────────┼──────────────┼────────────────┼──────────────┼──────────────┤");
        for row in &rows {
            let size1_str = format!("{} B", row.type1_size_b);
            let size2_str = format!("{} B", row.type2_size_b);
            println!("│  Our MPC  n={:<6}  │ {:>8} │ {:>12.0} │ {:>14.0} │ {:>12} │ {:>12.0} │",
                row.n, size1_str, row.type1_chain_tps, row.type1_record_tps, size2_str, row.type2_tps);
        }
        println!("└─────────────────────┴──────────┴──────────────┴────────────────┴──────────────┴──────────────┘");
    } else {
        println!("┌─────────────────────┬─────────────────────────────┬─────────────────────────────┐");
        println!("│                     │  First type (aggregated)    │        Second type          │");
        println!("│  Lenders' number    ├──────────────┬──────────────┼──────────────┬──────────────┤");
        println!("│                     │     Size     │     TPS      │     Size     │     TPS      │");
        println!("├─────────────────────┼──────────────┼──────────────┼──────────────┼──────────────┤");
        for row in &rows {
            let size1_str = format!("{} B", row.type1_size_b);
            let size2_str = format!("{} B", row.type2_size_b);
            println!("│  Our MPC  n={:<6}  │ {:>12} │ {:>12.0} │ {:>12} │ {:>12.0} │",
                row.n, size1_str, row.type1_chain_tps, size2_str, row.type2_tps);
        }
        println!("└─────────────────────┴──────────────┴──────────────┴──────────────┴──────────────┘");
    }
    println!();
    println!("Aggregated protocol: Type-1 tx is constant 260 B for all n (O(1) on-chain).");
    println!("  Breakdown: 32B meta + 64B sig + 4×33B agg ciphertexts + 32B Merkle root = 260 B");
    println!("  Checkpoint (K>1): 132 B on-chain (32B meta + 64B sig + 32B Merkle root + 4B K).");
    println!("  Effective record TPS = chain-TPS × K (off-chain records verified via Merkle root).");
    println!("  Original sizes (96+165n bytes): n=2→426B  n=4→756B  n=6→1086B  n=8→1416B  n=10→1746B");
    println!();
    println!("Original paper baseline (O(n) protocol, for comparison):");
    println!("  n=10 : Type1=1.70 KB / 397 TPS   Type2=96 B / 403 TPS");
    println!("  n=50 : Type1=8.15 KB / 274 TPS   Type2=96 B / 286 TPS");
    println!("  n=100: Type1=16.21 KB / 33 TPS   Type2=96 B / 39 TPS");
    println!();

    Ok(())
}
