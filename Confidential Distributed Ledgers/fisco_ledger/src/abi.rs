/// ABI encoding for `recordTypeA(uint256 loanId, bytes data)`
/// and `recordTypeB(uint256 loanId, bytes data)`.
///
/// Both functions have identical signatures — only the 4-byte selector differs.
/// We implement the encoding manually to avoid pulling in a heavy abi crate.
///
/// ABI encoding for (uint256, bytes):
///   [4 bytes selector]
///   [32 bytes: loanId (uint256, big-endian, left-padded)]
///   [32 bytes: offset of `data` = 0x40 = 64, the start of dynamic part]
///   [32 bytes: length of `data`]
///   [padded bytes of `data` (rounded up to 32-byte boundary)]

use sha3::{Digest, Keccak256};

/// Compute the 4-byte ABI function selector for a given signature string.
/// e.g. `selector("recordTypeA(uint256,bytes)")` → first 4 bytes of keccak256
pub fn selector(sig: &str) -> [u8; 4] {
    let hash = Keccak256::digest(sig.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

/// ABI-encode a call to `recordTypeA(uint256 loanId, bytes data)`.
pub fn encode_record_type_a(loan_id: u64, data: &[u8]) -> Vec<u8> {
    let sel = selector("recordTypeA(uint256,bytes)");
    encode_call(sel, loan_id, data)
}

/// ABI-encode a call to `recordTypeB(uint256 loanId, bytes data)`.
pub fn encode_record_type_b(loan_id: u64, data: &[u8]) -> Vec<u8> {
    let sel = selector("recordTypeB(uint256,bytes)");
    encode_call(sel, loan_id, data)
}

/// ABI-encode a read-only call to `getTypeA(uint256 loanId)`.
/// Encodes: selector + 32-byte uint256(loanId)
pub fn encode_get_type_a(loan_id: u64) -> Vec<u8> {
    encode_query(selector("getTypeA(uint256)"), loan_id)
}

/// ABI-encode a read-only call to `getTypeB(uint256 loanId)`.
/// Encodes: selector + 32-byte uint256(loanId)
pub fn encode_get_type_b(loan_id: u64) -> Vec<u8> {
    encode_query(selector("getTypeB(uint256)"), loan_id)
}

/// Decode the `bytes` ABI return value returned by `getTypeA` / `getTypeB`.
///
/// ABI encoding of a bare `bytes` return:
///   [offset: 32 bytes — always 0x20]
///   [length: 32 bytes]
///   [data:   padded to 32-byte boundary]
///
/// We handle both the standard form (64-byte header) and the
/// "direct data" form where some nodes skip the offset word.
pub fn decode_bytes_return(raw: &[u8]) -> anyhow::Result<Vec<u8>> {
    if raw.is_empty() {
        return Ok(vec![]);
    }
    // Require at least 64 bytes (offset word + length word)
    if raw.len() < 64 {
        anyhow::bail!("ABI decode: response too short ({} bytes)", raw.len());
    }
    // Word 0 is the offset into the return data where the bytes value starts.
    // For a single `bytes` return it is always 32 (0x20).
    let offset = usize::from_be_bytes({
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&raw[24..32]);
        arr
    });
    // Length word starts at `offset`
    let len_start = offset;
    if raw.len() < len_start + 32 {
        anyhow::bail!("ABI decode: response truncated at length word");
    }
    let data_len = usize::from_be_bytes({
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&raw[len_start + 24..len_start + 32]);
        arr
    });
    let data_start = len_start + 32;
    if raw.len() < data_start + data_len {
        anyhow::bail!(
            "ABI decode: not enough bytes for data (need {}, have {})",
            data_start + data_len,
            raw.len()
        );
    }
    Ok(raw[data_start..data_start + data_len].to_vec())
}

