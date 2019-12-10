use crate::{
    cli::{
        build_balance_message, check_account_for_fee, CliCommand, CliCommandInfo, CliConfig,
        CliError, ProcessResult,
    },
    display::println_name_value,
};
use clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand};
use console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use solana_clap_utils::{input_parsers::*, input_validators::*};
use solana_client::{rpc_client::RpcClient, rpc_request::RpcVoteAccountInfo};
use solana_sdk::{
    clock::{self, Slot},
    commitment_config::CommitmentConfig,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_transaction,
};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    thread::sleep,
    time::{Duration, Instant},
};

static CHECK_MARK: Emoji = Emoji("✅ ", "");
static CROSS_MARK: Emoji = Emoji("❌ ", "");
static WARNING: Emoji = Emoji("⚠️", "!");

pub trait ClusterQuerySubCommands {
    fn cluster_query_subcommands(self) -> Self;
}

impl ClusterQuerySubCommands for App<'_, '_> {
    fn cluster_query_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("catchup")
                .about("Wait for a validator to catch up to the cluster")
                .arg(
                    Arg::with_name("node_pubkey")
                        .index(1)
                        .takes_value(true)
                        .value_name("PUBKEY")
                        .validator(is_pubkey_or_keypair)
                        .required(true)
                        .help("Identity pubkey of the validator"),
                ),
        )
        .subcommand(
            SubCommand::with_name("cluster-version")
                .about("Get the version of the cluster entrypoint"),
        )
        .subcommand(SubCommand::with_name("fees").about("Display current cluster fees"))
        .subcommand(SubCommand::with_name("get-block-time")
            .about("Get estimated production time of a block")
            .arg(
                Arg::with_name("slot")
                    .index(1)
                    .takes_value(true)
                    .value_name("SLOT")
                    .required(true)
                    .help("Slot number of the block to query")
            )
        )
        .subcommand(
            SubCommand::with_name("get-epoch-info")
            .about("Get information about the current epoch")
            .arg(
                Arg::with_name("confirmed")
                    .long("confirmed")
                    .takes_value(false)
                    .help(
                        "Return information at maximum-lockout commitment level",
                    ),
            ),
        )
        .subcommand(
            SubCommand::with_name("get-genesis-hash").about("Get the genesis hash"),
        )
        .subcommand(
            SubCommand::with_name("get-slot").about("Get current slot")
            .arg(
                Arg::with_name("confirmed")
                    .long("confirmed")
                    .takes_value(false)
                    .help(
                        "Return slot at maximum-lockout commitment level",
                    ),
            ),
        )
        .subcommand(
            SubCommand::with_name("get-transaction-count").about("Get current transaction count")
            .arg(
                Arg::with_name("confirmed")
                    .long("confirmed")
                    .takes_value(false)
                    .help(
                        "Return count at maximum-lockout commitment level",
                    ),
            ),
        )
        .subcommand(
            SubCommand::with_name("ping")
                .about("Submit transactions sequentially")
                .arg(
                    Arg::with_name("interval")
                        .short("i")
                        .long("interval")
                        .value_name("SECONDS")
                        .takes_value(true)
                        .default_value("2")
                        .help("Wait interval seconds between submitting the next transaction"),
                )
                .arg(
                    Arg::with_name("count")
                        .short("c")
                        .long("count")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .help("Stop after submitting count transactions"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .default_value("1")
                        .help("Number of lamports to transfer for each transaction"),
                )
                .arg(
                    Arg::with_name("timeout")
                        .short("t")
                        .long("timeout")
                        .value_name("SECONDS")
                        .takes_value(true)
                        .default_value("15")
                        .help("Wait up to timeout seconds for transaction confirmation"),
                )
                .arg(
                    Arg::with_name("confirmed")
                        .long("confirmed")
                        .takes_value(false)
                        .help(
                            "Wait until the transaction is confirmed at maximum-lockout commitment level",
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-gossip")
                .about("Show the current gossip network nodes"),
        )
        .subcommand(
            SubCommand::with_name("show-validators")
                .about("Show information about the current validators")
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
    }
}

pub fn parse_catchup(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let node_pubkey = pubkey_of(matches, "node_pubkey").unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::Catchup { node_pubkey },
        require_keypair: false,
    })
}

