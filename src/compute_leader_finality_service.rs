//! The `compute_leader_finality_service` module implements the tools necessary
//! to generate a thread which regularly calculates the last finality times
//! observed by the leader

use bank::Bank;
use influx_db_client as influxdb;
use metrics;
use service::Service;
use std::result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use timing;
use vote_program::VoteProgram;

#[derive(Debug, PartialEq, Eq)]
pub enum FinalityError {
    NoValidSupermajority,
}

pub const COMPUTE_FINALITY_MS: u64 = 1000;

pub struct ComputeLeaderFinalityService {
    compute_finality_thread: JoinHandle<()>,
}

impl ComputeLeaderFinalityService {
    fn get_last_supermajority_timestamp(
        bank: &Arc<Bank>,
        now: u64,
        last_valid_validator_timestamp: u64,
    ) -> result::Result<u64, FinalityError> {
        let mut total_stake = 0;

        let mut ticks_and_stakes: Vec<(u64, u64)> = {
            let bank_accounts = bank.accounts.read().unwrap();
            // TODO: Doesn't account for duplicates since a single validator could potentially register
            // multiple vote accounts. Once that is no longer possible (see the TODO in vote_program.rs,
            // process_transaction(), case VoteInstruction::RegisterAccount), this will be more accurate.
            // See github issue 1654.
            bank_accounts
                .values()
                .filter_map(|account| {
                    // Filter out any accounts that don't belong to the VoteProgram
                    // by returning None
                    if VoteProgram::check_id(&account.program_id) {
                        if let Ok(vote_state) = VoteProgram::deserialize(&account.userdata) {
                            let validator_stake = bank.get_stake(&vote_state.node_id);
                            total_stake += validator_stake;
                            // Filter out any validators that don't have at least one vote
                            // by returning None
                            return vote_state
                                .votes
                                .back()
                                .map(|vote| (vote.tick_height, validator_stake));
                        }
                    }

                    None
                }).collect()
        };

        let super_majority_stake = (2 * total_stake) / 3;

        if let Some(last_valid_validator_timestamp) =
            bank.get_finality_timestamp(&mut ticks_and_stakes, super_majority_stake)
        {
            return Ok(last_valid_validator_timestamp);
        }

        if last_valid_validator_timestamp != 0 {
            metrics::submit(
                influxdb::Point::new(&"leader-finality")
                    .add_field(
                        "duration_ms",
                        influxdb::Value::Integer((now - last_valid_validator_timestamp) as i64),
                    ).to_owned(),
            );
        }

        Err(FinalityError::NoValidSupermajority)
    }

    pub fn compute_finality(bank: &Arc<Bank>, last_valid_validator_timestamp: &mut u64) {
        let now = timing::timestamp();
        if let Ok(super_majority_timestamp) =
            Self::get_last_supermajority_timestamp(bank, now, *last_valid_validator_timestamp)
        {
            let finality_ms = now - super_majority_timestamp;

            *last_valid_validator_timestamp = super_majority_timestamp;
            bank.set_finality((now - *last_valid_validator_timestamp) as usize);

            metrics::submit(
                influxdb::Point::new(&"leader-finality")
                    .add_field("duration_ms", influxdb::Value::Integer(finality_ms as i64))
                    .to_owned(),
            );
        }
    }

    /// Create a new ComputeLeaderFinalityService for computing finality.
    pub fn new(bank: Arc<Bank>, exit: Arc<AtomicBool>) -> Self {
        let compute_finality_thread = Builder::new()
            .name("solana-leader-finality-stage".to_string())
            .spawn(move || {
                let mut last_valid_validator_timestamp = 0;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    Self::compute_finality(&bank, &mut last_valid_validator_timestamp);
                    sleep(Duration::from_millis(COMPUTE_FINALITY_MS));
                }
            }).unwrap();

        (ComputeLeaderFinalityService {
            compute_finality_thread,
        })
    }
}

impl Service for ComputeLeaderFinalityService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.compute_finality_thread.join()
    }
}

#[cfg(test)]
pub mod tests {
    use bank::Bank;
    use bincode::serialize;
    use compute_leader_finality_service::ComputeLeaderFinalityService;
    use hash::hash;
    use logger;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use transaction::Transaction;
    use vote_program::Vote;
    use vote_transaction::{create_vote_account, VoteTransaction};

    #[test]
    fn test_compute_finality() {
        logger::setup();

        let mint = Mint::new(1234);
        let bank = Arc::new(Bank::new(&mint));
        // generate 10 validators, but only vote for the first 6 validators
        let ids: Vec<_> = (0..10)
            .map(|i| {
                let last_id = hash(&serialize(&i).unwrap()); // Unique hash
                bank.register_entry_id(&last_id);
                // sleep to get a different timestamp in the bank
                sleep(Duration::from_millis(1));
                last_id
            }).collect();

        // Create a total of 10 vote accounts, each will have a balance of 1 (after giving 1 to
        // their vote account), for a total staking pool of 10 tokens.
        let vote_accounts: Vec<_> = (0..10)
            .map(|i| {
                // Create new validator to vote
                let validator_keypair = Keypair::new();
                let last_id = ids[i];

                // Give the validator some tokens
                bank.transfer(2, &mint.keypair(), validator_keypair.pubkey(), last_id)
                    .unwrap();
                let vote_account = create_vote_account(&validator_keypair, &bank, 1, last_id)
                    .expect("Expected successful creation of account");

                if i < 6 {
                    let vote = Vote {
                        tick_height: (i + 1) as u64,
                    };
                    let vote_tx = Transaction::vote_new(&vote_account, vote, last_id, 0);
                    bank.process_transaction(&vote_tx).unwrap();
                }
                vote_account
            }).collect();

        // There isn't 2/3 consensus, so the bank's finality value should be the default
        let mut last_finality_time = 0;
        ComputeLeaderFinalityService::compute_finality(&bank, &mut last_finality_time);
        assert_eq!(bank.finality(), std::usize::MAX);

        // Get another validator to vote, so we now have 2/3 consensus
        let vote_account = &vote_accounts[7];
        let vote = Vote { tick_height: 7 };
        let vote_tx = Transaction::vote_new(&vote_account, vote, ids[6], 0);
        bank.process_transaction(&vote_tx).unwrap();

        ComputeLeaderFinalityService::compute_finality(&bank, &mut last_finality_time);
        assert!(bank.finality() != std::usize::MAX);
        assert!(last_finality_time > 0);
    }
}
