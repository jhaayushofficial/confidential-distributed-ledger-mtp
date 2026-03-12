/// Sui Move equivalent of LoanLedger.sol + RepaymentLedger.sol.
///
/// A single shared object stores both Type-A (loan MPC) and Type-B
/// (repayment) records keyed by `loan_id`.  Functionally identical to
/// the FISCO-BCOS Solidity contracts — the only difference is the
/// underlying consensus engine (Mysticeti DAG-BFT vs. PBFT).
module loan_ledger::ledger {
    use sui::table::{Self, Table};
    use sui::event;

    // ═══════════════════════════════════════════════════════════════
    // On-chain state
    // ═══════════════════════════════════════════════════════════════

    /// Shared ledger object created once at publish time.
    public struct LoanLedger has key {
        id: UID,
        type_a: Table<u64, vector<u8>>,
        type_b: Table<u64, vector<u8>>,
        total_a: u64,
        total_b: u64,
    }

    // ═══════════════════════════════════════════════════════════════
    // Events (mirroring Solidity events for indexing)
    // ═══════════════════════════════════════════════════════════════

    public struct TypeARecorded has copy, drop {
        loan_id: u64,
        size: u64,
    }

    public struct TypeBRecorded has copy, drop {
        loan_id: u64,
        size: u64,
    }

    // ═══════════════════════════════════════════════════════════════
    // Init — called once at publish, creates the shared object
    // ═══════════════════════════════════════════════════════════════

    fun init(ctx: &mut TxContext) {
        transfer::share_object(LoanLedger {
            id: object::new(ctx),
            type_a: table::new(ctx),
            type_b: table::new(ctx),
            total_a: 0,
            total_b: 0,
        });
    }

    // ═══════════════════════════════════════════════════════════════
    // Write functions — called via `unsafe_moveCall` from Rust
    // ═══════════════════════════════════════════════════════════════

    /// Record a Type-A (loan / MPC) payload.
    /// Equivalent to `LoanLedger.recordTypeA(uint256, bytes)` in Solidity.
    public entry fun record_type_a(
        ledger: &mut LoanLedger,
        loan_id: u64,
        data: vector<u8>,
    ) {
        let size = data.length();
        if (ledger.type_a.contains(loan_id)) {
            *ledger.type_a.borrow_mut(loan_id) = data;
        } else {
            ledger.type_a.add(loan_id, data);
        };
        ledger.total_a = ledger.total_a + 1;
        event::emit(TypeARecorded { loan_id, size });
    }

    /// Record a Type-B (repayment) payload.
    /// Equivalent to `RepaymentLedger.recordTypeB(uint256, bytes)` in Solidity.
    public entry fun record_type_b(
        ledger: &mut LoanLedger,
        loan_id: u64,
        data: vector<u8>,
    ) {
        let size = data.length();
        if (ledger.type_b.contains(loan_id)) {
            *ledger.type_b.borrow_mut(loan_id) = data;
        } else {
            ledger.type_b.add(loan_id, data);
        };
        ledger.total_b = ledger.total_b + 1;
        event::emit(TypeBRecorded { loan_id, size });
    }

    // ═══════════════════════════════════════════════════════════════
    // Read helpers
    // ═══════════════════════════════════════════════════════════════

    public fun get_type_a(ledger: &LoanLedger, loan_id: u64): &vector<u8> {
        ledger.type_a.borrow(loan_id)
    }

    public fun get_type_b(ledger: &LoanLedger, loan_id: u64): &vector<u8> {
        ledger.type_b.borrow(loan_id)
    }

    public fun total_a(ledger: &LoanLedger): u64 { ledger.total_a }
    public fun total_b(ledger: &LoanLedger): u64 { ledger.total_b }
}
