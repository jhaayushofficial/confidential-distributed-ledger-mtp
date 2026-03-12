//! Sui JSON-RPC client for the LoanLedger Move module.
//!
//! Mirrors `fisco_ledger::LedgerClient` but targets Sui's Mysticeti
//! (DAG-BFT) consensus.  Uses only lightweight crates (`reqwest`,
//! `ed25519-dalek`, `blake2`) — no heavy `sui-sdk` dependency.
//!
//! # Transaction flow
//!
//! 1. **Build** — delegates to the Sui node via `unsafe_moveCall`
//!    (server-side BCS construction).
//! 2. **Sign** — client-side Ed25519 over
//!    `blake2b_256([0x00,0x00,0x00] || tx_bytes)`.
//! 3. **Execute** — `sui_executeTransactionBlock` with
//!    `WaitForLocalExecution` for synchronous confirmation.

use anyhow::{anyhow, bail, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use blake2::{Blake2b, Digest, digest::consts::U32};
use ed25519_dalek::{SigningKey, Signer, VerifyingKey};
use rand::rngs::OsRng;
use serde_json::{json, Value};
use std::time::Duration;

use crate::config::SuiLedgerConfig;

// ═══════════════════════════════════════════════════════════════════════════
// Address derivation
// ═══════════════════════════════════════════════════════════════════════════

/// Derive a Sui address from an Ed25519 public key.
///
/// `address = 0x + hex( blake2b_256(0x00 || pubkey_32_bytes) )`
pub fn sui_address(pubkey: &VerifyingKey) -> String {
    let mut h = Blake2b::<U32>::new();
    h.update([0x00u8]); // Ed25519 scheme flag
    h.update(pubkey.as_bytes());
    let hash = h.finalize();
    format!("0x{}", hex::encode(hash))
}

/// Generate a fresh Ed25519 keypair → `(signing_key, sui_address)`.
pub fn generate_keypair() -> (SigningKey, String) {
    let sk = SigningKey::generate(&mut OsRng);
    let addr = sui_address(&VerifyingKey::from(&sk));
    (sk, addr)
}

// ═══════════════════════════════════════════════════════════════════════════
// Transaction signing
// ═══════════════════════════════════════════════════════════════════════════

/// Sign Sui `TransactionData` bytes (BCS-encoded, returned by
/// `unsafe_moveCall`).
///
/// Sui signing protocol:
/// 1. Prepend 3-byte intent prefix `[0x00, 0x00, 0x00]` to `tx_bytes`.
/// 2. `digest = blake2b_256(intent_prefix || tx_bytes)`.
/// 3. Ed25519-sign the 32-byte digest.
/// 4. Serialised signature: `[flag(0x00) | sig(64) | pubkey(32)]` → base64.
pub fn sign_tx_bytes(tx_bytes: &[u8], key: &SigningKey) -> String {
    // 1. Intent message: [scope=0, version=0, app_id=0] || tx_data
    let mut intent_msg = Vec::with_capacity(3 + tx_bytes.len());
    intent_msg.extend_from_slice(&[0x00, 0x00, 0x00]);
    intent_msg.extend_from_slice(tx_bytes);

    // 2. Blake2b-256 digest
    let digest: [u8; 32] = Blake2b::<U32>::digest(&intent_msg).into();

    // 3. Ed25519 sign the digest
    let sig = key.sign(&digest);

    // 4. Sui serialised signature format (97 bytes total)
    let pk = VerifyingKey::from(key);
    let mut out = Vec::with_capacity(97);
    out.push(0x00); // Ed25519 scheme flag
    out.extend_from_slice(&sig.to_bytes());
    out.extend_from_slice(pk.as_bytes());

    B64.encode(&out)
}

// ═══════════════════════════════════════════════════════════════════════════
// Client
// ═══════════════════════════════════════════════════════════════════════════

/// Result of a committed Sui transaction.
#[derive(Debug, Clone)]
pub struct SuiTxResult {
    pub digest: String,
    pub success: bool,
}

/// Lightweight Sui JSON-RPC client.
///
/// Equivalent to `fisco_ledger::LedgerClient` but targeting Sui nodes
/// running Mysticeti DAG-BFT consensus.
pub struct SuiLedgerClient {
    pub cfg: SuiLedgerConfig,
    http: reqwest::Client,
}

impl SuiLedgerClient {
    pub fn new(cfg: SuiLedgerConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(64) // keep connections warm for TPS benchmarks
            .build()
            .expect("Failed to build HTTP client");
        SuiLedgerClient { cfg, http }
    }

    // ── Low-level JSON-RPC ─────────────────────────────────────────────

    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "method":  method,
            "params":  params,
            "id":      1
        });

        let resp = self
            .http
            .post(&self.cfg.rpc_url)
            .json(&body)
            .send()
            .await?
            .json::<Value>()
            .await?;

        log::debug!("RPC {} → {}", method, resp);

        if let Some(err) = resp.get("error") {
            bail!("RPC error from {}: {}", method, err);
        }

        resp.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("RPC response missing 'result'"))
    }

    // ── Chain queries ──────────────────────────────────────────────────

    /// Latest checkpoint sequence number (analogous to FISCO block number).
    pub async fn get_latest_checkpoint(&self) -> Result<u64> {
        let r = self
            .rpc("sui_getLatestCheckpointSequenceNumber", json!([]))
            .await?;
        Ok(r.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| r.as_u64())
            .unwrap_or(0))
    }

    // ── Faucet ──────────────────────────────────────────────────────────

    /// Request SUI tokens from the faucet for `address`.
    /// Works with both localnet v1 and devnet/testnet v2 endpoints.
    pub async fn request_faucet(&self, address: &str) -> Result<()> {
        let body = json!({ "FixedAmountRequest": { "recipient": address } });

        // Retry up to 10 times on rate-limiting (HTTP 429).
        for attempt in 0..10 {
            let resp = self
                .http
                .post(&self.cfg.faucet_url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if resp.status() == 429 {
                let wait = (attempt + 1) * 5;
                log::warn!("Faucet rate-limited, waiting {}s (attempt {})", wait, attempt + 1);
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                // Treat 5xx as transient — retry after a delay
                if status.is_server_error() && attempt < 9 {
                    log::warn!("Faucet server error {}, retrying in 10s (attempt {})", status, attempt + 1);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
                bail!("Faucet error {}: {}", status, text);
            }

            // Wait for the faucet tx to commit.
            tokio::time::sleep(Duration::from_millis(800)).await;
            return Ok(());
        }

        bail!("Faucet rate-limited after 10 attempts for {}", address)
    }

    // ── Transaction building (server-side via unsafe_moveCall) ─────────

    /// Ask the Sui node to construct a Move call transaction.
    ///
    /// Returns the raw BCS-encoded `TransactionData` bytes which must
    /// be signed client-side before execution.
    async fn build_move_call(
        &self,
        sender: &str,
        function: &str,
        args: Vec<Value>,
        gas_budget: u64,
    ) -> Result<Vec<u8>> {
        let params = json!([
            sender,
            self.cfg.package_id,
            "ledger",
            function,
            [],                             // type_arguments (none)
            args,
            null,                           // gas coin: auto-select
            gas_budget.to_string()
        ]);

        let result = self.rpc("unsafe_moveCall", params).await?;
        let b64 = result
            .get("txBytes")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("unsafe_moveCall: missing txBytes"))?;
        B64.decode(b64).map_err(|e| anyhow!("base64 decode: {}", e))
    }

    /// Submit a signed transaction and wait for confirmation.
    ///
    /// - **Localnet** (127.0.0.1): uses `WaitForLocalExecution` so the
    ///   node updates object versions before the next tx is built.
    /// - **Remote** (devnet/testnet): uses `WaitForEffectsCert` which
    ///   returns after consensus without waiting for local execution,
    ///   hiding network latency.
    async fn execute_signed(
        &self,
        tx_bytes: &[u8],
        signature: &str,
    ) -> Result<SuiTxResult> {
        let is_local = self.cfg.rpc_url.contains("127.0.0.1")
            || self.cfg.rpc_url.contains("localhost");
        let confirm_mode = if is_local {
            "WaitForLocalExecution"
        } else {
            "WaitForEffectsCert"
        };
        let params = json!([
            B64.encode(tx_bytes),
            [signature],
            {
                "showEffects": true,
                "showEvents": false,
                "showInput": false,
                "showRawInput": false,
                "showObjectChanges": false,
                "showBalanceChanges": false
            },
            confirm_mode
        ]);

        let result = self
            .rpc("sui_executeTransactionBlock", params)
            .await?;

        let digest = result
            .get("digest")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let success = result
            .pointer("/effects/status/status")
            .and_then(|v| v.as_str())
            .map(|s| s == "success")
            .unwrap_or(false);

        Ok(SuiTxResult { digest, success })
    }

    // ── High-level record operations ───────────────────────────────────

    /// Build, sign, and submit `record_type_a(ledger, loan_id, data)`.
    ///
    /// Equivalent to `fisco_ledger::LedgerClient::submit_type_a`.
    pub async fn submit_type_a(
        &self,
        sender: &str,
        key: &SigningKey,
        loan_id: u64,
        payload: &[u8],
        gas_budget: u64,
    ) -> Result<SuiTxResult> {
        let args = vec![
            json!(self.cfg.ledger_object_id),   // &mut LoanLedger
            json!(loan_id.to_string()),          // u64
            json!(payload.to_vec()),             // vector<u8> — JSON array of u8
        ];
        let tx_bytes = self
            .build_move_call(sender, "record_type_a", args, gas_budget)
            .await?;
        let sig = sign_tx_bytes(&tx_bytes, key);
        self.execute_signed(&tx_bytes, &sig).await
    }

    /// Build, sign, and submit `record_type_b(ledger, loan_id, data)`.
    ///
    /// Equivalent to `fisco_ledger::LedgerClient::submit_type_b`.
    pub async fn submit_type_b(
        &self,
        sender: &str,
        key: &SigningKey,
        loan_id: u64,
        payload: &[u8],
        gas_budget: u64,
    ) -> Result<SuiTxResult> {
        let args = vec![
            json!(self.cfg.ledger_object_id),
            json!(loan_id.to_string()),
            json!(payload.to_vec()),
        ];
        let tx_bytes = self
            .build_move_call(sender, "record_type_b", args, gas_budget)
            .await?;
        let sig = sign_tx_bytes(&tx_bytes, key);
        self.execute_signed(&tx_bytes, &sig).await
    }
}
