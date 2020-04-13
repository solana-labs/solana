use crate::cli::SettingType;
use console::style;
use solana_sdk::hash::Hash;
use std::fmt;

// Pretty print a "name value"
pub fn println_name_value(name: &str, value: &str) {
    let styled_value = if value == "" {
        style("(not set)").italic()
    } else {
        style(value)
    };
    println!("{} {}", style(name).bold(), styled_value);
}

pub fn writeln_name_value(f: &mut fmt::Formatter, name: &str, value: &str) -> fmt::Result {
    let styled_value = if value == "" {
        style("(not set)").italic()
    } else {
        style(value)
    };
    writeln!(f, "{} {}", style(name).bold(), styled_value)
}

pub fn println_name_value_or(name: &str, value: &str, setting_type: SettingType) {
    let description = match setting_type {
        SettingType::Explicit => "",
        SettingType::Computed => "(computed)",
        SettingType::SystemDefault => "(default)",
    };

    println!(
        "{} {} {}",
        style(name).bold(),
        style(value),
        style(description).italic(),
    );
}

pub fn println_signers(
    blockhash: &Hash,
    signers: &[String],
    absent: &[String],
    bad_sig: &[String],
) {
    println!();
    println!("Blockhash: {}", blockhash);
    if !signers.is_empty() {
        println!("Signers (Pubkey=Signature):");
        signers.iter().for_each(|signer| println!("  {}", signer))
    }
    if !absent.is_empty() {
        println!("Absent Signers (Pubkey):");
        absent.iter().for_each(|pubkey| println!("  {}", pubkey))
    }
    if !bad_sig.is_empty() {
        println!("Bad Signatures (Pubkey):");
        bad_sig.iter().for_each(|pubkey| println!("  {}", pubkey))
    }
    println!();
}
