use serde::{Deserialize, Serialize};

/// Money is represented as an integer number of cents.
/// (This matches the paper's "scale by 100" approach.)
pub type Cents = i64;

pub type LoanId = u64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoanRequest {
    pub loan_id: LoanId,
    /// Requested total amount by borrower (cents).
    pub requested: Cents,
    /// Minimum acceptable amount by borrower (cents).
    pub minimum: Cents,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repayment {
    pub loan_id: LoanId,
    /// Total repayment amount paid by borrower (cents).
    pub amount: Cents,
    /// Agent fee / revenue for this repayment (cents).
    pub agent_fee: Cents,
}

/// Regulator → Nodes: start a batch of loans (joint decision).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegLoanBatchStartMsg {
    pub loans: Vec<LoanRequest>,
}

/// Agent/Regulator → Nodes: record a repayment event to be used in settlement.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegRepaymentBatchMsg {
    pub repayments: Vec<Repayment>,
}

/// Node → Agent/Regulator: open daily net settlement differential.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeSettlementOpenMsg {
    pub sender: u16,
    /// Net differential for the day (cents, scaled).
    pub net: Cents,
    /// Aggregated randomness corresponding to the public commitment recomputation.
    ///
    /// Note: this is represented as a decimal string for portability across serde formats.
    pub net_randomness: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LoanDecisionStatus {
    ApprovedFull,
    ApprovedPartial,
    Rejected,
}

/// Regulator's on-ledger record for a single loan decision.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoanDecisionRecord {
    pub loan_id: LoanId,
    pub requested: Cents,
    pub minimum: Cents,
    /// Sum of preferred contributions from all lenders (loanα).
    pub loan_alpha: Cents,
    /// Actual loan amount provided to the borrower (ĥ_loanα in the paper).
    pub hat_loan_alpha: Cents,
    pub status: LoanDecisionStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LendingMsg {
    RegLoanBatchStart(RegLoanBatchStartMsg),
    RegRepaymentBatch(RegRepaymentBatchMsg),
    NodeSettlementOpen(NodeSettlementOpenMsg),
}

