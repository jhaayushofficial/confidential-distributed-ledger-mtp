//! Off-chain ordering + on-chain checkpoint protocol.
//!
//! # Overview
//!
//! Instead of writing one `Type1AggregatedTx` (260 B) to the blockchain per
//! loan record, this module buffers **K** records off-chain and flushes a
//! single `Type1CheckpointTx` (132 B) once the buffer is full.
//!
//! ```text
//!  off-chain buffer          on-chain
//!  ┌──────────────┐
//!  │ record[0]    │
//!  │ record[1]    │──── Merkle root ──▶  Type1CheckpointTx (132 B)
//!  │   ...        │
//!  │ record[K-1]  │
//!  └──────────────┘
//! ```
//!
//! Effective record throughput = chain-TPS × K.
//! Chain tx size drops from 260 B → 132 B (49 % smaller).
//!
//! # Data availability
//!
//! Any verifier that has the K off-chain `Type1AggregatedTx` records can
//! independently reconstruct the Merkle root and confirm it matches the
//! on-chain checkpoint.  The off-chain records are broadcast over the P2P
//! layer (or stored in a DA sidecar) using [`OffChainCheckpointBatch`].

use crate::merkle::MerkleTree;
use crate::tx::{Type1AggregatedTx, Type1CheckpointTx};

/// Off-chain batch that corresponds to one on-chain [`Type1CheckpointTx`].
///
/// Recipients use [`OffChainCheckpointBatch::verify`] to confirm that the
/// included records reconstruct the committed `records_merkle_root`.
#[derive(Clone, Debug)]
pub struct OffChainCheckpointBatch {
    /// Identifies which blockchain tx this batch corresponds to.
    pub chain_tx_loan_id:   u64,
    /// The K records that were committed.
    pub records:            Vec<Type1AggregatedTx>,
}

impl OffChainCheckpointBatch {
    /// Recompute the Merkle root over `records` and return it.
    ///
    /// Verifiers compare the result against `Type1CheckpointTx::records_merkle_root`.
    pub fn compute_root(&self) -> [u8; 32] {
        if self.records.is_empty() {
            return [0u8; 32];
        }
        let leaves: Vec<Vec<u8>> = self.records.iter().map(|r| r.to_bytes()).collect();
        MerkleTree::build(&leaves).root()
    }

    /// Verify that these records match the root committed on-chain.
    ///
    /// Returns `true` iff the computed Merkle root equals `committed_root`.
    pub fn verify(&self, committed_root: &[u8; 32]) -> bool {
        &self.compute_root() == committed_root
    }
}

/// Accumulates [`Type1AggregatedTx`] records off-chain and produces a
/// [`Type1CheckpointTx`] + [`OffChainCheckpointBatch`] once K records
/// have been collected.
///
/// # Example
///
/// ```rust
/// use message::checkpoint::OffChainCheckpointBuffer;
/// use message::tx::Type1AggregatedTx;
///
/// let mut buf = OffChainCheckpointBuffer::new(10); // K = 10
/// for _ in 0..10 {
///     buf.push(Type1AggregatedTx::new_agg_dummy());
/// }
/// assert!(buf.is_full());
/// let (checkpoint_tx, batch) = buf.flush(0);
/// assert_eq!(checkpoint_tx.to_bytes().len(), 132);
/// assert!(batch.verify(&checkpoint_tx.records_merkle_root));
/// ```
pub struct OffChainCheckpointBuffer {
    /// Target batch size K.
    capacity: usize,
    /// Accumulated records (not yet committed on-chain).
    records: Vec<Type1AggregatedTx>,
}

impl OffChainCheckpointBuffer {
    /// Create a new buffer that flushes every `k` records.
    ///
    /// # Panics
    /// Panics if `k == 0`.
    pub fn new(k: usize) -> Self {
        assert!(k > 0, "batch size k must be >= 1");
        OffChainCheckpointBuffer {
            capacity: k,
            records: Vec::with_capacity(k),
        }
    }

    /// Push one aggregated record into the buffer.
    pub fn push(&mut self, record: Type1AggregatedTx) {
        self.records.push(record);
    }

    /// Returns `true` when the buffer has accumulated K records.
    pub fn is_full(&self) -> bool {
        self.records.len() >= self.capacity
    }

