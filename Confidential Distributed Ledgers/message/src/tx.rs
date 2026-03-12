use serde::{Deserialize, Serialize};
use crate::merkle::MerkleProof;

/// Fixed-length loan metadata recorded in a Type-1 transaction header.
/// For size calculations we just treat this as 32 opaque bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoanMetadata {
    pub bytes: [u8; 32],
}

/// ECDSA(secp256k1) signature in compact 64-byte form (r || s).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EcdsaSignature64 {
    #[serde(with = "serde_arrays")]
    pub bytes: [u8; 64],
}

/// One lender's contribution to a Type-1 transaction (our MPC).
///
/// All fields are compressed curve points (33 bytes each).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OurMpcLenderEntry {
    #[serde(with = "serde_arrays")]
    pub group_c1: [u8; 33],
    #[serde(with = "serde_arrays")]
    pub group_c2: [u8; 33],
    #[serde(with = "serde_arrays")]
    pub regulator_c1: [u8; 33],
    #[serde(with = "serde_arrays")]
    pub regulator_c2: [u8; 33],
    #[serde(with = "serde_arrays")]
    pub partial_dec_share: [u8; 33],
}

/// Type-1 transaction for "Our MPC" scheme (loan request).
///
/// This matches the paper's counting: 32-byte metadata, 64-byte signature,
/// and for each of n lenders, 5 compressed points (5 * 33 bytes).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Type1OurMpcTx {
    pub meta: LoanMetadata,
    pub sig: EcdsaSignature64,
    pub lenders: Vec<OurMpcLenderEntry>,
}

impl Type1OurMpcTx {
    /// Construct a dummy transaction with the right layout for a given n.
    /// All bytes are zero; this is only for size calculations.
    pub fn new_with_lenders(n: usize) -> Self {
        let meta = LoanMetadata { bytes: [0u8; 32] };
        let sig = EcdsaSignature64 { bytes: [0u8; 64] };
        let zero_entry = OurMpcLenderEntry {
            group_c1: [0u8; 33],
            group_c2: [0u8; 33],
            regulator_c1: [0u8; 33],
            regulator_c2: [0u8; 33],
            partial_dec_share: [0u8; 33],
        };
        let lenders = std::iter::repeat(zero_entry).take(n).collect();
        Type1OurMpcTx { meta, sig, lenders }
    }

    /// Serialize to a compact binary layout and return its size in bytes.
    ///
    /// Layout:
    /// - 32 bytes: metadata
    /// - 64 bytes: ECDSA signature
    /// - For each lender:
    ///   - 5 * 33 bytes = 165 bytes
    pub fn size_bytes(&self) -> usize {
        let header = 32 + 64;
        let per_lender = 5 * 33;
        header + per_lender * self.lenders.len()
    }

    /// Serialise to the compact fixed binary layout:
    ///   [32B metadata][64B signature][n × (5 × 33B compressed points)]
    ///
    /// The returned `Vec<u8>` is passed directly to `submit_type_a(loan_id, bytes)`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size_bytes());
        out.extend_from_slice(&self.meta.bytes);      // 32 B
        out.extend_from_slice(&self.sig.bytes);       // 64 B
        for lender in &self.lenders {
            out.extend_from_slice(&lender.group_c1);         // 33 B
            out.extend_from_slice(&lender.group_c2);         // 33 B
            out.extend_from_slice(&lender.regulator_c1);     // 33 B
            out.extend_from_slice(&lender.regulator_c2);     // 33 B
            out.extend_from_slice(&lender.partial_dec_share); // 33 B
        } // per lender = 165 B
        out
    }
}

/// Type-2 transaction (repayment) for our MPC scheme.
///
/// 32-byte payload + 64-byte ECDSA signature.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Type2RepaymentTx {
    pub payload: [u8; 32],
    pub sig: EcdsaSignature64,
}

impl Type2RepaymentTx {
    pub fn new_dummy() -> Self {
        Type2RepaymentTx {
            payload: [0u8; 32],
            sig: EcdsaSignature64 { bytes: [0u8; 64] },
        }
    }

    /// Always 96 bytes (32 payload + 64 signature).
    pub fn size_bytes(&self) -> usize {
        32 + 64
    }

    /// Serialise to compact binary layout — 96 bytes:
    ///   [32B payload][64B ECDSA signature]
    ///
    /// The returned `Vec<u8>` is passed directly to `submit_type_b(loan_id, bytes)`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(96);
        out.extend_from_slice(&self.payload); // 32 B
        out.extend_from_slice(&self.sig.bytes); // 64 B
        out
    }
}

// ─── Aggregated (O(1)) transaction types ─────────────────────────────────────

