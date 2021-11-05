use crate::{
    bank::{Bank, TransactionResults},
    genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs},
    hashed_transaction::HashedTransaction,
    vote_sender_types::ReplayVoteSender,
};
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use solana_vote_program::vote_transaction;

pub fn setup_bank_and_vote_pubkeys(num_vote_accounts: usize, stake: u64) -> (Bank, Vec<Pubkey>) {
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
    let bank = Bank::new(&genesis_config);
    (bank, vote_pubkeys)
}

pub fn find_and_send_votes(
    hashed_txs: &[HashedTransaction],
    tx_results: &TransactionResults,
    vote_sender: Option<&ReplayVoteSender>,
) {
    let TransactionResults {
        execution_results, ..
    } = tx_results;
    if let Some(vote_sender) = vote_sender {
        hashed_txs.iter().zip(execution_results.iter()).for_each(
            |(hashed_tx, (result, _nonce_rollback))| {
                let transaction = hashed_tx.transaction();
                if hashed_tx.is_simple_vote_transaction() && result.is_ok() {
                    if let Some(parsed_vote) = vote_transaction::parse_vote_transaction(transaction)
                    {
                        if parsed_vote.1.slots.last().is_some() {
                            let _ = vote_sender.send(parsed_vote);
                        }
                    }
                }
            },
        );
    }
}