    /// Current number of buffered records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// `true` when the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Flush all buffered records into a checkpoint tx + off-chain batch.
    ///
    /// Steps:
    /// 1. Build a Merkle tree over `record.to_bytes()` for each record.
    /// 2. Construct a `Type1CheckpointTx` (132 B) with the Merkle root.
    /// 3. Drain the buffer and return both the checkpoint and the batch.
    ///
    /// `chain_loan_id` is embedded in the off-chain batch for correlation;
    /// callers typically use the loan_id they pass to `submit_type_a`.
    ///
    /// # Panics
    /// Panics if the buffer is empty.
    pub fn flush(&mut self, chain_loan_id: u64) -> (Type1CheckpointTx, OffChainCheckpointBatch) {
        assert!(!self.records.is_empty(), "cannot flush an empty buffer");

        let k = self.records.len() as u32;

        // Build Merkle tree over the serialised records.
        let leaves: Vec<Vec<u8>> = self.records.iter().map(|r| r.to_bytes()).collect();
        let tree = MerkleTree::build(&leaves);
        let root = tree.root();

        // Build the on-chain checkpoint tx (132 B).
        let checkpoint = Type1CheckpointTx {
            meta:                crate::tx::LoanMetadata { bytes: [0u8; 32] },
            sig:                 crate::tx::EcdsaSignature64 { bytes: [0u8; 64] },
            records_merkle_root: root,
            batch_size:          k,
        };

        // Build the off-chain batch sidecar.
        let records = std::mem::replace(&mut self.records, Vec::with_capacity(self.capacity));
        let batch = OffChainCheckpointBatch {
            chain_tx_loan_id: chain_loan_id,
            records,
        };

        (checkpoint, batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::Type1AggregatedTx;

    #[test]
    fn buffer_is_full_after_k_pushes() {
        let mut buf = OffChainCheckpointBuffer::new(5);
        for _ in 0..4 {
            buf.push(Type1AggregatedTx::new_agg_dummy());
            assert!(!buf.is_full());
        }
        buf.push(Type1AggregatedTx::new_agg_dummy());
        assert!(buf.is_full());
    }

    #[test]
    fn flush_produces_132_byte_checkpoint() {
        let mut buf = OffChainCheckpointBuffer::new(10);
        for _ in 0..10 {
            buf.push(Type1AggregatedTx::new_agg_dummy());
        }
        let (chk, _batch) = buf.flush(1);
        assert_eq!(chk.to_bytes().len(), 132);
        assert_eq!(chk.batch_size, 10);
    }

    #[test]
    fn flush_drains_buffer() {
        let mut buf = OffChainCheckpointBuffer::new(4);
        for _ in 0..4 {
            buf.push(Type1AggregatedTx::new_agg_dummy());
        }
        buf.flush(1);
        assert!(buf.is_empty());
    }

    #[test]
    fn batch_verify_roundtrip() {
        let mut buf = OffChainCheckpointBuffer::new(8);
        for _ in 0..8 {
            buf.push(Type1AggregatedTx::new_agg_dummy());
        }
        let (chk, batch) = buf.flush(99);
        assert!(
            batch.verify(&chk.records_merkle_root),
            "off-chain batch must verify against on-chain checkpoint root"
        );
    }

    #[test]
    fn batch_verify_fails_if_record_tampered() {
        let mut buf = OffChainCheckpointBuffer::new(4);
        for _ in 0..4 {
            buf.push(Type1AggregatedTx::new_agg_dummy());
        }
        let (chk, mut batch) = buf.flush(1);
        // Corrupt one record byte.
        batch.records[0].c1_agg[0] = 0xFF;
        assert!(
            !batch.verify(&chk.records_merkle_root),
            "tampered batch should NOT verify"
        );
    }

    #[test]
    fn buffer_can_be_reused_after_flush() {
        let mut buf = OffChainCheckpointBuffer::new(3);
        for round in 0u64..4 {
            buf.push(Type1AggregatedTx::new_agg_dummy());
            buf.push(Type1AggregatedTx::new_agg_dummy());
            buf.push(Type1AggregatedTx::new_agg_dummy());
            assert!(buf.is_full());
            let (chk, batch) = buf.flush(round);
            assert_eq!(chk.batch_size, 3);
            assert!(batch.verify(&chk.records_merkle_root));
        }
    }
}