/// One lender's partial decryption share stored **off-chain** together with
/// its Merkle inclusion proof against the root committed in the on-chain tx.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OffChainShareEntry {
    /// 1-based node ID matching `NodeDecPhaseTwoBroadcastMsg::sender`.
    pub node_id: u16,
    /// Compressed secp256k1 EC point: `sk_i × C1_total` (33 bytes).
    #[serde(with = "serde_arrays")]
    pub partial_dec_share: [u8; 33],
    /// Merkle proof that `partial_dec_share` is the leaf at `node_id - 1`
    /// in the tree whose root is stored in the on-chain transaction.
    pub merkle_proof: MerkleProof,
}

/// All per-node partial decryption shares for one loan round, stored
/// **off-chain** (e.g. in a P2P broadcast layer or a DA sidecar).
///
/// The regulator fetches this, verifies each proof against
/// `Type1AggregatedTx::shares_merkle_root`, then performs Lagrange
/// reconstruction exactly as before.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OffChainDecShares {
    /// Identifies the loan round; matches the on-chain `loan_id`.
    pub loan_id: u64,
    /// Copy of the Merkle root already stored on-chain (for quick lookup).
    pub shares_merkle_root: [u8; 32],
    /// One entry per participating lender node, ordered by `node_id`.
    pub entries: Vec<OffChainShareEntry>,
}

/// **Aggregated** Type-1 transaction — constant 260-byte on-chain footprint
/// regardless of the number of lenders.
///
/// Layout (binary):
/// ```text
/// [32 B ] mortgage metadata
/// [64 B ] ECDSA-over-whole-tx signature (coordinator)
/// [33 B ] c1_agg        =  Σ group_c1_i    (ElGamal C1 under group PK)
/// [33 B ] c2_agg        =  Σ group_c2_i    (ElGamal C2 under group PK)
/// [33 B ] reg_c1_agg    =  Σ reg_c1_i      (ElGamal C1 under regulator PK)
/// [33 B ] reg_c2_agg    =  Σ reg_c2_i      (ElGamal C2 under regulator PK)
/// [32 B ] shares_merkle_root  (SHA-256 Merkle root of {partial_dec_share_i})
/// ─────── total = 260 bytes ───────────────────────────────────────────────
/// ```
///
/// Per-lender ciphertexts are aggregated using ElGamal's additive
/// homomorphism; `partial_dec_share_i` values are stored off-chain  
/// in [`OffChainDecShares`] with individual Merkle proofs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Type1AggregatedTx {
    pub meta:               LoanMetadata,     //  32 B
    pub sig:                EcdsaSignature64, //  64 B
    /// Σ group_c1_i — homomorphic aggregate of all lender ElGamal C1 ciphertexts
    /// under the shared group public key.
    #[serde(with = "serde_arrays")]
    pub c1_agg:             [u8; 33],         //  33 B
    /// Σ group_c2_i — homomorphic aggregate of all lender ElGamal C2 ciphertexts
    /// under the shared group public key.  Decrypts to Σ money_i.
    #[serde(with = "serde_arrays")]
    pub c2_agg:             [u8; 33],         //  33 B
    /// Σ reg_c1_i — aggregated ElGamal C1 ciphertexts under the regulator PK.
    #[serde(with = "serde_arrays")]
    pub reg_c1_agg:         [u8; 33],         //  33 B
    /// Σ reg_c2_i — aggregated ElGamal C2 ciphertexts under the regulator PK.
    #[serde(with = "serde_arrays")]
    pub reg_c2_agg:         [u8; 33],         //  33 B
    /// Merkle root (SHA-256) of the ordered set `{partial_dec_share_i}`.
    /// Enables off-chain share storage with on-chain verifiability.
    pub shares_merkle_root: [u8; 32],         //  32 B
}                                             // ─────
                                              // 260 B total

impl Type1AggregatedTx {
    /// Construct a dummy (all-zero bytes) aggregated transaction.
    ///
    /// Used by the TPS benchmark to measure throughput at the correct
    /// constant payload size (260 bytes) without running real MPC.
    pub fn new_agg_dummy() -> Self {
        Type1AggregatedTx {
            meta:               LoanMetadata { bytes: [0u8; 32] },
            sig:                EcdsaSignature64 { bytes: [0u8; 64] },
            c1_agg:             [0u8; 33],
            c2_agg:             [0u8; 33],
            reg_c1_agg:         [0u8; 33],
            reg_c2_agg:         [0u8; 33],
            shares_merkle_root: [0u8; 32],
        }
    }

    /// Constant on-chain size: 260 bytes.
    pub fn size_bytes(&self) -> usize {
        32   // meta
        + 64 // sig
        + 33 // c1_agg
        + 33 // c2_agg
        + 33 // reg_c1_agg
        + 33 // reg_c2_agg
        + 32 // shares_merkle_root
    }

