
use std::fs::File;
use std::io::{BufReader, BufRead};
use std::path::PathBuf;


use curv::elliptic::curves::{Point, Scalar, Secp256k1};
pub type FE = Scalar<Secp256k1>;
pub type GE = Point<Secp256k1>;
use curv::BigInt;
use log::info;
use message::node::dec_msg::{NodeDecPhaseOneBroadcastMsg, NodeDecPhaseTwoBroadcastMsg, RangeProof};
use elgamal::elgamal::elgamal::{map_share_to_new_params, BatchDecRightProof, BatchEncRightProof, ElgamalCipher, EncEqualProof};
use crate::node::Node;
use message::tx::{Type1AggregatedTx, OffChainDecShares, OffChainShareEntry, LoanMetadata, EcdsaSignature64};
use message::merkle::MerkleTree;

impl Node {
    pub fn dec_phase_one(&mut self) -> NodeDecPhaseOneBroadcastMsg
    {
        info!("money calculate is starting");
        let current_dir = std::env::current_dir().unwrap();
        let mut input_path = PathBuf::from(current_dir.clone());
        let path = "src/node/node".to_string() + &self.id.unwrap().to_string() + "/keypair.txt";
        input_path.push(path.clone());
        
        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let mut lines = reader.lines().map(|l| l.unwrap());
        
        self.regulator_pk = Some(serde_json::from_str(&lines.next().unwrap()).unwrap()).unwrap();
        self.pk = Some(serde_json::from_str(&lines.next().unwrap()).unwrap()).unwrap();
        let sk_str = &lines.next().unwrap();
        let sk_vec = sk_str.trim().as_bytes().chunks(2).map(|chunk| u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16)).collect::<Result<Vec<u8>, _>>().unwrap();
        self.sk = Some(FE::from_bytes(&sk_vec).unwrap());
        let batch_size = 1;
        let mut money_vec = Vec::new();
        let mut cipher_vec = Vec::new();
        let mut cipher_vec_reg = Vec::new();
        let mut random_vec = Vec::new();
        let mut equal_proof_vec = Vec::new();
        for _i in 0 .. batch_size{
            let money = FE::from(100);
            let (cipher, random) = ElgamalCipher::encrypt(self.pk.as_ref().unwrap(), &money);
            let (cipher_reg, random_reg) = ElgamalCipher::encrypt(self.regulator_pk.as_ref().unwrap(), &money);
            let equal_proof = EncEqualProof::proof(self.pk.as_ref().unwrap(), self.regulator_pk.as_ref().unwrap(), cipher.clone(), cipher_reg.clone(), &money, &random, &random_reg);
            money_vec.push(money);
            cipher_vec.push(cipher);
            cipher_vec_reg.push(cipher_reg);
            random_vec.push(random);
            equal_proof_vec.push(equal_proof);
        }
        let batch_enc_proof = BatchEncRightProof::proof(self.pk.as_ref().unwrap(), cipher_vec.clone(), money_vec.clone(), random_vec.clone());
        let range_proof = RangeProof::batch_prove_warpper(self.pk.as_ref().unwrap().clone(), money_vec.clone(), random_vec.clone());
        
       
        NodeDecPhaseOneBroadcastMsg
        {
            sender:self.id.unwrap(),
            role:self.role.clone(),
            mul_cipher_vec: cipher_vec,
            cipher_vec_reg,
            batch_enc_proof,
            range_proof,
            equal_proof_vec

        }
    }

    pub fn dec_phase_two(&mut self, msg_vec:&Vec<NodeDecPhaseOneBroadcastMsg>) -> NodeDecPhaseTwoBroadcastMsg
    {
        let batch_cipher_vec:Vec<Vec<ElgamalCipher>> = msg_vec.iter().map(|msg|msg.mul_cipher_vec.clone()).collect();
        let batch_proof = msg_vec.iter().map(|msg|msg.batch_enc_proof.clone()).collect();
        BatchEncRightProof::batch_verify(batch_proof, self.pk.as_ref().unwrap(), batch_cipher_vec.clone()).unwrap();
        for i in 0 .. self.threashold_param.share_counts as usize
        {
            let msg = msg_vec.get(i).unwrap();
            let ped_com_vec = msg.mul_cipher_vec.clone().iter().map(|cipher|cipher.c2.clone()).collect();
            msg.range_proof.batch_verify_warpper(self.pk.as_ref().unwrap().clone(), ped_com_vec).unwrap();
        }
        let mut batch_total_money = batch_cipher_vec[0].clone();
        for cipher_vec in batch_cipher_vec.iter().skip(1).clone(){
            for i in 0 .. cipher_vec.len(){
                batch_total_money[i] = batch_total_money.get(i).unwrap().clone() + cipher_vec.get(i).unwrap().clone();
            }
        }

        self.batch_total_money = Some(batch_total_money.clone());
        let batch_c1:Vec<Point<Secp256k1>> = batch_total_money.iter().map(|money|money.c1.clone()).collect();
        let batch_dec_c1:Vec<Point<Secp256k1>> = batch_c1.iter().map(|money_c1| money_c1 * self.sk.as_ref().unwrap()).collect();
        let pk_share = GE::generator() * self.sk.as_ref().unwrap();


        let dec_proof = BatchDecRightProof::proof(&pk_share, batch_c1, batch_dec_c1.clone(), self.sk.as_ref().unwrap().clone());
        info!("dec_phase_two");
        NodeDecPhaseTwoBroadcastMsg
        {
            sender:self.id.unwrap(),
            role:self.role.clone(),
            batch_dec_c1,
            dec_proof
        }
    }

    pub fn dec_phase_three(&mut self, msg_vec:&Vec<NodeDecPhaseTwoBroadcastMsg>)
    {   
        let current_dir = std::env::current_dir().unwrap();
        let mut input_path = PathBuf::from(current_dir.clone());
        let path = "src/node/node".to_string() + &self.id.unwrap().to_string() + "/pk_share.txt";
        input_path.push(path.clone());
        
        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let mut lines = reader.lines().map(|l| l.unwrap());
        let mut pk_share_vec: Vec<Point<Secp256k1>> = Vec::new();
        let mut pk_share_vec_queue: Vec<Point<Secp256k1>> = Vec::new();
        for _i in 0.. self.threashold_param.share_counts {
            pk_share_vec.push(serde_json::from_str(&lines.next().unwrap()).unwrap());
        }
        for msg in msg_vec {
            pk_share_vec_queue.push(pk_share_vec.get(msg.sender as usize - 1 ).unwrap().clone())
        }

        let batch_dec_proof = msg_vec.iter().map(|msg|msg.dec_proof.clone()).collect();
        let batch_dec_c1_vec = msg_vec.iter().map(|msg|msg.batch_dec_c1.clone()).collect();
        let c1_vec: Vec<Point<Secp256k1>> = self.batch_total_money.as_ref().unwrap().iter().map(|money|money.c1.clone()).collect();
 
        BatchDecRightProof::batch_verify(batch_dec_proof, pk_share_vec_queue, c1_vec.clone(), batch_dec_c1_vec).unwrap();


        let mut lagrange_vec = Vec::new();
        for i in 0 ..= self.threashold_param.threshold as usize
        {
            lagrange_vec.push(BigInt::from(msg_vec.get(i).unwrap().sender));
        }

        let mut batch_c1_total = vec![GE::zero();c1_vec.len()];
        for i in 0 ..= self.threashold_param.threshold as usize
        {
            let msg = msg_vec.get(i).unwrap();
            let li = map_share_to_new_params(BigInt::from(msg.sender), &lagrange_vec);
            for j in 0 .. batch_c1_total.len(){
                let power_li = msg.batch_dec_c1.get(j).unwrap() * li.clone();
                batch_c1_total[j] = batch_c1_total.get(j).unwrap() + power_li;
            }
        }

        let mut gm_vec = Vec::new();
        for i in 0 .. batch_c1_total.len(){
            gm_vec.push(self.batch_total_money.as_ref().unwrap().get(i).unwrap().c2.clone() - batch_c1_total.get(i).unwrap());
        }

        let gm_ = GE::base_point2() * FE::from(400);
        info!("gm_vec = {:?}", gm_vec.clone());
        info!("gm_ = {:?}", gm_);
    }
}

