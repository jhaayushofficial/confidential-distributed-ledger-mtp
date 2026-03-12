use message::tx::{Type1OurMpcTx, Type2RepaymentTx};

fn main() {
    let type2_bytes = Type2RepaymentTx::new_dummy().to_bytes().len();

    println!("Transaction sizes (serialised TARS bytes):");
    println!("{:-<62}", "");
    println!("{:<8} {:>16} {:>16} {:>16}", "n", "Type-1 (bytes)", "Type-1 (KB)", "Type-2 (bytes)");
    println!("{:-<62}", "");
    for &n in &[10usize, 50, 100] {
        let t1 = Type1OurMpcTx::new_with_lenders(n).to_bytes().len();
        println!("{:<8} {:>16} {:>16.2} {:>16}",
            n, t1, t1 as f64 / 1024.0, type2_bytes);
    }
    println!("{:-<62}", "");
    println!();
    println!("Paper reference:");
    println!("  n=10 : Type-1 = 1.70 KB,  Type-2 = 96 B");
    println!("  n=50 : Type-1 = 8.15 KB,  Type-2 = 96 B");
    println!("  n=100: Type-1 = 16.21 KB, Type-2 = 96 B");
}
