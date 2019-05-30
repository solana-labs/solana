use crate::solana_storage_program;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_storage_api::storage_contract;

pub trait GenesisBlockUtil {
    fn add_storage_program(&mut self, validator_storage_pubkey: &Pubkey);
}

impl GenesisBlockUtil for GenesisBlock {
    fn add_storage_program(&mut self, validator_storage_pubkey: &Pubkey) {
        self.accounts.push((
            *validator_storage_pubkey,
            storage_contract::create_validator_storage_account(1),
        ));
        self.native_instruction_processors
            .push(solana_storage_program!());
    }
}
