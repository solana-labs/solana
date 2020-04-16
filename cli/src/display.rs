use crate::cli::SettingType;
use console::style;
use solana_sdk::{
    hash::Hash, native_token::lamports_to_sol, program_utils::limited_deserialize,
    transaction::Transaction,
};
use solana_transaction_status::RpcTransactionStatusMeta;
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

pub fn println_transaction(
    transaction: &Transaction,
    transaction_status: &Option<RpcTransactionStatusMeta>,
    prefix: &str,
) {
    let message = &transaction.message;
    println!("{}Recent Blockhash: {:?}", prefix, message.recent_blockhash);
    for (signature_index, signature) in transaction.signatures.iter().enumerate() {
        println!("{}Signature {}: {:?}", prefix, signature_index, signature);
    }
    println!("{}{:?}", prefix, message.header);
    for (account_index, account) in message.account_keys.iter().enumerate() {
        println!("{}Account {}: {:?}", prefix, account_index, account);
    }
    for (instruction_index, instruction) in message.instructions.iter().enumerate() {
        let program_pubkey = message.account_keys[instruction.program_id_index as usize];
        println!("{}Instruction {}", prefix, instruction_index);
        println!(
            "{}  Program: {} ({})",
            prefix, program_pubkey, instruction.program_id_index
        );
        for (account_index, account) in instruction.accounts.iter().enumerate() {
            let account_pubkey = message.account_keys[*account as usize];
            println!(
                "{}  Account {}: {} ({})",
                prefix, account_index, account_pubkey, account
            );
        }

        let mut raw = true;
        if program_pubkey == solana_vote_program::id() {
            if let Ok(vote_instruction) = limited_deserialize::<
                solana_vote_program::vote_instruction::VoteInstruction,
            >(&instruction.data)
            {
                println!("{}  {:?}", prefix, vote_instruction);
                raw = false;
            }
        } else if program_pubkey == solana_stake_program::id() {
            if let Ok(stake_instruction) = limited_deserialize::<
                solana_stake_program::stake_instruction::StakeInstruction,
            >(&instruction.data)
            {
                println!("{}  {:?}", prefix, stake_instruction);
                raw = false;
            }
        } else if program_pubkey == solana_sdk::system_program::id() {
            if let Ok(system_instruction) = limited_deserialize::<
                solana_sdk::system_instruction::SystemInstruction,
            >(&instruction.data)
            {
                println!("{}  {:?}", prefix, system_instruction);
                raw = false;
            }
        }

        if raw {
            println!("{}  Data: {:?}", prefix, instruction.data);
        }
    }

    if let Some(transaction_status) = transaction_status {
        println!(
            "{}Status: {}",
            prefix,
            match &transaction_status.status {
                Ok(_) => "Ok".into(),
                Err(err) => err.to_string(),
            }
        );
        println!("{}  Fee: {}", prefix, transaction_status.fee);
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
                println!(
                    "{}  Account {} balance: {} SOL",
                    prefix,
                    i,
                    lamports_to_sol(*pre)
                );
            } else {
                println!(
                    "{}  Account {} balance: {} SOL -> {} SOL",
                    prefix,
                    i,
                    lamports_to_sol(*pre),
                    lamports_to_sol(*post)
                );
            }
        }
    } else {
        println!("{}Status: Unavailable", prefix);
    }
}
