use serde::{Deserialize, Serialize};
use std::fs;
use anyhow::{Context, Result};

/// Configuration loaded from `ledger_config.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerConfig {
    /// FISCO BCOS JSON-RPC endpoint — use "https://..." for TLS nodes
    pub rpc_url: String,

    /// Group ID, e.g. "group0"
    pub group_id: String,

    /// Chain ID, e.g. "chain0"
    pub chain_id: String,

    /// Deployed LoanLedger contract address (Type A)
    pub loan_ledger_address: String,

    /// Deployed RepaymentLedger contract address (Type B)
    pub repayment_ledger_address: String,

    /// The account address used to send transactions.
    pub account_address: String,

    /// Path to CA cert PEM (from ~/nodes/127.0.0.1/sdk/ca.crt)
    pub ca_cert: String,

    /// Path to SDK client certificate PEM (sdk.crt)
    pub sdk_cert: String,

    /// Path to SDK client private key PEM (sdk.key)
    pub sdk_key: String,

    /// Path to the ECDSA private key PEM used to sign transactions.
    /// This is the account file from console/account/ecdsa/<address>.pem
    pub private_key_pem: String,
}

impl LedgerConfig {
    /// Load config from a JSON file at `path`.
    pub fn from_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Cannot read ledger config: {}", path))?;
        let cfg: LedgerConfig = serde_json::from_str(&content)
            .context("Failed to parse ledger_config.json")?;
        Ok(cfg)
    }

    /// Load from the default path `./ledger_config.json`.
    pub fn load_default() -> Result<Self> {
        Self::from_file("ledger_config.json")
    }
}