// ─── helpers ────────────────────────────────────────────────────────────────────

/// Serialise a secp256k1 EC point to its 33-byte compressed representation.
///
/// `curv-kzen` serialises `Point<Secp256k1>` via serde as a compressed-hex
/// string, e.g. `"02abcd..."` (66 hex chars = 33 bytes).  This helper peels
/// off the JSON quotes and hex-decodes to produce a `[u8; 33]`.
fn ge_to_33bytes(p: &GE) -> [u8; 33] {
    let json_str = serde_json::to_string(p).expect("point to json");
    // serde_json wraps the hex string in outer quotes: `"02..."`
    let hex = json_str.trim_matches('"');
    let raw = hex::decode(hex).expect("hex-decode compressed point");
    assert_eq!(
        raw.len(),
        33,
        "expected 33-byte compressed secp256k1 point, got {} bytes (hex={})",
        raw.len(),
        hex
    );
    let mut arr = [0u8; 33];
    arr.copy_from_slice(&raw);
    arr
}

// ─── coordinator: assemble the aggregated on-chain tx ────────────────────────────

impl Node {
    /// Assemble the constant-size [`Type1AggregatedTx`] (260 bytes) after all
    /// lender nodes have broadcast their phase-one and phase-two messages.
    ///
    /// # Protocol steps performed here
    ///
    /// 1. **Aggregate group ciphertexts** using ElGamal homomorphism:
    ///    `C1_agg = Σ group_c1_i`,  `C2_agg = Σ group_c2_i`
    /// 2. **Aggregate regulator ciphertexts** similarly:
    ///    `reg_C1_agg`, `reg_C2_agg`
    /// 3. **Collect partial decryption shares** `d_i = sk_i × C1_total` from
    ///    each phase-two message.
    /// 4. **Build a binary Merkle tree** over the compressed-byte form of
    ///    every share; store only the 32-byte root on-chain.
    /// 5. **Create per-lender Merkle proofs** and pack them into
    ///    [`OffChainDecShares`] for distribution to the off-chain data layer.
    ///
    /// # Arguments
    /// * `phase_one_msgs` – one message per lender node (ciphertext inputs).
    /// * `phase_two_msgs` – one message per lender node (partial dec shares).
    /// * `loan_id`        – identifies the current lending round.
    ///
    /// # Returns
    /// * [`Type1AggregatedTx`]  – 260-byte on-chain transaction.
    /// * [`OffChainDecShares`]  – shares + proofs stored off-chain.
    pub fn assemble_aggregated_tx(
        phase_one_msgs: &[NodeDecPhaseOneBroadcastMsg],
        phase_two_msgs: &[NodeDecPhaseTwoBroadcastMsg],
        loan_id: u64,
    ) -> (Type1AggregatedTx, OffChainDecShares) {
        assert!(!phase_one_msgs.is_empty(), "need at least one phase-one message");
        assert_eq!(
            phase_one_msgs.len(),
            phase_two_msgs.len(),
            "phase-one and phase-two message counts must match (got {} vs {})",
            phase_one_msgs.len(),
            phase_two_msgs.len()
        );

        let batch_size = phase_one_msgs[0].mul_cipher_vec.len();
        assert_eq!(batch_size, 1, "only batch_size=1 is currently supported");

        // ── 1. Aggregate group ciphertexts (ElGamal homomorphism) ──────────────
        // C_total = cipher_0 + cipher_1 + ... + cipher_{n-1}
        let mut group_total: Vec<ElgamalCipher> = phase_one_msgs[0].mul_cipher_vec.clone();
        for msg in phase_one_msgs.iter().skip(1) {
            for (j, c) in msg.mul_cipher_vec.iter().enumerate() {
                group_total[j] = group_total[j].clone() + c.clone();
            }
        }

        // ── 2. Aggregate regulator ciphertexts ──────────────────────────────
        let mut reg_total: Vec<ElgamalCipher> = phase_one_msgs[0].cipher_vec_reg.clone();
        for msg in phase_one_msgs.iter().skip(1) {
            for (j, c) in msg.cipher_vec_reg.iter().enumerate() {
                reg_total[j] = reg_total[j].clone() + c.clone();
            }
        }

        // Serialise the four aggregate ciphertext components to [u8; 33].
        let c1_agg     = ge_to_33bytes(&group_total[0].c1);
        let c2_agg     = ge_to_33bytes(&group_total[0].c2);
        let reg_c1_agg = ge_to_33bytes(&reg_total[0].c1);
        let reg_c2_agg = ge_to_33bytes(&reg_total[0].c2);

        // ── 3. Collect partial dec shares and build Merkle tree ──────────────
        // d_i = sk_i × C1_total  (one EC point per node, batch entry 0)
        let share_bytes: Vec<[u8; 33]> = phase_two_msgs
            .iter()
            .map(|msg| ge_to_33bytes(&msg.batch_dec_c1[0]))
            .collect();

        // Build the Merkle tree; leaves are the raw 33-byte share arrays.
        let leaf_slices: Vec<&[u8]> = share_bytes.iter().map(|s| s.as_ref()).collect();
        let tree = MerkleTree::build(&leaf_slices);
        let root = tree.root();

        // ── 4. Pair each share with its Merkle inclusion proof ──────────────
        let entries: Vec<OffChainShareEntry> = phase_two_msgs
            .iter()
            .enumerate()
            .map(|(i, msg)| OffChainShareEntry {
                node_id:           msg.sender,
                partial_dec_share: share_bytes[i],
                merkle_proof:      tree.proof(i),
            })
            .collect();

        let off_chain = OffChainDecShares {
            loan_id,
            shares_merkle_root: root,
            entries,
        };

        // ── 5. Build the on-chain transaction (260 bytes) ────────────────────
        // meta and sig are zeroed here; the coordinator's signing layer would
        // fill meta.bytes with the loan round ID and overwrite sig.bytes with
        // the ECDSA signature over the serialised payload before RPC submission.
        let on_chain_tx = Type1AggregatedTx {
            meta:               LoanMetadata { bytes: [0u8; 32] },
            sig:                EcdsaSignature64 { bytes: [0u8; 64] },
            c1_agg,
            c2_agg,
            reg_c1_agg,
            reg_c2_agg,
            shares_merkle_root: root,
        };

        (on_chain_tx, off_chain)
    }
}