pub fn parse_cluster_ping(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let lamports = value_t_or_exit!(matches, "lamports", u64);
    let interval = Duration::from_secs(value_t_or_exit!(matches, "interval", u64));
    let count = if matches.is_present("count") {
        Some(value_t_or_exit!(matches, "count", u64))
    } else {
        None
    };
    let timeout = Duration::from_secs(value_t_or_exit!(matches, "timeout", u64));
    let commitment_config = if matches.is_present("confirmed") {
        CommitmentConfig::default()
    } else {
        CommitmentConfig::recent()
    };
    Ok(CliCommandInfo {
        command: CliCommand::Ping {
            lamports,
            interval,
            count,
            timeout,
            commitment_config,
        },
        require_keypair: true,
    })
}

pub fn parse_get_block_time(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let slot = value_t_or_exit!(matches, "slot", u64);
    Ok(CliCommandInfo {
        command: CliCommand::GetBlockTime { slot },
        require_keypair: false,
    })
}

pub fn parse_get_epoch_info(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let commitment_config = if matches.is_present("confirmed") {
        CommitmentConfig::default()
    } else {
        CommitmentConfig::recent()
    };
    Ok(CliCommandInfo {
        command: CliCommand::GetEpochInfo { commitment_config },
        require_keypair: false,
    })
}

pub fn parse_get_slot(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let commitment_config = if matches.is_present("confirmed") {
        CommitmentConfig::default()
    } else {
        CommitmentConfig::recent()
    };
    Ok(CliCommandInfo {
        command: CliCommand::GetSlot { commitment_config },
        require_keypair: false,
    })
}

pub fn parse_get_transaction_count(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let commitment_config = if matches.is_present("confirmed") {
        CommitmentConfig::default()
    } else {
        CommitmentConfig::recent()
    };
    Ok(CliCommandInfo {
        command: CliCommand::GetTransactionCount { commitment_config },
        require_keypair: false,
    })
}

pub fn parse_show_validators(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let use_lamports_unit = matches.is_present("lamports");

    Ok(CliCommandInfo {
        command: CliCommand::ShowValidators { use_lamports_unit },
        require_keypair: false,
    })
}

/// Creates a new process bar for processing that will take an unknown amount of time
fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

pub fn process_catchup(rpc_client: &RpcClient, node_pubkey: &Pubkey) -> ProcessResult {
    let cluster_nodes = rpc_client.get_cluster_nodes()?;

    let rpc_addr = cluster_nodes
        .iter()
        .find(|contact_info| contact_info.pubkey == node_pubkey.to_string())
        .ok_or_else(|| format!("Contact information not found for {}", node_pubkey))?
        .rpc
        .ok_or_else(|| format!("RPC service not found for {}", node_pubkey))?;

    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message("Connecting...");

    let node_client = RpcClient::new_socket(rpc_addr);
    let mut previous_rpc_slot = std::u64::MAX;
    let mut previous_slot_distance = 0;
    let sleep_interval = 5;
    loop {
        let rpc_slot = rpc_client.get_slot_with_commitment(CommitmentConfig::recent())?;
        let node_slot = node_client.get_slot_with_commitment(CommitmentConfig::recent())?;
        if node_slot > std::cmp::min(previous_rpc_slot, rpc_slot) {
            progress_bar.finish_and_clear();
            return Ok(format!(
                "{} has caught up (us:{} them:{})",
                node_pubkey, node_slot, rpc_slot,
            ));
        }

        let slot_distance = rpc_slot as i64 - node_slot as i64;
        progress_bar.set_message(&format!(
            "Validator is {} slots away (us:{} them:{}){}",
            slot_distance,
            node_slot,
            rpc_slot,
            if previous_rpc_slot == std::u64::MAX {
                "".to_string()
            } else {
                let slots_per_second =
                    (previous_slot_distance - slot_distance) as f64 / f64::from(sleep_interval);

                format!(
                    " and {} at {:.1} slots/second",
                    if slots_per_second < 0.0 {
                        "falling behind"
                    } else {
                        "gaining"
                    },
                    slots_per_second,
                )
            }
        ));

        sleep(Duration::from_secs(sleep_interval as u64));
        previous_rpc_slot = rpc_slot;
        previous_slot_distance = slot_distance;
    }
}

