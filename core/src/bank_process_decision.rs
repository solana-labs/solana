//! Separate the decision of how to process packets from banking stage. This decision will
//! eventually be made in a scheduling stage that will pass a list of packets and decision
//! to the banking stage.
//!

use {
    solana_poh::poh_recorder::PohRecorder,
    solana_runtime::bank::Bank,
    solana_sdk::{clock::DEFAULT_TICKS_PER_SLOT, pubkey::Pubkey},
    std::sync::{Arc, RwLock},
};

/// Transaction forwarding
pub const FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET: u64 = 2;
pub const HOLD_TRANSACTIONS_SLOT_OFFSET: u64 = 20;

/// Convenience struct for determining how a bank should process packets.
pub struct BankingDecisionMaker {
    poh_recorder: Arc<RwLock<PohRecorder>>,
    my_pubkey: Pubkey,
}

#[derive(Debug, Clone)]
pub enum BankPacketProcessingDecision {
    /// Active bank - can immediately process packets. Maximum number of nanoseconds to process for.
    Consume(u128),
    /// Forward to the leader without holding. No expecation that this node will become the leader soon.
    Forward,
    /// Forward packets but hold on to them as they may be needed when this node becomes leader.
    ForwardAndHold,
    /// Do nothing because:
    /// 1. Is the leader but no active bank is available, or
    /// 2. The leader is unknown.
    Hold,
}

impl BankingDecisionMaker {
    pub fn new(poh_recorder: Arc<RwLock<PohRecorder>>, my_pubkey: Pubkey) -> Self {
        Self {
            poh_recorder,
            my_pubkey,
        }
    }

    /// Determine how to process packets.
    pub fn make_decision(&self) -> BankPacketProcessingDecision {
        let poh = self.poh_recorder.read().unwrap();
        let bank_start = poh.bank_start();

        let bank_still_processing_txs =
            PohRecorder::get_working_bank_if_not_expired(&bank_start.as_ref());
        let leader_at_slot_offset =
            poh.leader_after_n_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET);
        let would_be_leader =
            poh.would_be_leader(HOLD_TRANSACTIONS_SLOT_OFFSET * DEFAULT_TICKS_PER_SLOT);
        let would_be_leader_shortly = poh.would_be_leader(
            (FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET - 1) * DEFAULT_TICKS_PER_SLOT,
        );

        Self::make_decision_from_state(
            &self.my_pubkey,
            leader_at_slot_offset,
            bank_still_processing_txs,
            would_be_leader,
            would_be_leader_shortly,
        )
    }

    fn make_decision_from_state(
        my_pubkey: &Pubkey,
        leader_at_slot_offset: Option<Pubkey>,
        bank_still_processing_txs: Option<&Arc<Bank>>,
        would_be_leader: bool,
        would_be_leader_shortly: bool,
    ) -> BankPacketProcessingDecision {
        // If has active bank, then immediately process buffered packets
        // otherwise, based on leader schedule to either forward or hold packets
        if let Some(bank) = bank_still_processing_txs {
            // If the bank is available, this node is the leader
            BankPacketProcessingDecision::Consume(bank.ns_per_slot)
        } else if would_be_leader_shortly {
            // If the node will be the leader soon, hold the packets for now
            BankPacketProcessingDecision::Hold
        } else if would_be_leader {
            // Node will be leader within ~20 slots, hold the transactions in
            // case it is the only node which produces an accepted slot.
            BankPacketProcessingDecision::ForwardAndHold
        } else if let Some(x) = leader_at_slot_offset {
            if x != *my_pubkey {
                // If the current node is not the leader, forward the buffered packets
                BankPacketProcessingDecision::Forward
            } else {
                // If the current node is the leader, return the buffered packets as is
                BankPacketProcessingDecision::Hold
            }
        } else {
            // We don't know the leader. Hold the packets for now
            BankPacketProcessingDecision::Hold
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_decision_from_state() {
        let my_pubkey = solana_sdk::pubkey::new_rand();
        let my_pubkey1 = solana_sdk::pubkey::new_rand();
        let bank = Arc::new(Bank::default_for_tests());
        // having active bank allows to consume immediately
        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(
                &my_pubkey,
                None,
                Some(&bank),
                false,
                false
            ),
            BankPacketProcessingDecision::Consume(_)
        );
        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(&my_pubkey, None, None, false, false),
            BankPacketProcessingDecision::Hold
        );
        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(&my_pubkey1, None, None, false, false),
            BankPacketProcessingDecision::Hold
        );

        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                false,
                false
            ),
            BankPacketProcessingDecision::Forward
        );

        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                true,
                true
            ),
            BankPacketProcessingDecision::Hold
        );
        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                true,
                false
            ),
            BankPacketProcessingDecision::ForwardAndHold
        );
        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(
                &my_pubkey,
                Some(my_pubkey1),
                Some(&bank),
                false,
                false
            ),
            BankPacketProcessingDecision::Consume(_)
        );
        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(
                &my_pubkey1,
                Some(my_pubkey1),
                None,
                false,
                false
            ),
            BankPacketProcessingDecision::Hold
        );
        assert_matches!(
            BankingDecisionMaker::make_decision_from_state(
                &my_pubkey1,
                Some(my_pubkey1),
                Some(&bank),
                false,
                false
            ),
            BankPacketProcessingDecision::Consume(_)
        );
    }
}
