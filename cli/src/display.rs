use crate::cli::SettingType;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use solana_sdk::{
    hash::Hash, native_token::lamports_to_sol, program_utils::limited_deserialize,
    transaction::Transaction,
};
use solana_transaction_status::RpcTransactionStatusMeta;
use std::{fmt, io};

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

pub fn write_transaction<W: io::Write>(
    w: &mut W,
    transaction: &Transaction,
    transaction_status: &Option<RpcTransactionStatusMeta>,
    prefix: &str,
) -> io::Result<()> {
    let message = &transaction.message;
    writeln!(
        w,
        "{}Recent Blockhash: {:?}",
        prefix, message.recent_blockhash
    )?;
    for (signature_index, signature) in transaction.signatures.iter().enumerate() {
        writeln!(
            w,
            "{}Signature {}: {:?}",
            prefix, signature_index, signature
        )?;
    }
    writeln!(w, "{}{:?}", prefix, message.header)?;
    for (account_index, account) in message.account_keys.iter().enumerate() {
        writeln!(w, "{}Account {}: {:?}", prefix, account_index, account)?;
    }
    for (instruction_index, instruction) in message.instructions.iter().enumerate() {
        let program_pubkey = message.account_keys[instruction.program_id_index as usize];
        writeln!(w, "{}Instruction {}", prefix, instruction_index)?;
        writeln!(
            w,
            "{}  Program: {} ({})",
            prefix, program_pubkey, instruction.program_id_index
        )?;
        for (account_index, account) in instruction.accounts.iter().enumerate() {
            let account_pubkey = message.account_keys[*account as usize];
            writeln!(
                w,
                "{}  Account {}: {} ({})",
                prefix, account_index, account_pubkey, account
            )?;
        }

        let mut raw = true;
        if program_pubkey == solana_vote_program::id() {
            if let Ok(vote_instruction) = limited_deserialize::<
                solana_vote_program::vote_instruction::VoteInstruction,
            >(&instruction.data)
            {
                writeln!(w, "{}  {:?}", prefix, vote_instruction)?;
                raw = false;
            }
        } else if program_pubkey == solana_stake_program::id() {
            if let Ok(stake_instruction) = limited_deserialize::<
                solana_stake_program::stake_instruction::StakeInstruction,
            >(&instruction.data)
            {
                writeln!(w, "{}  {:?}", prefix, stake_instruction)?;
                raw = false;
            }
        } else if program_pubkey == solana_sdk::system_program::id() {
            if let Ok(system_instruction) = limited_deserialize::<
                solana_sdk::system_instruction::SystemInstruction,
            >(&instruction.data)
            {
                writeln!(w, "{}  {:?}", prefix, system_instruction)?;
                raw = false;
            }
        }

        if raw {
            writeln!(w, "{}  Data: {:?}", prefix, instruction.data)?;
        }
    }

    if let Some(transaction_status) = transaction_status {
        writeln!(
            w,
            "{}Status: {}",
            prefix,
            match &transaction_status.status {
                Ok(_) => "Ok".into(),
                Err(err) => err.to_string(),
            }
        )?;
        writeln!(
            w,
            "{}  Fee: {} SOL",
            prefix,
            lamports_to_sol(transaction_status.fee)
        )?;
        assert_eq!(
            transaction_status.pre_balances.len(),
            transaction_status.post_balances.len()
        );
        for (i, (pre, post)) in transaction_status
            .pre_balances
            .iter()
            .zip(transaction_status.post_balances.iter())
            .enumerate()
        {
            if pre == post {
                writeln!(
                    w,
                    "{}  Account {} balance: {} SOL",
                    prefix,
                    i,
                    lamports_to_sol(*pre)
                )?;
            } else {
                writeln!(
                    w,
                    "{}  Account {} balance: {} SOL -> {} SOL",
                    prefix,
                    i,
                    lamports_to_sol(*pre),
                    lamports_to_sol(*post)
                )?;
            }
        }
    } else {
        writeln!(w, "{}Status: Unavailable", prefix)?;
    }

    Ok(())
}

pub fn println_transaction(
    transaction: &Transaction,
    transaction_status: &Option<RpcTransactionStatusMeta>,
    prefix: &str,
) {
    let mut w = Vec::new();
    if write_transaction(&mut w, transaction, transaction_status, prefix).is_ok() {
        if let Ok(s) = String::from_utf8(w) {
            print!("{}", s);
        }
    }
}

/// Creates a new process bar for processing that will take an unknown amount of time
pub fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}