pub fn process_cluster_version(rpc_client: &RpcClient) -> ProcessResult {
    let remote_version = rpc_client.get_version()?;
    Ok(remote_version.solana_core)
}

pub fn process_fees(rpc_client: &RpcClient) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    Ok(format!(
        "blockhash: {}\nlamports per signature: {}",
        recent_blockhash, fee_calculator.lamports_per_signature
    ))
}

pub fn process_get_block_time(rpc_client: &RpcClient, slot: Slot) -> ProcessResult {
    let timestamp = rpc_client.get_block_time(slot)?;
    Ok(timestamp.to_string())
}

pub fn process_get_epoch_info(
    rpc_client: &RpcClient,
    commitment_config: &CommitmentConfig,
) -> ProcessResult {
    let epoch_info = rpc_client.get_epoch_info_with_commitment(commitment_config.clone())?;
    println!();
    println_name_value("Current epoch:", &epoch_info.epoch.to_string());
    println_name_value("Current slot:", &epoch_info.absolute_slot.to_string());
    println_name_value(
        "Total slots in current epoch:",
        &epoch_info.slots_in_epoch.to_string(),
    );
    let remaining_slots_in_epoch = epoch_info.slots_in_epoch - epoch_info.slot_index;
    println_name_value(
        "Remaining slots in current epoch:",
        &remaining_slots_in_epoch.to_string(),
    );

    let remaining_time_in_epoch = Duration::from_secs(
        remaining_slots_in_epoch * clock::DEFAULT_TICKS_PER_SLOT / clock::DEFAULT_TICKS_PER_SECOND,
    );
    println_name_value(
        "Time remaining in current epoch:",
        &humantime::format_duration(remaining_time_in_epoch).to_string(),
    );
    Ok("".to_string())
}

pub fn process_get_genesis_hash(rpc_client: &RpcClient) -> ProcessResult {
    let genesis_hash = rpc_client.get_genesis_hash()?;
    Ok(genesis_hash.to_string())
}

pub fn process_get_slot(
    rpc_client: &RpcClient,
    commitment_config: &CommitmentConfig,
) -> ProcessResult {
    let slot = rpc_client.get_slot_with_commitment(commitment_config.clone())?;
    Ok(slot.to_string())
}

pub fn process_get_transaction_count(
    rpc_client: &RpcClient,
    commitment_config: &CommitmentConfig,
) -> ProcessResult {
    let transaction_count =
        rpc_client.get_transaction_count_with_commitment(commitment_config.clone())?;
    Ok(transaction_count.to_string())
}

