use {
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
pub enum BufferedPacketsDecision {
    Consume(BankStart),
    Forward,
    ForwardAndHold,
    Hold,
}

impl BufferedPacketsDecision {
    /// Returns the `BankStart` if the decision is `Consume`. Otherwise, returns `None`.
    pub fn bank_start(&self) -> Option<&BankStart> {
        match self {
            Self::Consume(bank_start) => Some(bank_start),
            _ => None,
        }
    }
}

pub struct DecisionMaker {
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

    pub(crate) fn make_consume_or_forward_decision(&self) -> BufferedPacketsDecision {
        let decision;
        {
            let poh_recorder = self.poh_recorder.read().unwrap();
            decision = Self::consume_or_forward_packets(
                &self.my_pubkey,
                || Self::bank_start(&poh_recorder),
                || Self::would_be_leader_shortly(&poh_recorder),
                || Self::would_be_leader(&poh_recorder),
                || Self::leader_pubkey(&poh_recorder),
            );
        }

        decision
    }

    fn consume_or_forward_packets(
        my_pubkey: &Pubkey,
        bank_start_fn: impl FnOnce() -> Option<BankStart>,
        would_be_leader_shortly_fn: impl FnOnce() -> bool,
        would_be_leader_fn: impl FnOnce() -> bool,
        leader_pubkey_fn: impl FnOnce() -> Option<Pubkey>,
    ) -> BufferedPacketsDecision {
        // If has active bank, then immediately process buffered packets
        // otherwise, based on leader schedule to either forward or hold packets
        if let Some(bank_start) = bank_start_fn() {
            // If the bank is available, this node is the leader
            BufferedPacketsDecision::Consume(bank_start)
        } else if would_be_leader_shortly_fn() {
            // If the node will be the leader soon, hold the packets for now
            BufferedPacketsDecision::Hold
        } else if would_be_leader_fn() {
            // Node will be leader within ~20 slots, hold the transactions in
            // case it is the only node which produces an accepted slot.
            BufferedPacketsDecision::ForwardAndHold
        } else if let Some(x) = leader_pubkey_fn() {
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

    fn bank_start(poh_recorder: &PohRecorder) -> Option<BankStart> {
        poh_recorder
            .bank_start()
            .filter(|bank_start| bank_start.should_working_bank_still_be_processing_txs())
    }

    fn would_be_leader_shortly(poh_recorder: &PohRecorder) -> bool {
        poh_recorder.would_be_leader(
            (FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET - 1) * DEFAULT_TICKS_PER_SLOT,
        )
    }

    fn would_be_leader(poh_recorder: &PohRecorder) -> bool {
        poh_recorder.would_be_leader(HOLD_TRANSACTIONS_SLOT_OFFSET * DEFAULT_TICKS_PER_SLOT)
    }

    fn leader_pubkey(poh_recorder: &PohRecorder) -> Option<Pubkey> {
        poh_recorder.leader_after_n_slots(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        core::panic,
        solana_ledger::{blockstore::Blockstore, genesis_utils::create_genesis_config},
        solana_poh::poh_recorder::create_test_recorder,
        solana_runtime::bank::Bank,
        solana_sdk::clock::NUM_CONSECUTIVE_LEADER_SLOTS,
        std::{
            env::temp_dir,
            sync::{atomic::Ordering, Arc},
            time::Instant,
        },
    };

    #[test]
    fn test_buffered_packet_decision_bank_start() {
        let bank = Arc::new(Bank::default_for_tests());
        let bank_start = BankStart {
            working_bank: bank,
            bank_creation_time: Arc::new(Instant::now()),
        };
        assert!(BufferedPacketsDecision::Consume(bank_start)
            .bank_start()
            .is_some());
        assert!(BufferedPacketsDecision::Forward.bank_start().is_none());
        assert!(BufferedPacketsDecision::ForwardAndHold
            .bank_start()
            .is_none());
        assert!(BufferedPacketsDecision::Hold.bank_start().is_none());
    }

    #[test]
    fn test_make_consume_or_forward_decision() {
        let genesis_config = create_genesis_config(2).genesis_config;
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let ledger_path = temp_dir();
        let blockstore = Arc::new(Blockstore::open(ledger_path.as_path()).unwrap());
        let (exit, poh_recorder, poh_service, _entry_receiver) =
            create_test_recorder(bank.clone(), blockstore, None, None);
        // Drop the poh service immediately to avoid potential ticking
        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();

        let my_pubkey = Pubkey::new_unique();
        let decision_maker = DecisionMaker::new(my_pubkey, poh_recorder.clone());
        poh_recorder.write().unwrap().reset(bank.clone(), None);
        let slot = bank.slot() + 1;
        let bank = Arc::new(Bank::new_from_parent(bank, &my_pubkey, slot));

        // Currently Leader - Consume
        {
            poh_recorder.write().unwrap().set_bank(bank.clone(), false);
            let decision = decision_maker.make_consume_or_forward_decision();
            assert_matches!(decision, BufferedPacketsDecision::Consume(_));
        }

        // Will be leader shortly - Hold
        for next_leader_slot_offset in [0, 1].into_iter() {
            let next_leader_slot = bank.slot() + next_leader_slot_offset;
            poh_recorder.write().unwrap().reset(
                bank.clone(),
                Some((
                    next_leader_slot,
                    next_leader_slot + NUM_CONSECUTIVE_LEADER_SLOTS,
                )),
            );
            let decision = decision_maker.make_consume_or_forward_decision();
            assert!(
                matches!(decision, BufferedPacketsDecision::Hold),
                "next_leader_slot_offset: {next_leader_slot_offset}",
            );
        }

        // Will be leader - ForwardAndHold
        for next_leader_slot_offset in [2, 19].into_iter() {
            let next_leader_slot = bank.slot() + next_leader_slot_offset;
            poh_recorder.write().unwrap().reset(
                bank.clone(),
                Some((
                    next_leader_slot,
                    next_leader_slot + NUM_CONSECUTIVE_LEADER_SLOTS + 1,
                )),
            );
            let decision = decision_maker.make_consume_or_forward_decision();
            assert!(
                matches!(decision, BufferedPacketsDecision::ForwardAndHold),
                "next_leader_slot_offset: {next_leader_slot_offset}",
            );
        }

        // Known leader, not me - Forward
        {
            poh_recorder.write().unwrap().reset(bank, None);
            let decision = decision_maker.make_consume_or_forward_decision();
            assert_matches!(decision, BufferedPacketsDecision::Forward);
        }
    }

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
                || bank_start.clone(),
                || panic!("should not be called"),
                || panic!("should not be called"),
                || panic!("should not be called")
            ),
            BufferedPacketsDecision::Consume(_)
        );
        // Unknown leader, hold the packets
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                || None,
                || false,
                || false,
                || None
            ),
            BufferedPacketsDecision::Hold
        );
        // Leader other than me, forward the packets
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                || None,
                || false,
                || false,
                || Some(my_pubkey1),
            ),
            BufferedPacketsDecision::Forward
        );
        // Will be leader shortly, hold the packets
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                || None,
                || true,
                || panic!("should not be called"),
                || panic!("should not be called"),
            ),
            BufferedPacketsDecision::Hold
        );
        // Will be leader (not shortly), forward and hold
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey,
                || None,
                || false,
                || true,
                || panic!("should not be called"),
            ),
            BufferedPacketsDecision::ForwardAndHold
        );
        // Current leader matches my pubkey, hold
        assert_matches!(
            DecisionMaker::consume_or_forward_packets(
                &my_pubkey1,
                || None,
                || false,
                || false,
                || Some(my_pubkey1),
            ),
            BufferedPacketsDecision::Hold
        );
    }
}
