# Confidential Distributed Ledgers

Privacy-preserving distributed ledger for syndicated lending using homomorphic encryption, comparing PBFT (FISCO-BCOS) and DAG-BFT (Sui) consensus mechanisms.

## Introduction

We propose a new collaborative financial ledger for online syndicated lending. It leverages homomorphic encryption/commitment to allow the reuse of intermediary transactional states without breaking the privacy promise during the full lifecycle of a loan. This system also supports efficient regulation-compliant auditing.

## Project Structure

```
Confidential Distributed Ledgers/   # Main implementation
├── node/                            # Validator/peer nodes
├── regulator/                       # Regulatory auditing entity
├── intergration_test/               # End-to-end integration tests (DKG + ledger)
├── message/                         # Shared cryptographic message types
├── elgamal/                         # ElGamal homomorphic encryption
├── bulletproof/                     # Zero-knowledge range proofs
├── fisco_ledger/                    # PBFT backend (FISCO-BCOS)
├── sui_ledger/                      # DAG-BFT backend (Sui Mysticeti)
└── third_party/curv/                # Elliptic curve crypto dependency

Comparison Scheme/                   # Baseline Pedersen-VSS comparison
```

## Prerequisites

- [Rust](https://www.rust-lang.org/) (stable toolchain)
- For FISCO-BCOS benchmarks: a running FISCO-BCOS network with TLS certificates
- For Sui benchmarks: [Sui CLI](https://docs.sui.io/build/install) installed

## Build

```bash
cd "Confidential Distributed Ledgers"
cargo build --release
```

## Usage — DKG and Distributed Ledger

Configure node addresses. If executing locally, no modifications are needed. Otherwise, modify the address in each config file:

```
./intergration_test/src/regulator/config/config_file/reg_config.json   # regulator
./intergration_test/src/node/node*/config/config_file/node_config.json # nodes
```

### Step 1: Distributed Key Generation (DKG)

```bash
# Start regulator
cargo test --package intergration_test --lib -- regulator::regulator::test --exact --show-output

# Start nodes (run each in a separate terminal)
cargo test --package intergration_test --lib -- node::node1::node1::test --exact --show-output
cargo test --package intergration_test --lib -- node::node2::node2::test --exact --show-output
cargo test --package intergration_test --lib -- node::node3::node3::test --exact --show-output
cargo test --package intergration_test --lib -- node::node4::node4::test --exact --show-output
```

The generated key pair will be written to `./node/node*/keypair.txt`.

### Step 2: Start Distributed Ledger

```bash
# Start regulator
cargo test --package intergration_test --lib -- regulator::regulator::decrypt_test --exact --show-output

# Start nodes (run each in a separate terminal)
cargo test --package intergration_test --lib -- node::node1::node1::test_decrypt --exact --show-output
cargo test --package intergration_test --lib -- node::node2::node2::test_decrypt --exact --show-output
cargo test --package intergration_test --lib -- node::node3::node3::test_decrypt --exact --show-output
cargo test --package intergration_test --lib -- node::node4::node4::test_decrypt --exact --show-output
```

Results are written to `./node/node*/log/node.log`.

---

## TPS Benchmarking

The project includes TPS (Transactions Per Second) benchmarks for both FISCO-BCOS (PBFT) and Sui (DAG-BFT) backends. Each benchmark tests two transaction types across three lender counts (n=10, 50, 100):

| Transaction Type | Description | Size |
|------------------|-------------|------|
| **Type 1 — Loan Recording (aggregated)** | Encrypted syndicated loan commitment | 260 B (constant, O(1)) |
| **Type 1 — Loan Recording (checkpoint K)** | Batched checkpoint of K records | 132 B (constant) |
| **Type 1 — Loan Recording (original)** | Naive O(n) format (Sui only) | 96 + 165n B |
| **Type 2 — Repayment** | Fixed-size repayment record | 96 B |

### A. FISCO-BCOS (PBFT) TPS Benchmark

#### 1. Set up FISCO-BCOS network

Build and start a FISCO-BCOS network (e.g., using `build_chain.sh`). Ensure you have:
- Running FISCO-BCOS nodes
- TLS certificates (`ca.crt`, `sdk.crt`, `sdk.key`)
- A funded ECDSA account

#### 2. Deploy smart contracts

```bash
cd "Confidential Distributed Ledgers"
cargo build --release --bin deploy
./target/release/deploy
```

This deploys `LoanLedger` and `RepaymentLedger` contracts and prints their addresses.

#### 3. Update configuration

Edit `ledger_config.json` with your network details:

```json
{
  "rpc_url": "https://127.0.0.1:20200",
  "group_id": "group0",
  "chain_id": "chain0",
  "loan_ledger_address": "<deployed LoanLedger address>",
  "repayment_ledger_address": "<deployed RepaymentLedger address>",
  "account_address": "<your account address>",
  "ca_cert": "/path/to/sdk/ca.crt",
  "sdk_cert": "/path/to/sdk/sdk.crt",
  "sdk_key": "/path/to/sdk/sdk.key",
  "private_key_pem": "/path/to/account.pem"
}
```

#### 4. Run the benchmark

```bash
cargo build --release --bin tps_bench

# Quick test (n=10 only, 15 seconds)
./target/release/tps_bench --duration 15 --only-n 10 --concurrent 64

# Full benchmark (all lender counts, 30s each)
./target/release/tps_bench --duration 30 --concurrent 200

# Checkpoint protocol (K=10)
./target/release/tps_bench --duration 30 --concurrent 200 --batch-size 10
```

### B. Sui (DAG-BFT / Mysticeti) TPS Benchmark

#### 1. Start Sui network

**Option A — Local multi-validator network:**
```bash
# Generate genesis with N validators
sui genesis --working-dir ~/sui-work --committee-size 50

# Start the network
sui start --network.config ~/sui-work/network.yaml &> ~/sui.log &
sleep 90  # allow validators to warm up
```

**Option B — Devnet:**
No setup needed; connect to `https://fullnode.devnet.sui.io:443`.

#### 2. Publish the Move contract

```bash
sui client publish "Confidential Distributed Ledgers/sui_ledger/move/loan_ledger"
```

Note the `package_id` and `ledger_object_id` from the output.

#### 3. Update configuration

Edit `sui_ledger_config.json`:

```json
{
  "rpc_url": "http://127.0.0.1:9000",
  "faucet_url": "http://127.0.0.1:9123/gas",
  "package_id": "<published package ID>",
  "ledger_object_id": "<created LoanLedger object ID>",
  "ledger_initial_shared_version": <initial shared version>
}
```

For devnet, use:
```json
{
  "rpc_url": "https://fullnode.devnet.sui.io:443",
  "faucet_url": "https://faucet.devnet.sui.io/v2/gas"
}
```

#### 4. Run the benchmark

```bash
cargo build --release --bin sui_tps_bench

# Aggregated 260 B transactions (default)
./target/release/sui_tps_bench --duration 30 --concurrent 32

# Original O(n) protocol baseline
./target/release/sui_tps_bench --duration 30 --concurrent 32 --original

# Checkpoint protocol (K=10)
./target/release/sui_tps_bench --duration 30 --concurrent 32 --batch-size 10
```

### CLI Parameters

| Parameter | Default (FISCO / Sui) | Description |
|-----------|-----------------------|-------------|
| `--duration` | 30 / 30 | Seconds per benchmark window |
| `--concurrent` | 0* / 32 | Parallel sender tasks (*0 = use n) |
| `--only-n` | 0 | Test specific lender count only (0 = all) |
| `--rows` | 3 | Number of lender counts to test (first N of [10, 50, 100]) |
| `--batch-size` | 1 | Checkpoint batch K (K=1 → 260 B, K>1 → 132 B) |
| `--original` | false (Sui only) | Use O(n) transaction format instead of O(1) aggregated |

### Sample Output

```
=== Table II Reproduction — Our MPC (FISCO BCOS) ===
  RPC : https://127.0.0.1:20200
  Window per test  : 30s
  Concurrent tasks : 200

[1/6] Type 1 (agg), n=10 lenders (260 B), 200 senders — running 30s ...
       committed=11910, TPS=397.0
[2/6] Type 2, n=10 (96 B), 200 senders — running 30s ...
       committed=12090, TPS=403.0
...

┌─────────────────┬────────────────────────┬──────────────────┐
│                 │  Type 1 (aggregated)   │     Type 2       │
│  Lenders (n)    ├──────────┬─────────────┼────────┬─────────┤
│                 │  Size    │  TPS        │  Size  │  TPS    │
├─────────────────┼──────────┼─────────────┼────────┼─────────┤
│  n=10           │  260 B   │  397.0      │  96 B  │  403.0  │
│  n=50           │  260 B   │  274.0      │  96 B  │  286.0  │
│  n=100          │  260 B   │   33.0      │  96 B  │   39.0  │
└─────────────────┴──────────┴─────────────┴────────┴─────────┘
```

---

## Comparison Experiment

The comparison experiment uses the Pedersen-VSS scheme as a baseline. The code is in the `Comparison Scheme/` directory. The setup and run procedure is the same as Confidential Distributed Ledgers above.
