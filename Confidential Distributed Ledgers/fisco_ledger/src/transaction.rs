/// FISCO BCOS 3.x Transaction builder and signer using TARS encoding.
///
/// FISCO BCOS uses Tencent's TARS protocol for transaction serialization.
/// The `sendTransaction` RPC expects a TARS-encoded Transaction struct.
///
/// TARS field head: (tag << 4) | type  (tag < 15)
/// Types (DataHead enum from TarsC++ Tars.h):
///   0=Char, 1=Short, 2=Int32, 3=Int64, 4=Float, 5=Double,
///   6=String1, 7=String4, 8=Map, 9=List,
///   10=StructBegin, 11=StructEnd, 12=ZeroTag, 13=SimpleList

use anyhow::{Context, Result};
use k256::{
    ecdsa::{SigningKey, signature::hazmat::PrehashSigner, RecoveryId},
    pkcs8::DecodePrivateKey,
};
use rand::Rng;
use sha3::{Digest, Keccak256};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

// ──────────────────────────────────────────────────────────────────────────────
// TARS encoder
// ──────────────────────────────────────────────────────────────────────────────

struct Tars {
    buf: Vec<u8>,
}

impl Tars {
    fn new() -> Self { Tars { buf: Vec::new() } }
    fn finish(self) -> Vec<u8> { self.buf }

    /// Write a 1-byte (or 2-byte for tag≥15) field header.
    fn head(&mut self, tag: u8, ty: u8) {
        if tag < 15 {
            self.buf.push((tag << 4) | ty);
        } else {
            self.buf.push(0xF0 | ty);
            self.buf.push(tag);
        }
    }

    /// Write an integer, choosing the smallest TARS int type.
    fn int64(&mut self, tag: u8, v: i64) {
        if v == 0 {
            self.head(tag, 12); // ZeroTag (eZeroTag = 12)
        } else if v >= i8::MIN as i64 && v <= i8::MAX as i64 {
            self.head(tag, 0);
            self.buf.push(v as i8 as u8);
        } else if v >= i16::MIN as i64 && v <= i16::MAX as i64 {
            self.head(tag, 1);
            self.buf.extend_from_slice(&(v as i16).to_be_bytes());
        } else if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
            self.head(tag, 2);
            self.buf.extend_from_slice(&(v as i32).to_be_bytes());
        } else {
            self.head(tag, 3);
            self.buf.extend_from_slice(&v.to_be_bytes());
        }
    }

    fn int32(&mut self, tag: u8, v: i32) { self.int64(tag, v as i64); }

    /// Write a UTF-8 string field.
    fn string(&mut self, tag: u8, s: &str) {
        let b = s.as_bytes();
        if b.len() <= 255 {
            self.head(tag, 6);
            self.buf.push(b.len() as u8);
        } else {
            self.head(tag, 7);
            self.buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
        }
        self.buf.extend_from_slice(b);
    }

    /// Write a byte slice as a TARS SimpleList.
    /// Format: [SimpleList head][char=0 head][length as tars-int][raw bytes]
    fn bytes(&mut self, tag: u8, data: &[u8]) {
        self.head(tag, 13);       // SimpleList (eSimpleList = 13)
        self.buf.push(0x00);      // element type = char (Int8, tag=0)
        self.int32(0, data.len() as i32); // length (TARS int at tag 0)
        self.buf.extend_from_slice(data);
    }

    fn struct_begin(&mut self, tag: u8) { self.head(tag, 10); } // eStructBegin = 10
    fn struct_end(&mut self)            { self.head(0,   11); } // eStructEnd   = 11
}

// ──────────────────────────────────────────────────────────────────────────────
// TransactionData (TARS writeTo output, used for hashing AND as field in Tx)
// ──────────────────────────────────────────────────────────────────────────────
//
// TransactionData fields:
//   tag 1: int32  version    = 0
//   tag 2: string chainID
//   tag 3: string groupID
//   tag 4: int64  blockLimit
//   tag 5: string nonce
//   tag 6: string to
//   tag 7: bytes  input
//   tag 8: string abi

fn encode_tx_data(
    chain_id: &str,
    group_id: &str,
    block_limit: i64,
    nonce: &str,
    to: &str,
    input: &[u8],
) -> Vec<u8> {
    let mut t = Tars::new();
    // tag 1: int32 version (optional, default 0, omit if 0)
    
    t.string(2, chain_id);
    t.string(3, group_id);
    t.int64(4, block_limit);
    t.string(5, nonce);
    
    if !to.is_empty() {
        t.string(6, to);
    }
    
    if !input.is_empty() {
        t.bytes(7, input);
    }
    
    // tag 8: string abi (optional, default "", omit if empty)
    
    t.finish()
}

// ──────────────────────────────────────────────────────────────────────────────
// Transaction outer struct (writeTo, no StructBegin/End wrapper)
// ──────────────────────────────────────────────────────────────────────────────

