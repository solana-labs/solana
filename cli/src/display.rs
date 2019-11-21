use console::style;
use solana_sdk::transaction::Transaction;

// Pretty print a "name value"
pub fn println_name_value(name: &str, value: &str) {
    let styled_value = if value == "" {
        style("(not set)").italic()
    } else {
        style(value)
    };
    println!("{} {}", style(name).bold(), styled_value);
}

pub fn println_name_value_or(name: &str, value: &str, default_value: &str) {
    if value == "" {
        println!(
            "{} {} {}",
            style(name).bold(),
            style(default_value),
            style("(default)").italic()
        );
    } else {
        println!("{} {}", style(name).bold(), style(value));
    };
}

pub fn println_signers(tx: &Transaction) {
    println!();
    println!("Blockhash: {}", tx.message.recent_blockhash);
    println!("Signers (Pubkey=Signature):");
    tx.signatures
        .iter()
        .zip(tx.message.account_keys.clone())
        .for_each(|(signature, pubkey)| println!("  {:?}={:?}", pubkey, signature));
    println!();
}