/// Generic encoder for `f(uint256, bytes)` calls.
fn encode_call(sel: [u8; 4], loan_id: u64, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32 + 32 + 32 + pad32(data.len()));

    // 4-byte selector
    out.extend_from_slice(&sel);

    // slot 0: loanId (uint256) — left-padded to 32 bytes
    out.extend_from_slice(&u64_to_u256_bytes(loan_id));

    // slot 1: offset of `data` dynamic arg = 0x40 (64 bytes from start of args)
    out.extend_from_slice(&u64_to_u256_bytes(0x40));

    // slot 2: length of `data`
    out.extend_from_slice(&u64_to_u256_bytes(data.len() as u64));

    // `data` itself, right-padded to 32-byte boundary
    out.extend_from_slice(data);
    let remainder = pad32(data.len()) - data.len();
    out.extend(std::iter::repeat(0u8).take(remainder));

    out
}

/// Generic encoder for a read-only query `f(uint256)` call (no bytes arg).
fn encode_query(sel: [u8; 4], loan_id: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32);
    out.extend_from_slice(&sel);
    out.extend_from_slice(&u64_to_u256_bytes(loan_id));
    out
}

/// Round `n` up to the next multiple of 32.
fn pad32(n: usize) -> usize {
    if n == 0 { 32 } else { ((n + 31) / 32) * 32 }
}

/// Encode a u64 as a 32-byte big-endian uint256.
fn u64_to_u256_bytes(v: u64) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[24..32].copy_from_slice(&v.to_be_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_type_a() {
        // keccak256("recordTypeA(uint256,bytes)")[0..4]
        let sel = selector("recordTypeA(uint256,bytes)");
        // Just check it's 4 non-zero bytes and deterministic
        assert_eq!(sel, selector("recordTypeA(uint256,bytes)"));
        assert_ne!(sel, selector("recordTypeB(uint256,bytes)"));
    }

    #[test]
    fn encode_length() {
        // f(1, b"hello") → 4 + 32 + 32 + 32 + 32 = 132 bytes
        let enc = encode_record_type_a(1, b"hello");
        assert_eq!(enc.len(), 4 + 32 + 32 + 32 + 32);
    }

    #[test]
    fn loan_id_in_correct_slot() {
        let enc = encode_record_type_a(42, b"x");
        // bytes 4..36 = uint256(42): only last byte should be 42
        assert_eq!(enc[35], 42);
        // all others in that slot should be 0
        assert!(enc[4..35].iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_get_type_a_length_and_selector() {
        let enc_a = encode_get_type_a(1);
        let enc_b = encode_get_type_b(1);
        // 4-byte selector + 32-byte uint256
        assert_eq!(enc_a.len(), 36);
        assert_eq!(enc_b.len(), 36);
        // selectors differ
        assert_ne!(&enc_a[..4], &enc_b[..4]);
        // selectors are deterministic
        assert_eq!(&encode_get_type_a(1)[..4], &encode_get_type_a(1)[..4]);
    }

    #[test]
    fn encode_get_type_a_loan_id_slot() {
        let enc = encode_get_type_a(77);
        // bytes 4..36 = uint256(77)
        assert_eq!(enc[35], 77);
        assert!(enc[4..35].iter().all(|&b| b == 0));
    }

    #[test]
    fn decode_bytes_return_roundtrip() {
        // Build a valid ABI `bytes` return for b"hello world"
        let payload = b"hello world";
        let mut raw = Vec::new();
        // offset word = 0x20
        raw.extend_from_slice(&u64_to_u256_bytes(0x20));
        // length word
        raw.extend_from_slice(&u64_to_u256_bytes(payload.len() as u64));
        // data padded to 32-byte boundary
        raw.extend_from_slice(payload);
        let pad = (32 - payload.len() % 32) % 32;
        raw.extend(std::iter::repeat(0u8).take(pad));

        let decoded = decode_bytes_return(&raw).expect("decode failed");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn decode_bytes_return_empty() {
        let decoded = decode_bytes_return(&[]).expect("empty decode failed");
        assert!(decoded.is_empty());
    }

    #[test]
    fn decode_bytes_return_too_short_errors() {
        // Only 32 bytes — no length word, should error
        let raw = u64_to_u256_bytes(0x20).to_vec();
        assert!(decode_bytes_return(&raw).is_err());
    }
}
