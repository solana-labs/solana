//! The `compute_leader_confirmation_service` module implements the tools necessary
//! to generate a thread which regularly calculates the last confirmation times
//! observed by the leader

use crate::bank::Bank;

use crate::service::Service;
use solana_metrics::{influxdb, submit};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing;
use solana_sdk::vote_program::{self, VoteProgram};
use std::result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmationError {
    NoValidSupermajority,
}

pub const COMPUTE_CONFIRMATION_MS: u64 = 100;

pub struct ComputeLeaderConfirmationService {
    compute_confirmation_thread: JoinHandle<()>,
}

impl ComputeLeaderConfirmationService {
    fn get_last_supermajority_timestamp(
        bank: &Arc<Bank>,
        leader_id: Pubkey,
        now: u64,
        last_valid_validator_timestamp: u64,
    ) -> result::Result<u64, ConfirmationError> {
        let mut total_stake = 0;

        // Hold an accounts_db read lock as briefly as possible, just long enough to collect all
        // the vote states
        let vote_states: Vec<(Pubkey, VoteProgram)> = bank
            .accounts
            .accounts_db
            .read()
            .unwrap()
            .accounts
            .iter()
            .filter_map(|(node_id, account)| {
                if vote_program::check_id(&account.owner) {
                    if let Ok(vote_state) = VoteProgram::deserialize(&account.userdata) {
                        if leader_id != *node_id {
                            return Some((*node_id, vote_state));
                        }
                    }
                }
                None
            })
            .collect();

        let mut ticks_and_stakes: Vec<(u64, u64)> = vote_states
            .iter()
            .filter_map(|(node_id, vote_state)| {
                let validator_stake = bank.get_balance(&node_id);
                total_stake += validator_stake;
                // Filter out any validators that don't have at least one vote
                // by returning None
                vote_state
                    .votes
                    .back()
                    .map(|vote| (vote.tick_height, validator_stake))
            })
            .collect();

        let super_majority_stake = (2 * total_stake) / 3;

        if let Some(last_valid_validator_timestamp) =
            bank.get_confirmation_timestamp(&mut ticks_and_stakes, super_majority_stake)
        {
            return Ok(last_valid_validator_timestamp);
        }

        if last_valid_validator_timestamp != 0 {
            submit(
                influxdb::Point::new(&"leader-confirmation")
                    .add_field(
                        "duration_ms",
                        influxdb::Value::Integer((now - last_valid_validator_timestamp) as i64),
                    )
                    .to_owned(),
            );
        }

        Err(ConfirmationError::NoValidSupermajority)
    }

    pub fn compute_confirmation(
        bank: &Arc<Bank>,
        leader_id: Pubkey,
        last_valid_validator_timestamp: &mut u64,
    ) {
        let now = timing::timestamp();
        if let Ok(super_majority_timestamp) = Self::get_last_supermajority_timestamp(
            bank,
            leader_id,
            now,
            *last_valid_validator_timestamp,
        ) {
            let confirmation_ms = now - super_majority_timestamp;

            *last_valid_validator_timestamp = super_majority_timestamp;
            bank.set_confirmation_time((now - *last_valid_validator_timestamp) as usize);

            submit(
                influxdb::Point::new(&"leader-confirmation")
                    .add_field(
                        "duration_ms",
                        influxdb::Value::Integer(confirmation_ms as i64),
                    )
                    .to_owned(),
            );
        }
    }

    /// Create a new ComputeLeaderConfirmationService for computing confirmation.
    pub fn new(bank: Arc<Bank>, leader_id: Pubkey, exit: Arc<AtomicBool>) -> Self {
        let compute_confirmation_thread = Builder::new()
            .name("solana-leader-confirmation-stage".to_string())
            .spawn(move || {
                let mut last_valid_validator_timestamp = 0;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    Self::compute_confirmation(
                        &bank,
                        leader_id,
                        &mut last_valid_validator_timestamp,
                    );
                    sleep(Duration::from_millis(COMPUTE_CONFIRMATION_MS));
                }
            })
            .unwrap();

        (ComputeLeaderConfirmationService {
            compute_confirmation_thread,
        })
    }
}

impl Service for ComputeLeaderConfirmationService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.compute_confirmation_thread.join()
    }
}

#[cfg(test)]
pub mod tests {
    use crate::bank::Bank;
    use crate::compute_leader_confirmation_service::ComputeLeaderConfirmationService;
    use crate::vote_signer_proxy::VoteSignerProxy;

    use crate::genesis_block::GenesisBlock;
    use crate::leader_scheduler::tests::new_vote_account;
    use bincode::serialize;
    use solana_sdk::hash::hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::Transaction;
    use solana_sdk::vote_transaction::VoteTransaction;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_compute_confirmation() {
        solana_logger::setup();

        let (genesis_block, mint_keypair) = GenesisBlock::new(1234);
        let dummy_leader_id = Keypair::new().pubkey();
        let bank = Arc::new(Bank::new(&genesis_block));
        // generate 10 validators, but only vote for the first 6 validators
        let ids: Vec<_> = (0..10)
            .map(|i| {
                let last_id = hash(&serialize(&i).unwrap()); // Unique hash
                bank.register_tick(&last_id);
                // sleep to get a different timestamp in the bank
                sleep(Duration::from_millis(1));
                last_id
            })
            .collect();

        // Create a total of 10 vote accounts, each will have a balance of 1 (after giving 1 to
        // their vote account), for a total staking pool of 10 tokens.
        let vote_accounts: Vec<_> = (0..10)
            .map(|i| {
                // Create new validator to vote
                let validator_keypair = Arc::new(Keypair::new());
                let last_id = ids[i];
                let vote_signer = VoteSignerProxy::new_local(&validator_keypair);

                // Give the validator some tokens
                bank.transfer(2, &mint_keypair, validator_keypair.pubkey(), last_id)
                    .unwrap();
                new_vote_account(&validator_keypair, &vote_signer, &bank, 1, last_id);

                if i < 6 {
                    let vote_tx = Transaction::vote_new(&vote_signer, (i + 1) as u64, last_id, 0);
                    bank.process_transaction(&vote_tx).unwrap();
                }
                (vote_signer, validator_keypair)
            })
            .collect();

        // There isn't 2/3 consensus, so the bank's confirmation value should be the default
        let mut last_confirmation_time = 0;
        ComputeLeaderConfirmationService::compute_confirmation(
            &bank,
            dummy_leader_id,
            &mut last_confirmation_time,
        );
        assert_eq!(bank.confirmation_time(), std::usize::MAX);

        // Get another validator to vote, so we now have 2/3 consensus
        let vote_signer = &vote_accounts[7].0;
        let vote_tx = Transaction::vote_new(vote_signer, 7, ids[6], 0);
        bank.process_transaction(&vote_tx).unwrap();

        ComputeLeaderConfirmationService::compute_confirmation(
            &bank,
            dummy_leader_id,
            &mut last_confirmation_time,
        );
        assert!(bank.confirmation_time() != std::usize::MAX);
        assert!(last_confirmation_time > 0);
    }
}
