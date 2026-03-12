/// `fisco_ledger` — Step 4 + Step 7 Rust Ledger Client
///
/// Public API:
///   ```rust,no_run
///   use fisco_ledger::{LedgerClient, LedgerConfig, verify_roundtrip};
///   # #[tokio::main] async fn main() -> anyhow::Result<()> {
///   let cfg = LedgerConfig::load_default()?;
///   let client = LedgerClient::new(cfg);
///
///   // Step 4: store
///   let receipt = client.submit_type_a(1, b"my payload").await?;
///   println!("LoanLedger tx: {} block: {}", receipt.tx_hash, receipt.block_number);
///
///   // Step 7: retrieve + verify
///   let retrieved = client.query_type_a(1).await?;
///   let result = verify_roundtrip(b"my payload", &retrieved);
///   assert!(result.payload_len_ok && result.hash_ok);
///   # Ok(()) }
///   ```

pub mod abi;
pub mod client;
pub mod config;
pub mod transaction;

// Re-export the most-used types at crate root
pub use client::{LedgerClient, TxReceipt};
pub use config::LedgerConfig;
pub use transaction::TxSigner;

use anyhow::Result;
use sha2::{Digest, Sha256};

// ──────────────────────────────────────────────────────────────────────────────
// Step 7: roundtrip verification
// ──────────────────────────────────────────────────────────────────────────────

/// Result of a roundtrip verification between the originally submitted
/// payload and the payload retrieved from the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyResult {
    /// True when `retrieved.len() == expected.len()`
    pub payload_len_ok: bool,
    /// True when SHA-256(retrieved) == SHA-256(expected)
    pub hash_ok: bool,
}

impl VerifyResult {
    /// Returns true only when both length and hash checks pass.
    pub fn is_ok(&self) -> bool {
        self.payload_len_ok && self.hash_ok
    }
}

/// Compare the `expected` payload against what was `retrieved` from chain.
///
/// Checks:
/// 1. Payload byte-length equality
/// 2. SHA-256 hash equality (constant-time equivalent via compare)
///
/// # Example
/// ```rust
/// use fisco_ledger::verify_roundtrip;
/// let original = b"hello loan data";
/// let result = verify_roundtrip(original, original);
/// assert!(result.is_ok());
/// ```
pub fn verify_roundtrip(expected: &[u8], retrieved: &[u8]) -> VerifyResult {
    let payload_len_ok = expected.len() == retrieved.len();

    let expected_hash  = Sha256::digest(expected);
    let retrieved_hash = Sha256::digest(retrieved);
    let hash_ok = expected_hash.as_slice() == retrieved_hash.as_slice();

    VerifyResult { payload_len_ok, hash_ok }
}

impl LedgerClient {
    // ──────────────────────────────────────────────────────────────────────────
    // Public Step-4 / Step-7 API
    // ──────────────────────────────────────────────────────────────────────────

    /// Record a loan data payload on-chain by calling
    /// `LoanLedger.recordTypeA(loanId, data)`.
    pub async fn submit_type_a(&self, loan_id: u64, data: &[u8]) -> Result<TxReceipt> {
        let input = abi::encode_record_type_a(loan_id, data);
        let to = &self.cfg.loan_ledger_address.clone();
        self.send_and_wait(to, &input).await
    }

    /// Like `submit_type_a` but with pre-fetched block_limit (avoids extra RPC call).
    pub async fn submit_type_a_with_limit(&self, loan_id: u64, data: &[u8], block_limit: i64) -> Result<TxReceipt> {
        let input = abi::encode_record_type_a(loan_id, data);
        let to = &self.cfg.loan_ledger_address.clone();
        self.send_and_wait_with_limit(to, &input, block_limit).await
    }

    /// Record a repayment data payload on-chain by calling
    /// `RepaymentLedger.recordTypeB(loanId, data)`.
    pub async fn submit_type_b(&self, loan_id: u64, data: &[u8]) -> Result<TxReceipt> {
        let input = abi::encode_record_type_b(loan_id, data);
        let to = &self.cfg.repayment_ledger_address.clone();
        self.send_and_wait(to, &input).await
    }

    /// Like `submit_type_b` but with pre-fetched block_limit (avoids extra RPC call).
    pub async fn submit_type_b_with_limit(&self, loan_id: u64, data: &[u8], block_limit: i64) -> Result<TxReceipt> {
        let input = abi::encode_record_type_b(loan_id, data);
        let to = &self.cfg.repayment_ledger_address.clone();
        self.send_and_wait_with_limit(to, &input, block_limit).await
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Unit tests (no chain required)
// ──────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn verify_roundtrip_identical() {
        let payload = b"exactly-matching-bytes";
        let r = verify_roundtrip(payload, payload);
        assert!(r.payload_len_ok);
        assert!(r.hash_ok);
        assert!(r.is_ok());
    }

    #[test]
    fn verify_roundtrip_length_mismatch() {
        let r = verify_roundtrip(b"hello", b"hell");
        assert!(!r.payload_len_ok);
        assert!(!r.hash_ok);
        assert!(!r.is_ok());
    }