fn encode_transaction(
    tx_data_bytes: &[u8],  // TARS-encoded TransactionData
    data_hash: &[u8],
    signature: &[u8],
    sender: &[u8],
) -> Vec<u8> {
    let mut t = Tars::new();

    // tag 1: nested TransactionData struct
    t.struct_begin(1);
    t.buf.extend_from_slice(tx_data_bytes);
    t.struct_end();

    // tag 2: dataHash bytes
    t.bytes(2, data_hash);

    // tag 3: signature bytes
    t.bytes(3, signature);

    // tag 4: importTime
    let import_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    t.int64(4, import_time);

    // tag 5: attribute = 0 (optional default, omitted)
    
    // tag 7: sender bytes
    if !sender.is_empty() {
        t.bytes(7, sender);
    }

    // tag 8: extraData = "" (optional default, omitted)
    // tag 9: type = 0 (optional default, omitted)

    t.finish()
}

// ──────────────────────────────────────────────────────────────────────────────
// Address derivation from secp256k1 key
// ──────────────────────────────────────────────────────────────────────────────

fn address_from_signing_key(sk: &SigningKey) -> Vec<u8> {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    let pk = sk.verifying_key();
    let encoded = pk.to_encoded_point(false); // uncompressed 65 bytes
    let pub_bytes = &encoded.as_bytes()[1..];  // drop 0x04 prefix → 64 bytes
    let hash = Keccak256::digest(pub_bytes);
    hash[12..32].to_vec() // last 20 bytes = Ethereum-style address
}

// ──────────────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────────────

pub struct TxSigner {
    signing_key: SigningKey,
    pub sender: Vec<u8>,
}

impl TxSigner {
    pub fn from_pem_file(path: &str) -> Result<Self> {
        let pem = fs::read_to_string(path)
            .with_context(|| format!("Cannot read private key: {}", path))?;
        let signing_key = SigningKey::from_pkcs8_pem(&pem)
            .map_err(|e| anyhow::anyhow!("Failed to parse secp256k1 key: {}", e))?;
        let sender = address_from_signing_key(&signing_key);
        Ok(TxSigner { signing_key, sender })
    }

    /// Build, sign (ECDSA secp256k1), TARS-encode, and hex-encode a transaction.
    pub fn sign_tx(
        &self,
        chain_id: &str,
        group_id: &str,
        block_limit: i64,
        to: &str,
        input: &[u8],
    ) -> Result<String> {
        // Random 32-byte hex nonce
        let nonce: String = {
            let b: [u8; 16] = rand::thread_rng().gen();
            hex::encode(b)
        };

        // 1. TARS-encode TransactionData (for embedding in the outer Transaction)
        let tx_data = encode_tx_data(chain_id, group_id, block_limit, &nonce, to, input);

        // 2. Hash = keccak256( field-by-field concat ) matching TarsHashable.h:
        //    version(4 BE) || chainID || groupID || blockLimit(8 BE)
        //    || nonce || to || input || abi(empty)
        let data_hash: [u8; 32] = {
            let mut h = Keccak256::new();
            h.update((0i32).to_be_bytes());          // version = 0
            h.update(chain_id.as_bytes());
            h.update(group_id.as_bytes());
            h.update(block_limit.to_be_bytes());
            h.update(nonce.as_bytes());
            h.update(to.as_bytes());
            h.update(input);
            h.update(b"");                           // abi = ""
            h.finalize().into()
        };

        // 3. Sign with secp256k1
        let (sig, rec_id): (k256::ecdsa::Signature, RecoveryId) = self
            .signing_key
            .sign_prehash_recoverable(&data_hash)
            .map_err(|e| anyhow::anyhow!("Signing failed: {}", e))?;

        // FISCO BCOS signature: r(32) + s(32) + v(1)
        let mut signature = sig.to_bytes().to_vec(); // 64 bytes
        signature.push(rec_id.to_byte());            // v

        // 4. Encode outer Transaction struct
        let tx_bytes = encode_transaction(&tx_data, &data_hash, &signature, &self.sender);

        let hex_tx = format!("0x{}", hex::encode(&tx_bytes));
        Ok(hex_tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tars_zero() {
        let mut t = Tars::new();
        t.int64(0, 0);
        assert_eq!(t.finish(), vec![0x0C]); // (0<<4)|12 = ZeroTag at tag 0
    }

    #[test]
    fn tars_string1() {
        let mut t = Tars::new();
        t.string(1, "hi");
        // (1<<4)|6=0x16, len=2, 'h','i'
        assert_eq!(t.finish(), vec![0x16, 0x02, b'h', b'i']);
    }

    #[test]
    fn tx_data_starts_correctly() {
        let data = encode_tx_data("chain0", "group0", 600, "abc", "0xdeadbeef", b"");
        // First byte: String1 at tag 2 = 0x26 (chainID, since version=0 is omitted)
        assert_eq!(data[0], 0x26);
        // Second byte: length of "chain0" = 6
        assert_eq!(data[1], 0x06);
    }
}
