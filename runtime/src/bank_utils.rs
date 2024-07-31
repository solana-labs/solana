use {
    crate::vote_sender_types::ReplayVoteSender,
    solana_sdk::transaction::SanitizedTransaction,
    solana_svm::transaction_commit_result::{
        TransactionCommitResult, TransactionCommitResultExtensions,
    },
    solana_vote::vote_parser,
};
#[cfg(feature = "dev-context-only-utils")]
use {
    crate::{
        bank::Bank,
        genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs},
    },
    solana_sdk::{pubkey::Pubkey, signature::Signer},
};

#[cfg(feature = "dev-context-only-utils")]
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
    commit_results: &[TransactionCommitResult],
    vote_sender: Option<&ReplayVoteSender>,
) {
    if let Some(vote_sender) = vote_sender {
        sanitized_txs
            .iter()
            .zip(commit_results.iter())
            .for_each(|(tx, commit_result)| {
                if tx.is_simple_vote_transaction() && commit_result.was_executed_successfully() {
                    if let Some(parsed_vote) = vote_parser::parse_sanitized_vote_transaction(tx) {
                        if parsed_vote.1.last_voted_slot().is_some() {
                            let _ = vote_sender.send(parsed_vote);
                        }
                    }
                }
            });
    }
}
