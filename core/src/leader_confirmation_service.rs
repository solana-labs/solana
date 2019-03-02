//! The `leader_confirmation_service` module implements the tools necessary
//! to generate a thread which regularly calculates the last confirmation times
//! observed by the leader

use crate::service::Service;
use solana_metrics::{influxdb, submit};
use solana_runtime::bank::Bank;
use solana_sdk::timing;
use solana_vote_api::vote_state::VoteState;
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

pub struct LeaderConfirmationService {
    thread_hdl: JoinHandle<()>,
}

impl LeaderConfirmationService {
    fn get_last_supermajority_timestamp(
        bank: &Arc<Bank>,
        last_valid_validator_timestamp: u64,
    ) -> result::Result<u64, ConfirmationError> {
        let mut total_stake = 0;
        let mut slots_and_stakes: Vec<(u64, u64)> = vec![];
        // Hold an accounts_db read lock as briefly as possible, just long enough to collect all
        // the vote states
        bank.vote_accounts().for_each(|(_, account)| {
            total_stake += account.tokens;
            let vote_state = VoteState::deserialize(&account.userdata).unwrap();
            if let Some(stake_and_state) = vote_state
                .votes
                .back()
                .map(|vote| (vote.slot_height, account.tokens))
            {
                slots_and_stakes.push(stake_and_state);
            }
        });

        let super_majority_stake = (2 * total_stake) / 3;

        if let Some(last_valid_validator_timestamp) =
            bank.get_confirmation_timestamp(slots_and_stakes, super_majority_stake)
        {
            return Ok(last_valid_validator_timestamp);
        }

        if last_valid_validator_timestamp != 0 {
            let now = timing::timestamp();
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

    pub fn compute_confirmation(bank: &Arc<Bank>, last_valid_validator_timestamp: &mut u64) {
        if let Ok(super_majority_timestamp) =
            Self::get_last_supermajority_timestamp(bank, *last_valid_validator_timestamp)
        {
            let now = timing::timestamp();
            let confirmation_ms = now - super_majority_timestamp;

            *last_valid_validator_timestamp = super_majority_timestamp;

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

    /// Create a new LeaderConfirmationService for computing confirmation.
    pub fn new(bank: &Arc<Bank>, exit: Arc<AtomicBool>) -> Self {
        let bank = bank.clone();
        let thread_hdl = Builder::new()
            .name("solana-leader-confirmation-service".to_string())
            .spawn(move || {
                let mut last_valid_validator_timestamp = 0;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    Self::compute_confirmation(&bank, &mut last_valid_validator_timestamp);
                    sleep(Duration::from_millis(COMPUTE_CONFIRMATION_MS));
                }
            })
            .unwrap();

        Self { thread_hdl }
    }
}

impl Service for LeaderConfirmationService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voting_keypair::tests::{new_vote_account, push_vote};
    use crate::voting_keypair::VotingKeypair;
    use bincode::serialize;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_vote_api::vote_transaction::VoteTransaction;
    use std::sync::Arc;

    #[test]
    fn test_compute_confirmation() {
        solana_logger::setup();

        let (genesis_block, mint_keypair) = GenesisBlock::new(1234);
        let bank = Arc::new(Bank::new(&genesis_block));

        // Move the bank up 10 slots
        let mut tick_hash = genesis_block.hash();
        while bank.slot_height() < 10 {
            tick_hash = hash(&serialize(&tick_hash).unwrap());
            bank.register_tick(&tick_hash);
        }
        let blockhash = bank.last_blockhash();

        // Create a total of 10 vote accounts, each will have a balance of 1 (after giving 1 to
        // their vote account), for a total staking pool of 10 tokens.
        let vote_accounts: Vec<_> = (0..10)
            .map(|i| {
                // Create new validator to vote
                let validator_keypair = Arc::new(Keypair::new());
                let voting_keypair = VotingKeypair::new_local(&validator_keypair);
                let voting_pubkey = voting_keypair.pubkey();

                // Give the validator some tokens
                bank.transfer(2, &mint_keypair, validator_keypair.pubkey(), blockhash)
                    .unwrap();
                new_vote_account(&validator_keypair, &voting_pubkey, &bank, 1);

                if i < 6 {
                    push_vote(&voting_keypair, &bank, (i + 1) as u64);
                }
                (voting_keypair, validator_keypair)
            })
            .collect();

        // There isn't 2/3 consensus, so the bank's confirmation value should be the default
        let mut last_confirmation_time = 0;
        LeaderConfirmationService::compute_confirmation(&bank, &mut last_confirmation_time);
        assert_eq!(last_confirmation_time, 0);

        // Get another validator to vote, so we now have 2/3 consensus
        let voting_keypair = &vote_accounts[7].0;
        let vote_tx = VoteTransaction::new_vote(voting_keypair, 7, blockhash, 0);
        bank.process_transaction(&vote_tx).unwrap();

        LeaderConfirmationService::compute_confirmation(&bank, &mut last_confirmation_time);
        assert!(last_confirmation_time > 0);
    }
}