pub fn process_ping(
    rpc_client: &RpcClient,
    config: &CliConfig,
    lamports: u64,
    interval: &Duration,
    count: &Option<u64>,
    timeout: &Duration,
    commitment_config: &CommitmentConfig,
) -> ProcessResult {
    let to = Keypair::new().pubkey();

    println_name_value("Source account:", &config.keypair.pubkey().to_string());
    println_name_value("Destination account:", &to.to_string());
    println!();

    let (signal_sender, signal_receiver) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = signal_sender.send(());
    })
    .expect("Error setting Ctrl-C handler");

    let mut last_blockhash = Hash::default();
    let mut submit_count = 0;
    let mut confirmed_count = 0;
    let mut confirmation_time: VecDeque<u64> = VecDeque::with_capacity(1024);

    'mainloop: for seq in 0..count.unwrap_or(std::u64::MAX) {
        let (recent_blockhash, fee_calculator) = rpc_client.get_new_blockhash(&last_blockhash)?;
        last_blockhash = recent_blockhash;

        let transaction =
            system_transaction::transfer(&config.keypair, &to, lamports, recent_blockhash);
        check_account_for_fee(
            rpc_client,
            &config.keypair.pubkey(),
            &fee_calculator,
            &transaction.message,
        )?;

        match rpc_client.send_transaction(&transaction) {
            Ok(signature) => {
                let transaction_sent = Instant::now();
                loop {
                    let signature_status = rpc_client.get_signature_status_with_commitment(
                        &signature,
                        commitment_config.clone(),
                    )?;
                    let elapsed_time = Instant::now().duration_since(transaction_sent);
                    if let Some(transaction_status) = signature_status {
                        match transaction_status {
                            Ok(()) => {
                                let elapsed_time_millis = elapsed_time.as_millis() as u64;
                                confirmation_time.push_back(elapsed_time_millis);
                                println!(
                                    "{}{} lamport(s) transferred: seq={:<3} time={:>4}ms signature={}",
                                    CHECK_MARK, lamports, seq, elapsed_time_millis, signature
                                );
                                confirmed_count += 1;
                            }
                            Err(err) => {
                                println!(
                                    "{}Transaction failed:    seq={:<3} error={:?} signature={}",
                                    CROSS_MARK, seq, err, signature
                                );
                            }
                        }
                        break;
                    }

                    if elapsed_time >= *timeout {
                        println!(
                            "{}Confirmation timeout:  seq={:<3}             signature={}",
                            CROSS_MARK, seq, signature
                        );
                        break;
                    }

                    // Sleep for half a slot
                    if signal_receiver
                        .recv_timeout(Duration::from_millis(
                            500 * clock::DEFAULT_TICKS_PER_SLOT / clock::DEFAULT_TICKS_PER_SECOND,
                        ))
                        .is_ok()
                    {
                        break 'mainloop;
                    }
                }
            }
            Err(err) => {
                println!(
                    "{}Submit failed:         seq={:<3} error={:?}",
                    CROSS_MARK, seq, err
                );
            }
        }
        submit_count += 1;

        if signal_receiver.recv_timeout(*interval).is_ok() {
            break 'mainloop;
        }
    }

    println!();
    println!("--- transaction statistics ---");
    println!(
        "{} transactions submitted, {} transactions confirmed, {:.1}% transaction loss",
        submit_count,
        confirmed_count,
        (100. - f64::from(confirmed_count) / f64::from(submit_count) * 100.)
    );
    if !confirmation_time.is_empty() {
        let samples: Vec<f64> = confirmation_time.iter().map(|t| *t as f64).collect();
        let dist = criterion_stats::Distribution::from(samples.into_boxed_slice());
        let mean = dist.mean();
        println!(
            "confirmation min/mean/max/stddev = {:.0}/{:.0}/{:.0}/{:.0} ms",
            dist.min(),
            mean,
            dist.max(),
            dist.std_dev(Some(mean))
        );
    }

    Ok("".to_string())
}

pub fn process_show_gossip(rpc_client: &RpcClient) -> ProcessResult {
    let cluster_nodes = rpc_client.get_cluster_nodes()?;

    fn format_port(addr: Option<SocketAddr>) -> String {
        addr.map(|addr| addr.port().to_string())
            .unwrap_or_else(|| "none".to_string())
    }

    let s: Vec<_> = cluster_nodes
        .iter()
        .map(|node| {
            format!(
                "{:15} | {:44} | {:6} | {:5} | {:5}",
                node.gossip
                    .map(|addr| addr.ip().to_string())
                    .unwrap_or_else(|| "none".to_string()),
                node.pubkey,
                format_port(node.gossip),
                format_port(node.tpu),
                format_port(node.rpc),
            )
        })
        .collect();

    Ok(format!(
        "IP Address      | Node identifier                              \
         | Gossip | TPU   | RPC\n\
         ----------------+----------------------------------------------+\
         --------+-------+-------\n\
         {}\n\
         Nodes: {}",
        s.join("\n"),
        s.len(),
    ))
}