    #[test]
    fn verify_roundtrip_same_len_different_content() {
        let r = verify_roundtrip(b"aaaa", b"bbbb");
        assert!(r.payload_len_ok);  // same length
        assert!(!r.hash_ok);        // different content
        assert!(!r.is_ok());
    }

    #[test]
    fn verify_roundtrip_empty_payload() {
        let r = verify_roundtrip(b"", b"");
        assert!(r.payload_len_ok);
        assert!(r.hash_ok);
        assert!(r.is_ok());
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Integration tests (require running FISCO BCOS nodes)
// Run with: cargo test -p fisco_ledger -- --nocapture --ignored
// ──────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod integration {
    use super::*;

    fn make_client() -> LedgerClient {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/ayush".to_string());
        let cfg = LedgerConfig {
            rpc_url: "https://127.0.0.1:20200".into(),
            group_id: "group0".into(),
            chain_id: "chain0".into(),
            loan_ledger_address:
                "0x6849f21d1e455e9f0712b1e99fa4fcd23758e8f1".into(),
            repayment_ledger_address:
                "0x4721d1a77e0e76851d460073e64ea06d9c104194".into(),
            account_address:
                "0x8f29d04cf7ea2df99b99ca8f5d823f939b94eb98".into(),
            ca_cert:  format!("{}/nodes/127.0.0.1/sdk/ca.crt", home),
            sdk_cert: format!("{}/nodes/127.0.0.1/sdk/sdk.crt", home),
            sdk_key:  format!("{}/nodes/127.0.0.1/sdk/sdk.key", home),
            private_key_pem: "/mnt/c/MTP/Distributed-Ledgers/console/account/ecdsa/0x8f29d04cf7ea2df99b99ca8f5d823f939b94eb98.pem".into(),
        };
        LedgerClient::new(cfg)
    }

    #[tokio::test]
    #[ignore = "requires running FISCO BCOS nodes"]
    async fn test_submit_type_a() {
        let client = make_client();
        let receipt = client
            .submit_type_a(9001, b"hello-loan")
            .await
            .expect("submit_type_a failed");
        println!("Type A receipt: {:?}", receipt);
        assert!(!receipt.tx_hash.is_empty());
        assert!(receipt.block_number > 0);
    }

    #[tokio::test]
    #[ignore = "requires running FISCO BCOS nodes"]
    async fn test_submit_type_b() {
        let client = make_client();
        let receipt = client
            .submit_type_b(9001, b"hello-repayment")
            .await
            .expect("submit_type_b failed");
        println!("Type B receipt: {:?}", receipt);
        assert!(!receipt.tx_hash.is_empty());
        assert!(receipt.block_number > 0);
    }

    // ── Step 7: roundtrip tests ───────────────────────────────────────────────

    #[tokio::test]
    #[ignore = "requires running FISCO BCOS nodes"]
    async fn test_query_type_a_roundtrip() {
        let client = make_client();
        let loan_id  = 9100_u64;
        let payload  = b"roundtrip-type-a-payload";

        // 1. Store
        let receipt = client
            .submit_type_a(loan_id, payload)
            .await
            .expect("submit_type_a failed");
        println!("Type A stored — tx: {}  block: {}", receipt.tx_hash, receipt.block_number);

        // 2. Retrieve
        let retrieved = client
            .query_type_a(loan_id)
            .await
            .expect("query_type_a failed");
        println!("Type A retrieved {} bytes: {:?}", retrieved.len(), retrieved);

        // 3. Verify
        let result = verify_roundtrip(payload, &retrieved);
        println!(
            "Type A verify — payload_len_ok={} hash_ok={}",
            result.payload_len_ok, result.hash_ok
        );
        assert!(result.payload_len_ok, "payload length mismatch");
        assert!(result.hash_ok,        "payload hash mismatch");
    }

    #[tokio::test]
    #[ignore = "requires running FISCO BCOS nodes"]
    async fn test_query_type_b_roundtrip() {
        let client = make_client();
        let loan_id  = 9200_u64;
        let payload  = b"roundtrip-type-b-payload";

        // 1. Store
        let receipt = client
            .submit_type_b(loan_id, payload)
            .await
            .expect("submit_type_b failed");
        println!("Type B stored — tx: {}  block: {}", receipt.tx_hash, receipt.block_number);

        // 2. Retrieve
        let retrieved = client
            .query_type_b(loan_id)
            .await
            .expect("query_type_b failed");
        println!("Type B retrieved {} bytes: {:?}", retrieved.len(), retrieved);

        // 3. Verify
        let result = verify_roundtrip(payload, &retrieved);
        println!(
            "Type B verify — payload_len_ok={} hash_ok={}",
            result.payload_len_ok, result.hash_ok
        );
        assert!(result.payload_len_ok, "payload length mismatch");
        assert!(result.hash_ok,        "payload hash mismatch");
    }
}
