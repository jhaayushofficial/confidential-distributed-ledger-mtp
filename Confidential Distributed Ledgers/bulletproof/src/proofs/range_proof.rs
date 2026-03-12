use serde::{Deserialize, Serialize};
use curv::elliptic::curves::{secp256_k1::Secp256k1, Point};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RangeProof {
    dummy: Vec<u8>,
}

impl RangeProof {
    pub fn prove_multiple(_values: &[u64], _blindings: &[curv::arithmetic::BigInt], _n: usize) -> Result<Self, &'static str> {
        Ok(RangeProof { dummy: vec![] })
    }

    pub fn verify_multiple(&self, _commitments: &[Point<Secp256k1>], _n: usize) -> Result<(), &'static str> {
        Ok(())
    }
}
