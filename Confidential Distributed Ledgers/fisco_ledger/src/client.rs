/// FISCO BCOS JSON-RPC client with mutual TLS + transaction signing.
///
/// Uses `sendRawTransaction` with a protobuf-encoded, secp256k1-signed
/// transaction — matching FISCO BCOS 3.x (AIR mode) requirements.

use anyhow::{anyhow, bail, Result};
use reqwest::{Certificate, Identity};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, time::Duration};

use crate::config::LedgerConfig;
use crate::transaction::TxSigner;

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Decode the ABI-encoded `Error(string)` revert reason from transaction output.
/// The format is: `0x08c379a0` ++ ABI-encoded string.
fn decode_revert_reason(output_hex: &str) -> String {
    let hex = output_hex.trim_start_matches("0x");
    if hex.len() < 8 { return format!("(raw output: {})", output_hex); }
    // Skip the 4-byte selector 08c379a0, then ABI-decode the string
    let data = match hex::decode(&hex[8..]) {
        Ok(d) => d,
        Err(_) => return format!("(hex decode failed: {})", hex),
    };
    // ABI string: offset (32 bytes) + length (32 bytes) + data
    if data.len() >= 64 {
        let str_len = usize::from_be_bytes({
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&data[56..64]);
            arr
        });
        if data.len() >= 64 + str_len {
            return String::from_utf8_lossy(&data[64..64 + str_len]).to_string();
        }
    }
    format!("(output: {})", output_hex)
}

// ──────────────────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxReceipt {
    pub tx_hash: String,
    pub block_number: u64,
    pub gas_used: String,
}

// ──────────────────────────────────────────────────────────────────────────────
// Client
// ──────────────────────────────────────────────────────────────────────────────

pub struct LedgerClient {
    pub cfg: LedgerConfig,
    http: reqwest::Client,
    signer: TxSigner,
}

impl LedgerClient {
    pub fn new(cfg: LedgerConfig) -> Self {
        // ── TLS setup ───────────────────────────────────────────────────────
        let ca_bytes   = fs::read(&cfg.ca_cert)
            .unwrap_or_else(|e| panic!("Cannot read ca_cert {}: {}", cfg.ca_cert, e));
        let cert_bytes = fs::read(&cfg.sdk_cert)
            .unwrap_or_else(|e| panic!("Cannot read sdk_cert {}: {}", cfg.sdk_cert, e));
        let key_bytes  = fs::read(&cfg.sdk_key)
            .unwrap_or_else(|e| panic!("Cannot read sdk_key {}: {}", cfg.sdk_key, e));

        let ca_cert = Certificate::from_pem(&ca_bytes)
            .expect("Failed to parse ca.crt");
        let identity = Identity::from_pkcs8_pem(&cert_bytes, &key_bytes)
            .expect("Failed to build TLS identity from sdk.crt + sdk.key");

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(true)
            .add_root_certificate(ca_cert)
            .identity(identity)
            .build()
            .expect("Failed to build HTTPS client");

        // ── Transaction signer ──────────────────────────────────────────────
        let signer = TxSigner::from_pem_file(&cfg.private_key_pem)
            .unwrap_or_else(|e| panic!("Cannot load signing key {}: {}", cfg.private_key_pem, e));

        LedgerClient { cfg, http, signer }
    }

    // ── Low-level JSON-RPC ───────────────────────────────────────────────────

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

        log::debug!("RPC {} raw response: {}", method, resp);

        if let Some(err) = resp.get("error") {
            bail!("RPC error from {}: {}", method, err);
        }

