use {
    crate::cli_output::CliSignatureVerificationStatus,
    chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone, Utc},
    console::style,
    indicatif::{ProgressBar, ProgressStyle},
    solana_sdk::{
        clock::UnixTimestamp, hash::Hash, message::Message, native_token::lamports_to_sol,
        program_utils::limited_deserialize, pubkey::Pubkey, stake, transaction::Transaction,
    },
    solana_transaction_status::UiTransactionStatusMeta,
    spl_memo::{id as spl_memo_id, v1::id as spl_memo_v1_id},
    std::{collections::HashMap, fmt, io},
};

#[derive(Clone, Debug)]
pub struct BuildBalanceMessageConfig {
    pub use_lamports_unit: bool,
    pub show_unit: bool,
    pub trim_trailing_zeros: bool,
}

impl Default for BuildBalanceMessageConfig {
    fn default() -> Self {
        Self {
            use_lamports_unit: false,
            show_unit: true,
            trim_trailing_zeros: true,
        }
    }
}

fn is_memo_program(k: &Pubkey) -> bool {
    let k_str = k.to_string();
    (k_str == spl_memo_v1_id().to_string()) || (k_str == spl_memo_id().to_string())
}

pub fn build_balance_message_with_config(
    lamports: u64,
    config: &BuildBalanceMessageConfig,
) -> String {
    let value = if config.use_lamports_unit {
        lamports.to_string()
    } else {
        let sol = lamports_to_sol(lamports);
        let sol_str = format!("{:.9}", sol);
        if config.trim_trailing_zeros {
            sol_str
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        } else {
            sol_str
        }
    };
    let unit = if config.show_unit {
        if config.use_lamports_unit {
            let ess = if lamports == 1 { "" } else { "s" };
            format!(" lamport{}", ess)
        } else {
            " SOL".to_string()
        }
    } else {
        "".to_string()
    };
    format!("{}{}", value, unit)
}

pub fn build_balance_message(lamports: u64, use_lamports_unit: bool, show_unit: bool) -> String {
    build_balance_message_with_config(
        lamports,
        &BuildBalanceMessageConfig {
            use_lamports_unit,
            show_unit,
            ..BuildBalanceMessageConfig::default()
        },
    )
}

// Pretty print a "name value"
pub fn println_name_value(name: &str, value: &str) {
    let styled_value = if value.is_empty() {
        style("(not set)").italic()
    } else {
        style(value)
    };
    println!("{} {}", style(name).bold(), styled_value);
}

pub fn writeln_name_value(f: &mut dyn fmt::Write, name: &str, value: &str) -> fmt::Result {
    let styled_value = if value.is_empty() {
        style("(not set)").italic()
    } else {
        style(value)
    };
    writeln!(f, "{} {}", style(name).bold(), styled_value)
}

