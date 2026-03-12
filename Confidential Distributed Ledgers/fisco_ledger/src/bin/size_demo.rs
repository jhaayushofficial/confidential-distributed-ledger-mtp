use message::tx::{Type1OurMpcTx, Type2RepaymentTx};

/// Formula breakdown:
///   Type 1 (Our MPC) : 32B metadata + 64B ECDSA sig + n × (5 compressed points × 33B)
///                    = 96 + 165·n  bytes
///   Type 2 (repayment): 32B payload + 64B ECDSA sig = 96 B (fixed)
fn main() -> anyhow::Result<()> {
    println!();
    println!("=== Table II — Transaction Size Column (Our MPC) ===");
    println!("  Type 1 formula: 32 + 64 + 165·n  bytes");
    println!("  Type 2 formula: 32 + 64           = 96 B (fixed)");
    println!();
    println!("{:<12} {:>15} {:>15} {:>15} {:>10}",
        "Lenders (n)", "Type1 bytes", "Type1 KB", "Paper KB", "Match?");
    println!("{}", "-".repeat(70));

    let paper_kb: &[(usize, f64)] = &[(10, 1.70), (50, 8.15), (100, 16.21)];

    let mut all_pass = true;
    for &(n, paper) in paper_kb {
        let tx = Type1OurMpcTx::new_with_lenders(n);
        let bytes = tx.to_bytes().len();
        let expected = 96 + 165 * n;
        let kb = bytes as f64 / 1024.0;
        // Paper rounds to 2 decimal places; allow ±0.01 KB tolerance
        let ok = bytes == expected && (kb - paper).abs() < 0.015;
        if !ok { all_pass = false; }
        println!("{:<12} {:>15} {:>14.2} KB {:>14.2} KB {:>10}",
            n, bytes, kb, paper,
            if ok { "✓" } else { "✗" });
    }
    println!();

    // Type 2
    let tx2 = Type2RepaymentTx::new_dummy();
    let bytes2 = tx2.to_bytes().len();
    let ok2 = bytes2 == 96;
    if !ok2 { all_pass = false; }
    println!("Type 2 (any n): {} bytes  →  {} B  Paper: 96 B  {}",
        bytes2, bytes2, if ok2 { "✓" } else { "✗" });
    println!();

    if all_pass {
        println!("All size checks passed ✓ — size column of Table II reproduced.");
    } else {
        anyhow::bail!("One or more size checks failed.");
    }
    Ok(())
}
