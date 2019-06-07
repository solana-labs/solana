use crate::bank::Bank;
use solana_sdk::account::Account;
use solana_sdk::account_utils::State;
use solana_sdk::pubkey::Pubkey;
use solana_storage_api::storage_contract::StorageContract;
use std::collections::{HashMap, HashSet};

#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize)]
pub struct StorageAccounts {
    /// validator storage accounts
    validator_accounts: HashSet<Pubkey>,

    /// replicator storage accounts
    replicator_accounts: HashSet<Pubkey>,
}

pub fn is_storage(account: &Account) -> bool {
    solana_storage_api::check_id(&account.owner)
}

impl StorageAccounts {
    pub fn store(&mut self, pubkey: &Pubkey, account: &Account) {
        if let Ok(storage_state) = account.state() {
            if let StorageContract::ReplicatorStorage { .. } = storage_state {
                if account.lamports == 0 {
                    self.replicator_accounts.remove(pubkey);
                } else {
                    self.replicator_accounts.insert(*pubkey);
                }
            } else if let StorageContract::ValidatorStorage { .. } = storage_state {
                if account.lamports == 0 {
                    self.validator_accounts.remove(pubkey);
                } else {
                    self.validator_accounts.insert(*pubkey);
                }
            }
        };
    }
}

pub fn validator_accounts(bank: &Bank) -> HashMap<Pubkey, Account> {
    bank.storage_accounts()
        .validator_accounts
        .iter()
        .filter_map(|account_id| {
            bank.get_account(account_id)
                .and_then(|account| Some((*account_id, account)))
        })
        .collect()
}

pub fn replicator_accounts(bank: &Bank) -> HashMap<Pubkey, Account> {
    bank.storage_accounts()
        .replicator_accounts
        .iter()
        .filter_map(|account_id| {
            bank.get_account(account_id)
                .and_then(|account| Some((*account_id, account)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_storage_api::{storage_instruction, storage_processor};
    use std::sync::Arc;

    #[test]
    fn test_store_and_recover() {
        let (genesis_block, mint_keypair) = create_genesis_block(1000);
        let mint_pubkey = mint_keypair.pubkey();
        let replicator_keypair = Keypair::new();
        let replicator_pubkey = replicator_keypair.pubkey();
        let validator_keypair = Keypair::new();
        let validator_pubkey = validator_keypair.pubkey();
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(
            solana_storage_api::id(),
            storage_processor::process_instruction,
        );

        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);

        bank_client
            .transfer(10, &mint_keypair, &replicator_pubkey)
            .unwrap();
        let message = Message::new(storage_instruction::create_replicator_storage_account(
            &mint_pubkey,
            &Pubkey::default(),
            &replicator_pubkey,
            1,
        ));
        bank_client.send_message(&[&mint_keypair], message).unwrap();

        bank_client
            .transfer(10, &mint_keypair, &validator_pubkey)
            .unwrap();
        let message = Message::new(storage_instruction::create_validator_storage_account(
            &mint_pubkey,
            &Pubkey::default(),
            &validator_pubkey,
            1,
        ));
        bank_client.send_message(&[&mint_keypair], message).unwrap();

        assert_eq!(validator_accounts(bank.as_ref()).len(), 1);
        assert_eq!(replicator_accounts(bank.as_ref()).len(), 1);
    }
}
