use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Configuration for the Sui ledger client.
///
/// After publishing the Move package (`sui client publish`), fill in:
/// - `package_id` — the published package Object ID
/// - `ledger_object_id` — the shared `LoanLedger` object created by `init()`
/// - `ledger_initial_shared_version` — initial version from the publish output
///
/// Loaded from `sui_ledger_config.json` (same pattern as `ledger_config.json`
/// for the FISCO-BCOS client).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SuiLedgerConfig {
    /// Sui full-node JSON-RPC endpoint.
    /// Localnet default: `"http://127.0.0.1:9000"`
    pub rpc_url: String,

    /// Sui faucet endpoint (localnet only).
    /// Localnet default: `"http://127.0.0.1:9123/gas"`
    pub faucet_url: String,

    /// Published Move package Object ID (hex with 0x prefix).
    pub package_id: String,

    /// Shared `LoanLedger` object ID (hex with 0x prefix).
    pub ledger_object_id: String,

    /// `initialSharedVersion` of the `LoanLedger` object.
    /// Found in the `objectChanges` section of `sui client publish` output.
    pub ledger_initial_shared_version: u64,
}

impl SuiLedgerConfig {
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read config: {}", path))?;
        serde_json::from_str(&content).context("Failed to parse sui_ledger_config.json")
    }

    pub fn load_default() -> Result<Self> {
        Self::from_file("sui_ledger_config.json")
    }
}