    /// Serialise to compact binary layout — always 260 bytes:
    ///   `[32B meta][64B sig][33B c1_agg][33B c2_agg][33B reg_c1_agg][33B reg_c2_agg][32B merkle_root]`
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size_bytes());
        out.extend_from_slice(&self.meta.bytes);         //  32 B
        out.extend_from_slice(&self.sig.bytes);          //  64 B
        out.extend_from_slice(&self.c1_agg);             //  33 B
        out.extend_from_slice(&self.c2_agg);             //  33 B
        out.extend_from_slice(&self.reg_c1_agg);         //  33 B
        out.extend_from_slice(&self.reg_c2_agg);         //  33 B
        out.extend_from_slice(&self.shares_merkle_root); //  32 B
        out
    }
}

// ─── Checkpoint (off-chain ordering + on-chain checkpoint) tx type ───────────

/// **Checkpoint** transaction — constant 132-byte on-chain footprint.
///
/// In the off-chain ordering + checkpoint protocol, K loan records are ordered
/// and aggregated off-chain.  Only a single checkpoint tx hits the chain per K
/// records, committing a Merkle root over all K `Type1AggregatedTx` payloads.
///
/// Layout (binary):
/// ```text
/// [32 B ] mortgage/batch metadata
/// [64 B ] ECDSA-over-whole-tx signature (coordinator)
/// [32 B ] records_merkle_root  (SHA-256 Merkle root of K Type1AggregatedTx bytes)
/// [ 4 B ] batch_size           (value of K, little-endian u32)
/// ──────── total = 132 bytes ─────────────────────────────────────────────────
/// ```
///
/// Effective record throughput = chain-TPS × K.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Type1CheckpointTx {
    pub meta:                LoanMetadata,     //  32 B
    pub sig:                 EcdsaSignature64, //  64 B
    /// SHA-256 Merkle root over the K `Type1AggregatedTx::to_bytes()` leaves.
    pub records_merkle_root: [u8; 32],        //  32 B
    /// Number of off-chain records committed by this checkpoint.
    pub batch_size:          u32,             //   4 B
}                                             // ──────
                                              // 132 B total

impl Type1CheckpointTx {
    /// Construct a dummy (all-zero except `batch_size`) checkpoint tx.
    /// Used by the TPS benchmark to measure chain throughput at constant 132 B.
    pub fn new_checkpoint_dummy(k: u32) -> Self {
        Type1CheckpointTx {
            meta:                LoanMetadata { bytes: [0u8; 32] },
            sig:                 EcdsaSignature64 { bytes: [0u8; 64] },
            records_merkle_root: [0u8; 32],
            batch_size:          k,
        }
    }

    /// Constant on-chain size: 132 bytes.
    pub fn size_bytes(&self) -> usize {
        32   // meta
        + 64 // sig
        + 32 // records_merkle_root
        + 4  // batch_size (u32)
    }

