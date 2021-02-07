use {
    crate::{get_validator_rpc_addr, get_validator_start_time},
    console::style,
    indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle},
    solana_client::{
        client_error, rpc_client::RpcClient, rpc_request, rpc_response::RpcContactInfo,
    },
    solana_sdk::{
        clock::{Slot, DEFAULT_TICKS_PER_SLOT, MS_PER_TICK},
        commitment_config::CommitmentConfig,
        native_token::Sol,
        pubkey::Pubkey,
    },
    std::{
        io,
        path::{Path, PathBuf},
        thread,
        time::Duration,
    },
};

/// Creates a new process bar for processing that will take an unknown amount of time
fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar.set_draw_target(ProgressDrawTarget::stdout());
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

/// Pretty print a "name value"
fn println_name_value(name: &str, value: &str) {
    println!("{} {}", style(name).bold(), value);
}

pub struct Dashboard {
    progress_bar: ProgressBar,
    ledger_path: PathBuf,
}

impl Dashboard {
    pub fn new(ledger_path: &Path, log_path: Option<&Path>) -> Result<Self, io::Error> {
        println_name_value("Ledger location:", &format!("{}", ledger_path.display()));
        if let Some(log_path) = log_path {
            println_name_value("Log:", &format!("{}", log_path.display()));
        }

        let rpc_addr = get_validator_rpc_addr(&ledger_path)?;
        if rpc_addr.is_none() {
            return Err(io::Error::new(io::ErrorKind::Other, "RPC not available"));
        }

        let progress_bar = new_spinner_progress_bar();
        progress_bar.set_message("Initializing...");

        Ok(Self {
            progress_bar,
            //ledger_path: ledger_path.clone().to_path_buf(),
            ledger_path: ledger_path.to_path_buf(),
        })
    }

    pub fn run(self) -> ! {
        let Self {
            progress_bar,
            ledger_path,
        } = self;

        progress_bar.set_message("Connecting...");

        let rpc_addr = get_validator_rpc_addr(&ledger_path).unwrap().unwrap();
        let rpc_client = RpcClient::new_socket(rpc_addr);

        // Wait until RPC starts responding...
        loop {
            match rpc_client.get_identity() {
                Ok(_) => break,
                Err(err) => {
                    progress_bar.set_message(&format!("{}", err));
                    thread::sleep(Duration::from_millis(500));
                }
            }
        }

        drop(progress_bar);

        let identity = &rpc_client.get_identity().expect("get_identity");

        println_name_value("Identity:", &identity.to_string());

        fn get_contact_info(rpc_client: &RpcClient, identity: &Pubkey) -> Option<RpcContactInfo> {
            rpc_client
                .get_cluster_nodes()
                .ok()
                .unwrap_or_default()
                .into_iter()
                .find(|node| node.pubkey == identity.to_string())
        }

        if let Some(contact_info) = get_contact_info(&rpc_client, &identity) {
            println_name_value(
                "Version:",
                &contact_info.version.unwrap_or_else(|| "?".to_string()),
            );
            if let Some(gossip) = contact_info.gossip {
                println_name_value("Gossip Address:", &gossip.to_string());
            }
            if let Some(tpu) = contact_info.tpu {
                println_name_value("TPU Address:", &tpu.to_string());
            }
            if let Some(rpc) = contact_info.rpc {
                println_name_value("JSON RPC URL:", &format!("http://{}", rpc.to_string()));
            }
        }

        let progress_bar = new_spinner_progress_bar();

        fn get_validator_stats(
            rpc_client: &RpcClient,
            identity: &Pubkey,
        ) -> client_error::Result<(Slot, Slot, Slot, u64, Sol, String)> {
            let processed_slot =
                rpc_client.get_slot_with_commitment(CommitmentConfig::processed())?;
            let confirmed_slot =
                rpc_client.get_slot_with_commitment(CommitmentConfig::confirmed())?;
            let finalized_slot =
                rpc_client.get_slot_with_commitment(CommitmentConfig::finalized())?;
            let transaction_count =
                rpc_client.get_transaction_count_with_commitment(CommitmentConfig::processed())?;
            let identity_balance = rpc_client
                .get_balance_with_commitment(identity, CommitmentConfig::confirmed())?
                .value;

            let health = match rpc_client.get_health() {
                Ok(()) => "ok".to_string(),
                Err(err) => {
                    if let client_error::ClientErrorKind::RpcError(
                        rpc_request::RpcError::RpcResponseError {
                            code: _,
                            message: _,
                            data:
                                rpc_request::RpcResponseErrorData::NodeUnhealthy {
                                    num_slots_behind: Some(num_slots_behind),
                                },
                        },
                    ) = &err.kind
                    {
                        format!("{} slots behind", num_slots_behind)
                    } else {
                        "unhealthy".to_string()
                    }
                }
            };

            Ok((
                processed_slot,
                confirmed_slot,
                finalized_slot,
                transaction_count,
                Sol(identity_balance),
                health,
            ))
        }

        let mut start_time = get_validator_start_time(&ledger_path).ok();
        loop {
            let snapshot_slot = rpc_client.get_snapshot_slot().ok();

            for _i in 0..10 {
                match get_validator_stats(&rpc_client, &identity) {
                    Ok((
                        processed_slot,
                        confirmed_slot,
                        finalized_slot,
                        transaction_count,
                        identity_balance,
                        health,
                    )) => {
                        let uptime = match start_time {
                            Some(start_time) => {
                                let uptime =
                                    chrono::Duration::from_std(start_time.elapsed().unwrap())
                                        .unwrap();

                                format!(
                                    "{:02}:{:02}:{:02} ",
                                    uptime.num_hours(),
                                    uptime.num_minutes() % 60,
                                    uptime.num_seconds() % 60
                                )
                            }
                            None => " ".to_string(),
                        };

                        progress_bar.set_message(&format!(
                            "{}{}| \
                                Processed Slot: {} | Confirmed Slot: {} | Finalized Slot: {} | \
                                Snapshot Slot: {} | \
                                Transactions: {} | {}",
                            uptime,
                            if health == "ok" {
                                "".to_string()
                            } else {
                                format!("| {} ", style(health).bold().red())
                            },
                            processed_slot,
                            confirmed_slot,
                            finalized_slot,
                            snapshot_slot
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                            transaction_count,
                            identity_balance
                        ));
                    }
                    Err(err) => {
                        start_time = get_validator_start_time(&ledger_path).ok();
                        progress_bar.set_message(&format!("{}", err));
                    }
                }
                thread::sleep(Duration::from_millis(
                    MS_PER_TICK * DEFAULT_TICKS_PER_SLOT / 2,
                ));
            }
        }
    }
}
