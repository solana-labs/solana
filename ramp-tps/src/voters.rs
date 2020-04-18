use crate::notifier::Notifier;
use crate::utils;
use log::*;
use solana_client::{client_error::Result as ClientResult, rpc_client::RpcClient};
use solana_sdk::{
    clock::Slot,
    epoch_schedule::EpochSchedule,
    native_token::sol_to_lamports,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use solana_stake_program::{
    stake_instruction,
    stake_state::{Authorized as StakeAuthorized, Lockup},
};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    str::FromStr,
    thread::sleep,
    time::Duration,
};

// The percentage of leader slots that validators complete in order to receive the stake
// reward at the end of a TPS round.
const MIN_LEADER_SLOT_PCT: f64 = 80.0;

#[derive(Default)]
pub struct LeaderRecord {
    total_slots: u64,
    missed_slots: u64,
}

impl LeaderRecord {
    pub fn completed_slot_pct(&self) -> f64 {
        if self.total_slots == 0 {
            0f64
        } else {
            let completed_slots = self.total_slots - self.missed_slots;
            100f64 * completed_slots as f64 / self.total_slots as f64
        }
    }

    pub fn healthy(&self) -> bool {
        self.completed_slot_pct() >= MIN_LEADER_SLOT_PCT
    }
}

/// Calculate the leader record for each active validator
pub fn calculate_leader_records(
    rpc_client: &RpcClient,
    epoch_schedule: &EpochSchedule,
    start_slot: Slot,
    end_slot: Slot,
    notifier: &Notifier,
) -> ClientResult<HashMap<Pubkey, LeaderRecord>> {
    let start_epoch = epoch_schedule.get_epoch(start_slot);
    let end_epoch = epoch_schedule.get_epoch(end_slot);
    let confirmed_blocks: HashSet<_> = rpc_client
        .get_confirmed_blocks(start_slot, Some(end_slot))?
        .into_iter()
        .collect();

    let mut leader_records = HashMap::<Pubkey, LeaderRecord>::new();
    for epoch in start_epoch..=end_epoch {
        let first_slot_in_epoch = epoch_schedule.get_first_slot_in_epoch(epoch);
        let start_slot = std::cmp::max(start_slot, first_slot_in_epoch);
        let last_slot_in_epoch = epoch_schedule.get_last_slot_in_epoch(epoch);
        let end_slot = std::cmp::min(end_slot, last_slot_in_epoch);

        rpc_client
            .get_leader_schedule(Some(start_slot))?
            .unwrap_or_else(|| utils::bail(notifier, "Error: Leader schedule was not found"))
            .into_iter()
            .map(|(pk, s)| (Pubkey::from_str(&pk).unwrap(), s))
            .for_each(|(pubkey, leader_slots)| {
                let mut record = leader_records.entry(pubkey).or_default();
                for slot_index in leader_slots.iter() {
                    let slot = (*slot_index as u64) + first_slot_in_epoch;
                    if slot >= start_slot && slot <= end_slot {
                        record.total_slots += 1;
                        if !confirmed_blocks.contains(&slot) {
                            record.missed_slots += 1;
                        }
                    }
                }
            });
    }

    Ok(leader_records)
}

pub fn fetch_active_validators(rpc_client: &RpcClient) -> HashMap<Pubkey, Pubkey> {
    match rpc_client.get_vote_accounts() {
        Err(err) => {
            warn!("Failed to get_vote_accounts(): {}", err);
            HashMap::new()
        }
        Ok(vote_accounts) => vote_accounts
            .current
            .into_iter()
            .filter_map(|info| {
                if let (Ok(node_pubkey), Ok(vote_pubkey)) = (
                    Pubkey::from_str(&info.node_pubkey),
                    Pubkey::from_str(&info.vote_pubkey),
                ) {
                    Some((node_pubkey, vote_pubkey))
                } else {
                    None
                }
            })
            .collect(),
    }
}