pub fn process_show_validators(rpc_client: &RpcClient, use_lamports_unit: bool) -> ProcessResult {
    let vote_accounts = rpc_client.get_vote_accounts()?;
    let total_active_stake = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .fold(0, |acc, vote_account| acc + vote_account.activated_stake)
        as f64;

    let total_deliquent_stake = vote_accounts
        .delinquent
        .iter()
        .fold(0, |acc, vote_account| acc + vote_account.activated_stake)
        as f64;
    let total_current_stake = total_active_stake - total_deliquent_stake;

    println_name_value(
        "Active Stake:",
        &build_balance_message(total_active_stake as u64, use_lamports_unit, true),
    );
    if total_deliquent_stake > 0. {
        println_name_value(
            "Current Stake:",
            &format!(
                "{} ({:0.2}%)",
                &build_balance_message(total_current_stake as u64, use_lamports_unit, true),
                100. * total_current_stake / total_active_stake
            ),
        );
        println_name_value(
            "Delinquent Stake:",
            &format!(
                "{} ({:0.2}%)",
                &build_balance_message(total_deliquent_stake as u64, use_lamports_unit, true),
                100. * total_deliquent_stake / total_active_stake
            ),
        );
    }
    println!();

    println!(
        "{}",
        style(format!(
            "  {:<44}  {:<44}  {}  {}  {}  {}",
            "Identity Pubkey",
            "Vote Account Pubkey",
            "Commission",
            "Last Vote",
            "Root Block",
            "Active Stake",
        ))
        .bold()
    );

    fn print_vote_account(
        vote_account: &RpcVoteAccountInfo,
        total_active_stake: f64,
        use_lamports_unit: bool,
        delinquent: bool,
    ) {
        fn non_zero_or_dash(v: u64) -> String {
            if v == 0 {
                "-".into()
            } else {
                format!("{}", v)
            }
        }
        println!(
            "{} {:<44}  {:<44}  {:>9}%   {:>8}  {:>10}  {:>12}",
            if delinquent {
                WARNING.to_string()
            } else {
                " ".to_string()
            },
            vote_account.node_pubkey,
            vote_account.vote_pubkey,
            vote_account.commission,
            non_zero_or_dash(vote_account.last_vote),
            non_zero_or_dash(vote_account.root_slot),
            if vote_account.activated_stake > 0 {
                format!(
                    "{} ({:.2}%)",
                    build_balance_message(vote_account.activated_stake, use_lamports_unit, true),
                    100. * vote_account.activated_stake as f64 / total_active_stake
                )
            } else {
                "-".into()
            },
        );
    }

    for vote_account in vote_accounts.current.iter() {
        print_vote_account(vote_account, total_active_stake, use_lamports_unit, false);
    }
    for vote_account in vote_accounts.delinquent.iter() {
        print_vote_account(vote_account, total_active_stake, use_lamports_unit, true);
    }

    Ok("".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");

        let test_cluster_version = test_commands
            .clone()
            .get_matches_from(vec!["test", "cluster-version"]);
        assert_eq!(
            parse_command(&test_cluster_version).unwrap(),
            CliCommandInfo {
                command: CliCommand::ClusterVersion,
                require_keypair: false
            }
        );

        let test_fees = test_commands.clone().get_matches_from(vec!["test", "fees"]);
        assert_eq!(
            parse_command(&test_fees).unwrap(),
            CliCommandInfo {
                command: CliCommand::Fees,
                require_keypair: false
            }
        );

        let slot = 100;
        let test_get_block_time = test_commands.clone().get_matches_from(vec![
            "test",
            "get-block-time",
            &slot.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_get_block_time).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetBlockTime { slot },
                require_keypair: false
            }
        );

        let test_get_epoch_info = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-epoch-info"]);
        assert_eq!(
            parse_command(&test_get_epoch_info).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetEpochInfo {
                    commitment_config: CommitmentConfig::recent(),
                },
                require_keypair: false
            }
        );

        let test_get_genesis_hash = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-genesis-hash"]);
        assert_eq!(
            parse_command(&test_get_genesis_hash).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetGenesisHash,
                require_keypair: false
            }
        );

        let test_get_slot = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-slot"]);
        assert_eq!(
            parse_command(&test_get_slot).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetSlot {
                    commitment_config: CommitmentConfig::recent(),
                },
                require_keypair: false
            }
        );

        let test_transaction_count = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-transaction-count"]);
        assert_eq!(
            parse_command(&test_transaction_count).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetTransactionCount {
                    commitment_config: CommitmentConfig::recent(),
                },
                require_keypair: false
            }
        );

        let test_ping = test_commands.clone().get_matches_from(vec![
            "test",
            "ping",
            "-i",
            "1",
            "-c",
            "2",
            "-t",
            "3",
            "--confirmed",
        ]);
        assert_eq!(
            parse_command(&test_ping).unwrap(),
            CliCommandInfo {
                command: CliCommand::Ping {
                    lamports: 1,
                    interval: Duration::from_secs(1),
                    count: Some(2),
                    timeout: Duration::from_secs(3),
                    commitment_config: CommitmentConfig::default(),
                },
                require_keypair: true
            }
        );
    }
}
