use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_program::solana_system_program;

#[macro_use]
extern crate solana_bpf_loader_program;
#[macro_use]
extern crate solana_budget_program;
#[macro_use]
extern crate solana_config_program;
#[macro_use]
extern crate solana_exchange_program;
#[cfg(feature = "move")]
#[macro_use]
extern crate solana_move_loader_program;
#[macro_use]
extern crate solana_stake_program;
#[macro_use]
extern crate solana_storage_program;
#[macro_use]
extern crate solana_token_program;
#[macro_use]
extern crate solana_vote_program;

pub fn get() -> Vec<(String, Pubkey)> {
    vec![
        solana_system_program(),
        solana_bpf_loader_program!(),
        solana_budget_program!(),
        solana_config_program!(),
        solana_exchange_program!(),
        #[cfg(feature = "move")]
        solana_move_loader_program!(),
        solana_stake_program!(),
        solana_storage_program!(),
        solana_token_program!(),
        solana_vote_program!(),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    #[test]
    fn test_id_uniqueness() {
        let mut unique = HashSet::new();
        let ids = vec![
            solana_budget_api::id(),
            solana_config_api::id(),
            solana_exchange_api::id(),
            #[cfg(feature = "move")]
            solana_move_loader_api::id(),
            solana_sdk::bpf_loader::id(),
            solana_sdk::native_loader::id(),
            solana_sdk::system_program::id(),
            solana_stake_api::id(),
            solana_storage_api::id(),
            solana_token_api::id(),
            solana_vote_api::id(),
        ];
        assert!(ids.into_iter().all(move |id| unique.insert(id)));
    }
}
