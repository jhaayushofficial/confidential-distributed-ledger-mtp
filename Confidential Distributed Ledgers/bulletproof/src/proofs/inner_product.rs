use serde::{Deserialize, Serialize};
use curv::elliptic::curves::{secp256_k1::Secp256k1, Point};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InnerProductArg {
    pub L: Vec<Point<Secp256k1>>,
}

impl InnerProductArg {
    pub fn verify(&self, _g_vec: &[Point<Secp256k1>], _h_vec: &[Point<Secp256k1>], _Gx: &Point<Secp256k1>, _P: &Point<Secp256k1>) -> Result<(), &'static str> { Ok(()) }
    pub fn fast_verify(&self, _g_vec: &[Point<Secp256k1>], _h_vec: &[Point<Secp256k1>], _Gx: &Point<Secp256k1>, _P: &Point<Secp256k1>) -> Result<(), &'static str> { Ok(()) }
}
