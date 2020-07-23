use crate::{
    bank::Bank,
    genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs},
};
use solana_sdk::{pubkey::Pubkey, signature::Signer};

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
