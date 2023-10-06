use {
    crate::{
        bank::Bank,
        genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs},
    },
    solana_accounts_db::transaction_results::TransactionResults,
    solana_sdk::{pubkey::Pubkey, signature::Signer, transaction::SanitizedTransaction},
    solana_vote::{vote_parser, vote_sender_types::ReplayVoteSender},
};

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
                if tx.is_simple_vote_transaction() && result.was_executed_successfully() {
                    if let Some(parsed_vote) = vote_parser::parse_sanitized_vote_transaction(tx) {
                        if parsed_vote.1.last_voted_slot().is_some() {
                            let _ = vote_sender.send(parsed_vote);
                        }
                    }
                }
            });
    }
}