        resp.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("RPC response missing 'result' field"))
    }

    // ── Block number for block limit ─────────────────────────────────────────

    pub async fn get_block_number(&self) -> Result<i64> {
        let result = self.rpc("getBlockNumber", json!([self.cfg.group_id])).await?;
        // FISCO-BCOS 3.x returns either an integer or a hex string
        let n = if let Some(v) = result.as_i64() {
            v
        } else if let Some(s) = result.as_str() {
            i64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0)
        } else {
            0
        };
        Ok(n)
    }

    // ── Transaction sending ──────────────────────────────────────────────────

    /// Parse a `blockNumber` field that FISCO-BCOS 3.x returns as either
    /// an integer (`357`) or a hex string (`"0x165"`).
    fn parse_block_number(v: &Value) -> u64 {
        if let Some(n) = v.as_u64() {
            return n;
        }
        if let Some(s) = v.as_str() {
            return u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0);
        }
        0
    }

    /// Parse a `status` field that FISCO-BCOS 3.x returns as either
    /// an integer (`0`, `19`) or a hex string (`"0x0"`, `"0x13"`).
    fn parse_status(v: &Value) -> u64 {
        if let Some(n) = v.as_u64() {
            return n;
        }
        if let Some(s) = v.as_str() {
            return u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0);
        }
        0
    }

    /// Parse the inline receipt returned synchronously by `sendTransaction`.
    ///
    /// FISCO-BCOS 3.x executes transactions synchronously and embeds the full
    /// receipt in the `sendTransaction` JSON-RPC result object.
    fn parse_inline_receipt(result: &Value, tx_hash: &str) -> Result<TxReceipt> {
        // blockNumber is an integer in FISCO-BCOS 3.x
        let block_number = result.get("blockNumber")
            .map(Self::parse_block_number)
            .unwrap_or(0);

        // gasUsed may be `"0"` (string) or an integer
        let gas_used = result.get("gasUsed")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_else(|| "0".to_string());

        // status is an integer: 0 = success, non-zero = revert/error
        let status = result.get("status")
            .map(Self::parse_status)
            .unwrap_or(0);

        if status != 0 {
            // Decode ABI-encoded Error(string) from output if present
            let output_hex = result.get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let err_msg = decode_revert_reason(output_hex);
            bail!(
                "Transaction reverted (status={}): {} [tx={}]",
                status, err_msg, tx_hash
            );
        }

        Ok(TxReceipt { tx_hash: tx_hash.to_string(), block_number, gas_used })
    }

    /// Build, sign, and send a transaction to `to` with ABI-encoded `input`.
    /// FISCO-BCOS 3.x executes synchronously — the receipt is embedded in the
    /// `sendTransaction` response directly (no polling needed).
    ///
    /// Pass `to = ""` for a contract deployment transaction.
    pub async fn send_and_wait(&self, to: &str, input: &[u8]) -> Result<TxReceipt> {
        // Get current block number for block_limit
        let block_number = self.get_block_number().await?;
        self.send_and_wait_with_limit(to, input, block_number + 600).await
    }

    /// Same as `send_and_wait` but with a pre-supplied block_limit.
    /// Use this for high-throughput benchmarking where a fresh `getBlockNumber`
    /// call per transaction would double the RPC overhead.
    ///
    /// The block_limit only needs to be refreshed every ~600 blocks (~10 min at
    /// 1 block/s), so callers can fetch once and reuse for the whole bench run.
    pub async fn send_and_wait_with_limit(
        &self,
        to: &str,
        input: &[u8],
        block_limit: i64,
    ) -> Result<TxReceipt> {
        // Build + sign the transaction
        let raw_tx = self.signer.sign_tx(
            &self.cfg.chain_id,
            &self.cfg.group_id,
            block_limit,
            to,
            input,
        )?;

        // sendTransaction params: [groupID, node, signedTxHex, requireProof(bool)]
        let params = json!([self.cfg.group_id, "", raw_tx, false]);
        let result = self.rpc("sendTransaction", params).await?;

        // In FISCO-BCOS 3.x, result is always the inline receipt object
        let tx_hash = result.get("transactionHash")
            .and_then(|v| v.as_str())
            .unwrap_or("0x")
            .to_string();

        log::debug!("sent tx {} (limit {})", tx_hash, block_limit);
        Self::parse_inline_receipt(&result, &tx_hash)
    }

    /// Deploy a compiled contract (hex-encoded bytecode).
    /// Returns `(tx_hash, contract_address)`.
    pub async fn deploy_contract(&self, bytecode_hex: &str) -> Result<(String, String)> {
        let bytecode = hex::decode(bytecode_hex.trim())
            .map_err(|e| anyhow::anyhow!("Invalid bytecode hex: {}", e))?;
        let receipt = self.send_and_wait("", &bytecode).await?;
        Ok((receipt.tx_hash, receipt.gas_used)) // gas_used field repurposed temporarily
    }

    /// Deploy a contract and return the deployed contract address.
    pub async fn deploy_and_get_address(&self, bytecode_hex: &str) -> Result<String> {
        let block_number = self.get_block_number().await?;
        let block_limit = block_number + 600;

        let bytecode = hex::decode(bytecode_hex.trim())
            .map_err(|e| anyhow::anyhow!("Invalid bytecode hex: {}", e))?;

        let raw_tx = self.signer.sign_tx(
            &self.cfg.chain_id,
            &self.cfg.group_id,
            block_limit,
            "",
            &bytecode,
        )?;

        let params = json!([self.cfg.group_id, "", raw_tx, false]);
        let result = self.rpc("sendTransaction", params).await?;

        let tx_hash = result.get("transactionHash")
            .and_then(|v| v.as_str())
            .unwrap_or("0x")
            .to_string();

        let status = result.get("status")
            .map(Self::parse_status)
            .unwrap_or(0);

        if status != 0 {
            let output_hex = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
            bail!("Deploy failed (status={}): {}", status, decode_revert_reason(output_hex));
        }

        let addr = result.get("contractAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        log::info!("Deployed contract: {} (tx={})", addr, tx_hash);
        Ok(addr)
    }

    /// Legacy: kept for compat — just calls send_and_wait
    #[allow(dead_code)]
    pub async fn send_transaction(&self, to: &str, input: &[u8]) -> Result<String> {
        let receipt = self.send_and_wait(to, input).await?;
        Ok(receipt.tx_hash)
    }

    // ── Read-only eth_call ───────────────────────────────────────────────────

    /// Issue a read-only `call` (no signing) to `to` with ABI-encoded `input`.
    ///
    /// FISCO BCOS 3.x JSON-RPC: `call(groupId, {from, to, data})`
    /// Returns the raw decoded bytes of the `output` field.
    pub async fn call(&self, to: &str, input: &[u8]) -> Result<Vec<u8>> {
        let data = format!("0x{}", hex::encode(input));
        let params = serde_json::json!([
            self.cfg.group_id,
            {
                "from": self.cfg.account_address,
                "to":   to,
                "data": data
            }
        ]);
        let result = self.rpc("call", params).await?;

        // FISCO BCOS returns { output: "0x...", ... }
        let output_hex = result
            .get("output")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("call: missing 'output' field in response: {}", result))?;

        let stripped = output_hex.trim_start_matches("0x");
        let raw = hex::decode(stripped)
            .map_err(|e| anyhow::anyhow!("call: hex decode failed: {}", e))?;
        Ok(raw)
    }

    /// Query the stored Type A payload for `loan_id` from `LoanLedger.getTypeA(loanId)`.
    pub async fn query_type_a(&self, loan_id: u64) -> Result<Vec<u8>> {
        let input = crate::abi::encode_get_type_a(loan_id);
        let raw = self.call(&self.cfg.loan_ledger_address.clone(), &input).await?;
        crate::abi::decode_bytes_return(&raw)
    }

    /// Query the stored Type B payload for `loan_id` from `RepaymentLedger.getTypeB(loanId)`.
    pub async fn query_type_b(&self, loan_id: u64) -> Result<Vec<u8>> {
        let input = crate::abi::encode_get_type_b(loan_id);
        let raw = self.call(&self.cfg.repayment_ledger_address.clone(), &input).await?;
        crate::abi::decode_bytes_return(&raw)
    }
}