    /// Serialise to compact binary layout — always 132 bytes:
    ///   `[32B meta][64B sig][32B merkle_root][4B batch_size_le]`
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size_bytes());
        out.extend_from_slice(&self.meta.bytes);         //  32 B
        out.extend_from_slice(&self.sig.bytes);          //  64 B
        out.extend_from_slice(&self.records_merkle_root);//  32 B
        out.extend_from_slice(&self.batch_size.to_le_bytes()); //  4 B
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type1_byte_layout_matches_paper() {
        // Paper: 32 + 64 + 4 * 165 = 756 bytes for 4 lenders
        let tx = Type1OurMpcTx::new_with_lenders(4);
        assert_eq!(tx.to_bytes().len(), 756);
        assert_eq!(tx.size_bytes(), 756);
    }

    #[test]
    fn type1_single_lender() {
        let tx = Type1OurMpcTx::new_with_lenders(1);
        assert_eq!(tx.to_bytes().len(), 32 + 64 + 165);
    }

    #[test]
    fn type2_byte_layout_matches_paper() {
        // Paper: 32 + 64 = 96 bytes
        let tx = Type2RepaymentTx::new_dummy();
        assert_eq!(tx.to_bytes().len(), 96);
        assert_eq!(tx.size_bytes(), 96);
    }

    #[test]
    fn to_bytes_starts_with_metadata() {
        let mut tx = Type1OurMpcTx::new_with_lenders(1);
        tx.meta.bytes[0] = 0xAB;
        let bytes = tx.to_bytes();
        assert_eq!(bytes[0], 0xAB);
    }

    /// Step 6 — verify the 32-byte repayment payload encoding used by
    /// `simulate_repayments_on_chain`: loan_id at [0..8], amount at [8..16].
    #[test]
    fn type2_repayment_payload_encodes_loan_id_and_amount() {
        let loan_id: u64  = 42;
        let amount:  i64  = 5000; // $50.00 in cents

        let mut tx = Type2RepaymentTx::new_dummy();
        tx.payload[0..8].copy_from_slice(&loan_id.to_le_bytes());
        tx.payload[8..16].copy_from_slice(&amount.to_le_bytes());

        let bytes = tx.to_bytes();
        assert_eq!(bytes.len(), 96); // total must always be 96B

        // loan_id readable from first 8 bytes of payload (bytes[0..8])
        let decoded_id = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        assert_eq!(decoded_id, loan_id);

        // amount readable from bytes[8..16]
        let decoded_amt = i64::from_le_bytes(bytes[8..16].try_into().unwrap());
        assert_eq!(decoded_amt, amount);

        // sig portion is all zeros (bytes[32..96])
        assert!(bytes[32..96].iter().all(|&b| b == 0));
    }

    // ─── Aggregated tx tests ──────────────────────────────────────────────

    #[test]
    fn aggregated_tx_is_constant_260_bytes() {
        let tx = Type1AggregatedTx::new_agg_dummy();
        assert_eq!(tx.size_bytes(), 260);
        assert_eq!(tx.to_bytes().len(), 260);
    }

    #[test]
    fn aggregated_tx_layout_check() {
        let mut tx = Type1AggregatedTx::new_agg_dummy();
        // Mark each field so we can verify byte offsets.
        tx.meta.bytes[0]         = 0xAA;
        tx.sig.bytes[0]          = 0xBB;
        tx.c1_agg[0]             = 0xCC;
        tx.c2_agg[0]             = 0xDD;
        tx.reg_c1_agg[0]         = 0xEE;
        tx.reg_c2_agg[0]         = 0xFF;
        tx.shares_merkle_root[0] = 0x11;

        let b = tx.to_bytes();
        assert_eq!(b[0],   0xAA); // meta starts at 0
        assert_eq!(b[32],  0xBB); // sig starts at 32
        assert_eq!(b[96],  0xCC); // c1_agg starts at 96
        assert_eq!(b[129], 0xDD); // c2_agg starts at 129
        assert_eq!(b[162], 0xEE); // reg_c1_agg starts at 162
        assert_eq!(b[195], 0xFF); // reg_c2_agg starts at 195
        assert_eq!(b[228], 0x11); // merkle_root starts at 228
    }

    #[test]
    fn aggregated_tx_is_smaller_than_original_for_any_n() {
        // The aggregated tx must be smaller than the original for n >= 2.
        let agg_size = Type1AggregatedTx::new_agg_dummy().size_bytes();
        for n in 2..=100 {
            let orig_size = Type1OurMpcTx::new_with_lenders(n).size_bytes();
            assert!(
                agg_size < orig_size,
                "agg_size ({}) should be < orig_size ({}) for n={}",
                agg_size, orig_size, n
            );
        }
    }

    // ─── Checkpoint tx tests ──────────────────────────────────────────────

    #[test]
    fn checkpoint_tx_is_constant_132_bytes() {
        for k in [1u32, 10, 50, 100] {
            let tx = Type1CheckpointTx::new_checkpoint_dummy(k);
            assert_eq!(tx.size_bytes(), 132, "size_bytes() wrong for k={}", k);
            assert_eq!(tx.to_bytes().len(), 132, "to_bytes() len wrong for k={}", k);
        }
    }

    #[test]
    fn checkpoint_tx_layout_check() {
        let mut tx = Type1CheckpointTx::new_checkpoint_dummy(42);
        tx.meta.bytes[0]                = 0xAA;
        tx.sig.bytes[0]                 = 0xBB;
        tx.records_merkle_root[0]       = 0xCC;

        let b = tx.to_bytes();
        assert_eq!(b.len(), 132);
        assert_eq!(b[0],   0xAA); // meta starts at 0
        assert_eq!(b[32],  0xBB); // sig starts at 32
        assert_eq!(b[96],  0xCC); // records_merkle_root starts at 96
        // batch_size at [128..132) = 42 as little-endian u32
        let batch = u32::from_le_bytes(b[128..132].try_into().unwrap());
        assert_eq!(batch, 42);
    }

    #[test]
    fn checkpoint_tx_is_smaller_than_aggregated_tx() {
        let chk_size = Type1CheckpointTx::new_checkpoint_dummy(1).size_bytes();
        let agg_size = Type1AggregatedTx::new_agg_dummy().size_bytes();
        assert!(
            chk_size < agg_size,
            "checkpoint ({} B) should be smaller than aggregated ({} B)",
            chk_size, agg_size
        );
    }
}

