//! `sui_ledger` — Sui (Mysticeti DAG-BFT) ledger client.
//!
//! Drop-in replacement for `fisco_ledger`, targeting a Sui localnet
//! instead of FISCO-BCOS.  All transaction types (`Type1AggregatedTx`,
//! `Type1CheckpointTx`, `Type2RepaymentTx`) come from the shared
//! `message` crate — only the blockchain transport layer changes.
//!
//! ## Quick start
//!
//! ```bash
//! # 1. Install Sui CLI
//! cargo install --locked sui
//!
//! # 2. Start localnet with faucet
//! sui start --with-faucet --force-regenesis
//!
//! # 3. Publish the Move package
//! cd sui_ledger/move/loan_ledger
//! sui client publish --gas-budget 100000000
//! #    → note package ID + LoanLedger shared object ID + initialSharedVersion
//!
//! # 4. Fill in sui_ledger_config.json (see example in repo root)
//!
//! # 5. Run benchmark
//! cd ../..   # back to "Confidential Distributed Ledgers/"
//! cargo run --manifest-path sui_ledger/Cargo.toml --bin sui_tps_bench \
//!       -- --duration 30 --concurrent 32
//! ```

pub mod client;
pub mod config;

pub use client::SuiLedgerClient;
pub use config::SuiLedgerConfig;