pub fn format_labeled_address(pubkey: &str, address_labels: &HashMap<String, String>) -> String {
    let label = address_labels.get(pubkey);
    match label {
        Some(label) => format!(
            "{:.31} ({:.4}..{})",
            label,
            pubkey,
            pubkey.split_at(pubkey.len() - 4).1
        ),
        None => pubkey.to_string(),
    }
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

fn format_account_mode(message: &Message, index: usize) -> String {
    format!(
        "{}r{}{}", // accounts are always readable...
        if message.is_signer(index) {
            "s" // stands for signer
        } else {
            "-"
        },
        if message.is_writable(index, /*demote_program_write_locks=*/ true) {
            "w" // comment for consistent rust fmt (no joking; lol)
        } else {
            "-"
        },
        // account may be executable on-chain while not being
        // designated as a program-id in the message
        if message.maybe_executable(index) {
            "x"
        } else {
            // programs to be executed via CPI cannot be identified as
            // executable from the message
            "-"
        },
    )
}

pub fn write_transaction<W: io::Write>(
    w: &mut W,
    transaction: &Transaction,
    transaction_status: &Option<UiTransactionStatusMeta>,
    prefix: &str,
    sigverify_status: Option<&[CliSignatureVerificationStatus]>,
    block_time: Option<UnixTimestamp>,
) -> io::Result<()> {
    let message = &transaction.message;
    if let Some(block_time) = block_time {
        writeln!(
            w,
            "{}Block Time: {:?}",
            prefix,
            Local.timestamp(block_time, 0)
        )?;
    }
    writeln!(
        w,
        "{}Recent Blockhash: {:?}",
        prefix, message.recent_blockhash
    )?;
    let sigverify_statuses = if let Some(sigverify_status) = sigverify_status {
        sigverify_status
            .iter()
            .map(|s| format!(" ({})", s))
            .collect()
    } else {
        vec!["".to_string(); transaction.signatures.len()]
    };
    for (signature_index, (signature, sigverify_status)) in transaction
        .signatures
        .iter()
        .zip(&sigverify_statuses)
        .enumerate()
    {
        writeln!(
            w,
            "{}Signature {}: {:?}{}",
            prefix, signature_index, signature, sigverify_status,
        )?;
    }
    let mut fee_payer_index = None;
    for (account_index, account) in message.account_keys.iter().enumerate() {
        if fee_payer_index.is_none() && message.is_non_loader_key(account_index) {
            fee_payer_index = Some(account_index)
        }
        writeln!(
            w,
            "{}Account {}: {} {}{}",
            prefix,
            account_index,
            format_account_mode(message, account_index),
            account,
            if Some(account_index) == fee_payer_index {
                " (fee payer)"
            } else {
                ""
            },
        )?;
    }
    for (instruction_index, instruction) in message.instructions.iter().enumerate() {
        let program_pubkey = message.account_keys[instruction.program_id_index as usize];
        writeln!(w, "{}Instruction {}", prefix, instruction_index)?;
        writeln!(
            w,
            "{}  Program:   {} ({})",
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
        } else if program_pubkey == stake::program::id() {
            if let Ok(stake_instruction) =
                limited_deserialize::<stake::instruction::StakeInstruction>(&instruction.data)
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
        } else if is_memo_program(&program_pubkey) {
            if let Ok(s) = std::str::from_utf8(&instruction.data) {
                writeln!(w, "{}  Data: \"{}\"", prefix, s)?;
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
            "{}  Fee: ◎{}",
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
                    "{}  Account {} balance: ◎{}",
                    prefix,
                    i,
                    lamports_to_sol(*pre)
                )?;
            } else {
                writeln!(
                    w,
                    "{}  Account {} balance: ◎{} -> ◎{}",
                    prefix,
                    i,
                    lamports_to_sol(*pre),
                    lamports_to_sol(*post)
                )?;
            }
        }

        if let Some(log_messages) = &transaction_status.log_messages {
            if !log_messages.is_empty() {
                writeln!(w, "{}Log Messages:", prefix,)?;
                for log_message in log_messages {
                    writeln!(w, "{}  {}", prefix, log_message)?;
                }
            }
        }

        if let Some(rewards) = &transaction_status.rewards {
            if !rewards.is_empty() {
                writeln!(w, "{}Rewards:", prefix,)?;
                writeln!(
                    w,
                    "{}  {:<44}  {:^15}  {:<15}  {:<20}",
                    prefix, "Address", "Type", "Amount", "New Balance"
                )?;
                for reward in rewards {
                    let sign = if reward.lamports < 0 { "-" } else { "" };
                    writeln!(
                        w,
                        "{}  {:<44}  {:^15}  {}◎{:<14.9}  ◎{:<18.9}",
                        prefix,
                        reward.pubkey,
                        if let Some(reward_type) = reward.reward_type {
                            format!("{}", reward_type)
                        } else {
                            "-".to_string()
                        },
                        sign,
                        lamports_to_sol(reward.lamports.abs() as u64),
                        lamports_to_sol(reward.post_balance)
                    )?;
                }
            }
        }
    } else {
        writeln!(w, "{}Status: Unavailable", prefix)?;
    }

    Ok(())
}

pub fn println_transaction(
    transaction: &Transaction,
    transaction_status: &Option<UiTransactionStatusMeta>,
    prefix: &str,
    sigverify_status: Option<&[CliSignatureVerificationStatus]>,
    block_time: Option<UnixTimestamp>,
) {
    let mut w = Vec::new();
    if write_transaction(
        &mut w,
        transaction,
        transaction_status,
        prefix,
        sigverify_status,
        block_time,
    )
    .is_ok()
    {
        if let Ok(s) = String::from_utf8(w) {
            print!("{}", s);
        }
    }
}

pub fn writeln_transaction(
    f: &mut dyn fmt::Write,
    transaction: &Transaction,
    transaction_status: &Option<UiTransactionStatusMeta>,
    prefix: &str,
    sigverify_status: Option<&[CliSignatureVerificationStatus]>,
    block_time: Option<UnixTimestamp>,
) -> fmt::Result {
    let mut w = Vec::new();
    if write_transaction(
        &mut w,
        transaction,
        transaction_status,
        prefix,
        sigverify_status,
        block_time,
    )
    .is_ok()
    {
        if let Ok(s) = String::from_utf8(w) {
            write!(f, "{}", s)?;
        }
    }
    Ok(())
}

/// Creates a new process bar for processing that will take an unknown amount of time
pub fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

pub fn unix_timestamp_to_string(unix_timestamp: UnixTimestamp) -> String {
    match NaiveDateTime::from_timestamp_opt(unix_timestamp, 0) {
        Some(ndt) => DateTime::<Utc>::from_utc(ndt, Utc).to_rfc3339_opts(SecondsFormat::Secs, true),
        None => format!("UnixTimestamp {}", unix_timestamp),
    }
}

#[cfg(test)]
mod test {
    use {super::*, solana_sdk::pubkey::Pubkey};

    #[test]
    fn test_format_labeled_address() {
        let pubkey = Pubkey::default().to_string();
        let mut address_labels = HashMap::new();

        assert_eq!(format_labeled_address(&pubkey, &address_labels), pubkey);

        address_labels.insert(pubkey.to_string(), "Default Address".to_string());
        assert_eq!(
            &format_labeled_address(&pubkey, &address_labels),
            "Default Address (1111..1111)"
        );

        address_labels.insert(
            pubkey.to_string(),
            "abcdefghijklmnopqrstuvwxyz1234567890".to_string(),
        );
        assert_eq!(
            &format_labeled_address(&pubkey, &address_labels),
            "abcdefghijklmnopqrstuvwxyz12345 (1111..1111)"
        );
    }
}
