// Removed bulletproof dependency
use curv::elliptic::curves::{Point, Scalar, Secp256k1};
use serde::{Deserialize, Serialize};
use elgamal::elgamal::elgamal::{BatchDecRightProof, BatchEncRightProof, ElgamalCipher, EncEqualProof};

type FE = Scalar<Secp256k1>;
type GE = Point<Secp256k1>;

/// Stub range proof \u2014 the original bulletproof dependency was removed.
/// Methods are no-ops so that `dec_phase_one` and `dec_phase_two` continue to
/// compile.  A real range proof library (e.g. `bulletproofs`) should be
/// substituted here if soundness of the range check is required.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RangeProof {
    dummy: Vec<u8>,
}

impl RangeProof {
    /// Stub batch range-proof generation \u2014 returns an empty proof.
    pub fn batch_prove_warpper(_pk: GE, _money_vec: Vec<FE>, _random_vec: Vec<FE>) -> Self {
        RangeProof { dummy: vec![] }
    }

    /// Stub batch range-proof verification \u2014 always succeeds.
    pub fn batch_verify_warpper(
        &self,
        _pk: GE,
        _ped_com_vec: Vec<GE>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDecPhaseOneBroadcastMsg
{
    pub sender:u16,
    pub role:String,
    pub mul_cipher_vec:Vec<ElgamalCipher>,
    pub cipher_vec_reg:Vec<ElgamalCipher>,
    pub batch_enc_proof:BatchEncRightProof,
    pub range_proof:RangeProof,
    pub equal_proof_vec:Vec<EncEqualProof>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDecPhaseTwoBroadcastMsg
{
    pub sender:u16,
    pub role:String,
    pub batch_dec_c1:Vec<Point<Secp256k1>>,
    pub dec_proof:BatchDecRightProof
}

/// Coordinator-level message produced by `Node::assemble_aggregated_tx` after
/// collecting all [`NodeDecPhaseTwoBroadcastMsg`] from every lender node.
///
/// This struct carries everything needed to build the constant-size
/// [`message::tx::Type1AggregatedTx`] that is submitted on-chain, plus the
/// off-chain [`message::tx::OffChainDecShares`] sidecar that the regulator
/// fetches when it needs to run Lagrange reconstruction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDecPhaseTwoAggMsg {
    /// Aggregated ElGamal C1 under the group PK: `Σ group_c1_i`
    pub c1_agg:  Vec<Point<Secp256k1>>,
    /// Aggregated ElGamal C2 under the group PK: `Σ group_c2_i`
    pub c2_agg:  Vec<Point<Secp256k1>>,
    /// Aggregated ElGamal C1 under the regulator PK: `Σ reg_c1_i`
    pub reg_c1_agg: Vec<Point<Secp256k1>>,
    /// Aggregated ElGamal C2 under the regulator PK: `Σ reg_c2_i`
    pub reg_c2_agg: Vec<Point<Secp256k1>>,
    /// 32-byte Merkle root of `{partial_dec_share_i}` — stored on-chain.
    pub shares_merkle_root: [u8; 32],
    /// Per-node partial decryption shares + proofs — stored off-chain.
    pub off_chain_shares: crate::tx::OffChainDecShares,
}