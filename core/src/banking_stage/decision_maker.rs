use {
    crate::leader_slot_banking_stage_metrics::{LeaderSlotMetricsTracker, MetricsTrackerAction},
    solana_poh::poh_recorder::{BankStart, PohRecorder},
    solana_sdk::{
        clock::{
            DEFAULT_TICKS_PER_SLOT, FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET,
            HOLD_TRANSACTIONS_SLOT_OFFSET,
        },
        pubkey::Pubkey,
    },
    std::sync::{Arc, RwLock},
};

#[derive(Debug, Clone)]
pub(crate) enum BufferedPacketsDecision {
    Consume(BankStart),
    Forward,
    ForwardAndHold,
    Hold,
}

pub(crate) struct DecisionMaker {
    my_pubkey: Pubkey,
    poh_recorder: Arc<RwLock<PohRecorder>>,
}

impl DecisionMaker {
    pub fn new(my_pubkey: Pubkey, poh_recorder: Arc<RwLock<PohRecorder>>) -> Self {
        Self {
            my_pubkey,
            poh_recorder,
        }
    }

    pub fn make_consume_or_forward_decision(
        &self,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    ) -> (MetricsTrackerAction, BufferedPacketsDecision) {
        let (leader_at_slot_offset, bank_start, would_be_leader, would_be_leader_shortly) = {
            let poh = self.poh_recorder.read().unwrap();
            let bank_start = poh
                .bank_start()
                .filter(|bank_start| bank_start.should_working_bank_still_be_processing_txs());
            (
                poh.leader_after_n_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET),
                bank_start,
                poh.would_be_leader(HOLD_TRANSACTIONS_SLOT_OFFSET * DEFAULT_TICKS_PER_SLOT),
                poh.would_be_leader(
                    (FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET - 1) * DEFAULT_TICKS_PER_SLOT,
                ),
            )
        };

        (
            slot_metrics_tracker.check_leader_slot_boundary(&bank_start),
            Self::consume_or_forward_packets(
                &self.my_pubkey,
                leader_at_slot_offset,
                bank_start,
                would_be_leader,
                would_be_leader_shortly,
            ),
        )
    }

    fn consume_or_forward_packets(
        my_pubkey: &Pubkey,
        leader_pubkey: Option<Pubkey>,
        bank_start: Option<BankStart>,
        would_be_leader: bool,
        would_be_leader_shortly: bool,
    ) -> BufferedPacketsDecision {
        // If has active bank, then immediately process buffered packets
        // otherwise, based on leader schedule to either forward or hold packets
        if let Some(bank_start) = bank_start {
            // If the bank is available, this node is the leader
            BufferedPacketsDecision::Consume(bank_start)
        } else if would_be_leader_shortly {
            // If the node will be the leader soon, hold the packets for now
            BufferedPacketsDecision::Hold
        } else if would_be_leader {
            // Node will be leader within ~20 slots, hold the transactions in
            // case it is the only node which produces an accepted slot.
            BufferedPacketsDecision::ForwardAndHold
        } else if let Some(x) = leader_pubkey {
            if x != *my_pubkey {
                // If the current node is not the leader, forward the buffered packets
                BufferedPacketsDecision::Forward
            } else {
                // If the current node is the leader, return the buffered packets as is
                BufferedPacketsDecision::Hold
            }
        } else {
            // We don't know the leader. Hold the packets for now
            BufferedPacketsDecision::Hold
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_runtime::bank::Bank,
        std::{sync::Arc, time::Instant},
    };

    #[test]
    fn test_should_process_or_forward_packets() {
        let my_pubkey = solana_sdk::pubkey::new_rand();
        let my_pubkey1 = solana_sdk::pubkey::new_rand();
        let bank = Arc::new(Bank::default_for_tests());
        let bank_start = Some(BankStart {
            working_bank: bank,
            bank_creation_time: Arc::new(Instant::now()),
        });
        // having active bank allows to consume immediately
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                None,
                bank_start.clone(),
                false,
                false
            ),
            BufferedPacketsDecision::Consume(_)
        );
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(&my_pubkey, None, None, false, false),
            BufferedPacketsDecision::Hold
        );
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(&my_pubkey1, None, None, false, false),
            BufferedPacketsDecision::Hold
        );

        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                false,
                false
            ),
            BufferedPacketsDecision::Forward
        );

        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                true,
                true
            ),
            BufferedPacketsDecision::Hold
        );
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                None,
                true,
                false
            ),
            BufferedPacketsDecision::ForwardAndHold
        );
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                Some(my_pubkey1),
                bank_start.clone(),
                false,
                false
            ),
            BufferedPacketsDecision::Consume(_)
        );
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey1,
                Some(my_pubkey1),
                None,
                false,
                false
            ),
            BufferedPacketsDecision::Hold
        );
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey1,
                Some(my_pubkey1),
                bank_start,
                false,
                false
            ),
            BufferedPacketsDecision::Consume(_)
        );
    }
}
