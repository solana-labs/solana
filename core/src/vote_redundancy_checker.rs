use log::*;
use solana_runtime::{bank::Bank, transaction_batch::TransactionBatch};
use solana_sdk::{pubkey::Pubkey, transaction::TransactionError};
use solana_vote_program::{vote_instruction::VoteError, vote_state::Vote, vote_transaction};
use std::borrow::Cow;

pub fn check_redundant_votes<'a, 'b>(
    bank: &'a Bank,
    batch: &'b TransactionBatch,
) -> TransactionBatch<'a, 'b> {
    let sanitized_txs = batch.sanitized_transactions();
    let check_results = sanitized_txs
        .iter()
        .zip(batch.lock_results().to_vec())
        .map(|(tx, lock_res)| match lock_res {
            Ok(()) => {
                if let Some((vote_account_pubkey, vote, _vote_switch_to_slot_hash)) =
                    vote_transaction::parse_sanitized_vote_transaction(tx)
                {
                    debug!(
                        "tx {:?} parsed into vote {:?}, vote account key {}",
                        tx, vote, vote_account_pubkey
                    );
                    inc_new_counter_info!("bank-process_vote_transactions", 1);

                    if is_redundant_by_vote_state(bank, &vote_account_pubkey, &vote) {
                        inc_new_counter_info!("bank-process_redundant_vote_transactions", 1);
                        info!("TEST_ vote {:?} is redundant for slot {} vote account {:?}", vote, bank.slot(),vote_account_pubkey );
                        Err(TransactionError::AlreadyProcessed)
                    } else {
                        info!("TEST_ vote {:?} is good for slot {} vote account {:?}", vote, bank.slot(),vote_account_pubkey );
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(e),
        })
        .collect();
    TransactionBatch::new(check_results, bank, Cow::Borrowed(sanitized_txs))
}

fn is_redundant_by_vote_state(bank: &Bank, vote_account_pubkey: &Pubkey, vote: &Vote) -> bool {
    // ignore vote without slot, or slot '0' (during startup)
    let last_slot = match vote.slots.last() {
        None => {
            debug!(
                "Vote has no slots, skip checking redundancy for vote {:?}",
                vote
            );
            return false;
        }
        Some(slot) => {
            if 0 == *slot {
                debug!("Vote slot 0, skip checking redundancy for vote {:?}", vote);
                return false;
            }
            slot
        }
    };

    let vote_account = match bank.get_vote_account(vote_account_pubkey) {
        None => {
            warn!(
                "Vote account {} does not exist, skip checking redundancy for vote {:?}",
                vote_account_pubkey, vote
            );
            return false;
        }
        Some((_stake, vote_account)) => vote_account,
    };

    let vote_state = vote_account.vote_state();
    let vote_state = match vote_state.as_ref() {
        Err(_) => {
            warn!(
                "Vote account {} is unreachable, skip checking redundancy for vote {:?}",
                vote_account_pubkey, vote
            );
            return false;
        }
        Ok(vote_state) => vote_state,
    };

    match vote_state.check_slots_are_valid(vote, &[(*last_slot, vote.hash)]) {
        Err(VoteError::VoteTooOld) => {
            debug!(
                "Vote {:?} by vote account {} is redundant",
                vote, vote_account_pubkey
            );
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vote_simulator::VoteSimulator;
    use solana_sdk::{genesis_config::create_genesis_config, hash::Hash};
    use std::{collections::HashMap, sync::Arc};
    use trees::notation::tr;

    #[test]
    fn test_empty_vote_is_not_checked() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let empty_vote = Vote::new(vec![], Hash::default());
        let vote_account_pubkey = Pubkey::new_unique();

        // first check, should pass - not redundant
        assert!(!is_redundant_by_vote_state(
            &bank,
            &vote_account_pubkey,
            &empty_vote
        ));
    }

    #[test]
    fn test_slot0_vote_is_not_checked() {
        let (genesis_config, _mint_keypair) = create_genesis_config(1);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let empty_vote = Vote::new(vec![0], Hash::default());
        let vote_account_pubkey = Pubkey::new_unique();

        // first check, should pass - not redundant
        assert!(!is_redundant_by_vote_state(
            &bank,
            &vote_account_pubkey,
            &empty_vote
        ));
    }

    #[test]
    fn test_vote_redundancy() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(1);
        let my_node_pubkey = vote_simulator.node_pubkeys[0];
        let my_vote_pubkey = vote_simulator.vote_pubkeys[0];

        // Create the tree of banks in a BankForks object
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3))));

        // Setup votes for slot 0 and 1
        {
            let mut cluster_votes = HashMap::new();
            let votes = vec![0, 1];
            cluster_votes.insert(my_node_pubkey, votes);
            vote_simulator.fill_bank_forks(forks, &cluster_votes, true);
        }

        let bank1 = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(1)
            .unwrap()
            .clone();
        let vote1 = Vote::new(vec![1], bank1.hash());

        let bank2 = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(2)
            .unwrap()
            .clone();
        let vote2 = Vote::new(vec![2], bank2.hash());

        let bank3 = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .get(3)
            .unwrap()
            .clone();

        // for bank3, vote for bank1 is redundant, vote for bank2 is not
        assert!(is_redundant_by_vote_state(&bank3, &my_vote_pubkey, &vote1));
        assert!(!is_redundant_by_vote_state(&bank3, &my_vote_pubkey, &vote2));
    }
}
