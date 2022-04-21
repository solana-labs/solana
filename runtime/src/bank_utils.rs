use {
    crate::{
        bank::{Bank, TransactionResults},
        genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs},
        vote_parser,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{
        pubkey::Pubkey, signature::Signer, timing::AtomicInterval,
        transaction::SanitizedTransaction,
    },
    std::sync::atomic::{AtomicUsize, Ordering},
};

// report vote instruction processing stats every 2 seconds
const VOTE_INSTRUCITON_PROCESSING_STATS_REPORT_INTERVAL_MS: u64 = 2000;

#[derive(Debug, Default)]
pub struct VoteInstructionProcessingStats {
    last_report: AtomicInterval,
    vote_native_count: AtomicUsize,
    vote_state_native_count: AtomicUsize,
}

impl VoteInstructionProcessingStats {
    fn is_empty(&self) -> bool {
        0 == self.vote_native_count.load(Ordering::Relaxed) as u64
            && 0 == self.vote_state_native_count.load(Ordering::Relaxed) as u64
    }

    pub fn inc_vote_native(&self) {
        self.vote_native_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_vote_state_native(&self) {
        self.vote_state_native_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn report(&self, report_interval_ms: u64) {
        // skip reporting metrics if stats is empty
        if self.is_empty() {
            return;
        }
        if self.last_report.should_update(report_interval_ms) {
            datapoint_info!(
                "vote_instruction_processing_stats",
                (
                    "vote_native",
                    self.vote_native_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
                (
                    "vote_state_native",
                    self.vote_state_native_count.swap(0, Ordering::Relaxed) as i64,
                    i64
                ),
            );
        }
    }
}

lazy_static! {
    static ref VOTE_INSTRUCTION_PROCESSING_STATS: VoteInstructionProcessingStats =
        VoteInstructionProcessingStats::default();
}

pub fn setup_bank_and_vote_pubkeys_for_tests(
    num_vote_accounts: usize,
    stake: u64,
) -> (Bank, Vec<Pubkey>) {
    // Create some voters at genesis
    let validator_voting_keypairs: Vec<_> = (0..num_vote_accounts)
        .map(|_| ValidatorVoteKeypairs::new_rand())
        .collect();

    let vote_pubkeys: Vec<_> = validator_voting_keypairs
        .iter()
        .map(|k| k.vote_keypair.pubkey())
        .collect();
    let GenesisConfigInfo { genesis_config, .. } =
        genesis_utils::create_genesis_config_with_vote_accounts(
            10_000,
            &validator_voting_keypairs,
            vec![stake; validator_voting_keypairs.len()],
        );
    let bank = Bank::new_for_tests(&genesis_config);
    (bank, vote_pubkeys)
}

pub fn find_and_send_votes(
    sanitized_txs: &[SanitizedTransaction],
    tx_results: &TransactionResults,
    vote_sender: Option<&ReplayVoteSender>,
) {
    let TransactionResults {
        execution_results, ..
    } = tx_results;
    if let Some(vote_sender) = vote_sender {
        sanitized_txs
            .iter()
            .zip(execution_results.iter())
            .for_each(|(tx, result)| {
                // TODO:
                //         VoteInstruction::Vote(vote) | VoteInstruction::VoteSwitch(vote, _)
                //         VoteInstruction::UpdateVoteState(vote_state_update) | VoteInstruction::UpdateVoteStateSwitch(vote_state_update, _)
                if tx.is_simple_vote_transaction() && result.was_executed_successfully() {
                    VOTE_INSTRUCTION_PROCESSING_STATS.inc_vote_native();

                    if let Some(parsed_vote) = vote_parser::parse_sanitized_vote_transaction(tx) {
                        if parsed_vote.1.last_voted_slot().is_some() {
                            let _ = vote_sender.send(parsed_vote);
                        }
                    }
                }
            });
    }

    VOTE_INSTRUCTION_PROCESSING_STATS.report(VOTE_INSTRUCITON_PROCESSING_STATS_REPORT_INTERVAL_MS);
}