/// Endlessly retry stake delegation until success
fn delegate_stake(
    rpc_client: &RpcClient,
    faucet_keypair: &Keypair,
    vote_account_pubkey: &Pubkey,
    sol_gift: u64,
) {
    let stake_account_keypair = Keypair::new();
    info!(
        "delegate_stake: stake pubkey: {}",
        stake_account_keypair.pubkey()
    );
    let mut retry_count = 0;
    loop {
        let recent_blockhash = loop {
            match rpc_client.get_recent_blockhash() {
                Ok(response) => break response.0,
                Err(err) => {
                    error!("Failed to get recent blockhash: {}", err);
                    sleep(Duration::from_secs(5));
                }
            }
        };

        let mut transaction = Transaction::new_signed_instructions(
            &[faucet_keypair, &stake_account_keypair],
            stake_instruction::create_account_and_delegate_stake(
                &faucet_keypair.pubkey(),
                &stake_account_keypair.pubkey(),
                &vote_account_pubkey,
                &StakeAuthorized::auto(&faucet_keypair.pubkey()),
                &Lockup::default(),
                sol_to_lamports(sol_gift as f64),
            ),
            recent_blockhash,
        );

        // Check if stake was delegated but just failed to confirm on an earlier attempt
        if retry_count > 0 {
            if let Ok(stake_account) = rpc_client.get_account(&stake_account_keypair.pubkey()) {
                if stake_account.owner == solana_stake_program::id() {
                    break;
                }
            }
        }

        if let Err(err) = rpc_client.send_and_confirm_transaction(
            &mut transaction,
            &[faucet_keypair, &stake_account_keypair],
        ) {
            error!(
                "Failed to delegate stake (retries: {}): {}",
                retry_count, err
            );
            retry_count += 1;
            sleep(Duration::from_secs(5));
        } else {
            break;
        }
    }
}

/// Announce validator status leader slot performance
pub fn announce_results(
    starting_validators: &HashMap<Pubkey, Pubkey>,
    remaining_validators: &HashMap<Pubkey, Pubkey>,
    pubkey_to_keybase: Rc<dyn Fn(&Pubkey) -> String>,
    leader_records: &HashMap<Pubkey, LeaderRecord>,
    notifier: &mut Notifier,
) {
    let buffer_records = |keys: Vec<&Pubkey>, notifier: &mut Notifier| {
        if keys.is_empty() {
            notifier.buffer("* None".to_string());
            return;
        }

        let mut validators = vec![];
        for pubkey in keys {
            let name = pubkey_to_keybase(pubkey);
            if let Some(record) = leader_records.get(pubkey) {
                validators.push(format!(
                    "* {} ({:.1}% leader efficiency)",
                    name,
                    record.completed_slot_pct()
                ));
            }
        }
        validators.sort();
        notifier.buffer_vec(validators);
    };

    let healthy: Vec<_> = remaining_validators
        .keys()
        .filter(|k| leader_records.get(k).map(|r| r.healthy()).unwrap_or(false))
        .collect();

    let unhealthy: Vec<_> = remaining_validators
        .keys()
        .filter(|k| leader_records.get(k).map(|r| !r.healthy()).unwrap_or(true))
        .collect();

    let inactive: Vec<_> = starting_validators
        .keys()
        .filter(|k| !remaining_validators.contains_key(k))
        .collect();

    notifier.buffer("Healthy Validators:".to_string());
    buffer_records(healthy, notifier);
    notifier.buffer("Unhealthy Validators:".to_string());
    buffer_records(unhealthy, notifier);
    notifier.buffer("Inactive Validators:".to_string());
    buffer_records(inactive, notifier);

    notifier.flush();
}

/// Award stake to the surviving validators by delegating stake to their vote account
pub fn award_stake(
    rpc_client: &RpcClient,
    faucet_keypair: &Keypair,
    voters: Vec<(String, &Pubkey)>,
    sol_gift: u64,
    notifier: &mut Notifier,
) {
    for (node_pubkey, vote_account_pubkey) in voters {
        info!("Delegate {} SOL to {}", sol_gift, node_pubkey);
        delegate_stake(rpc_client, faucet_keypair, vote_account_pubkey, sol_gift);
        notifier.buffer(format!("Delegated {} SOL to {}", sol_gift, node_pubkey));
    }
    notifier.flush();
}
