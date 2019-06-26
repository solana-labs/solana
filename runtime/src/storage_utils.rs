use crate::bank::Bank;
use solana_sdk::account::Account;
use solana_sdk::account_utils::State;
use solana_sdk::pubkey::Pubkey;
use solana_storage_api::storage_contract::StorageContract;
use std::collections::{HashMap, HashSet};

#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize)]
pub struct StorageAccounts {
    /// validator storage accounts and their credits
    validator_accounts: HashSet<Pubkey>,

    /// replicator storage accounts and their credits
    replicator_accounts: HashSet<Pubkey>,

    /// unclaimed points.
    //  1 point == 1 storage account credit
    points: HashMap<Pubkey, u64>,
}

pub fn is_storage(account: &Account) -> bool {
    solana_storage_api::check_id(&account.owner)
}

impl StorageAccounts {
    pub fn store(&mut self, pubkey: &Pubkey, account: &Account) {
        if let Ok(storage_state) = account.state() {
            if let StorageContract::ReplicatorStorage { credits, .. } = storage_state {
                if account.lamports == 0 {
                    self.replicator_accounts.remove(pubkey);
                } else {
                    self.replicator_accounts.insert(*pubkey);
                    self.points.insert(*pubkey, credits.current_epoch);
                }
            } else if let StorageContract::ValidatorStorage { credits, .. } = storage_state {
                if account.lamports == 0 {
                    self.validator_accounts.remove(pubkey);
                } else {
                    self.validator_accounts.insert(*pubkey);
                    self.points.insert(*pubkey, credits.current_epoch);
                }
            }
        };
    }

    /// currently unclaimed points
    pub fn points(&self) -> u64 {
        self.points.values().sum()
    }

    /// "claims" points, resets points to 0
    pub fn claim_points(&mut self) -> u64 {
        let points = self.points();
        self.points.clear();
        points
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
pub(crate) mod tests {
    use super::*;
    use crate::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_storage_api::storage_contract::{StorageAccount, STORAGE_ACCOUNT_SPACE};
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

    #[test]
    fn test_points() {
        // note: storage_points == storage_credits
        let credits = 42;
        let mut storage_accounts = StorageAccounts::default();
        assert_eq!(storage_accounts.points(), 0);
        assert_eq!(storage_accounts.claim_points(), 0);

        // create random validator and replicator accounts with `credits`
        let ((validator_pubkey, validator_account), (replicator_pubkey, replicator_account)) =
            create_storage_accounts_with_credits(credits);

        storage_accounts.store(&validator_pubkey, &validator_account);
        storage_accounts.store(&replicator_pubkey, &replicator_account);
        // check that 2x credits worth of points are available
        assert_eq!(storage_accounts.points(), credits * 2);

        let ((validator_pubkey, validator_account), (replicator_pubkey, mut replicator_account)) =
            create_storage_accounts_with_credits(credits);

        storage_accounts.store(&validator_pubkey, &validator_account);
        storage_accounts.store(&replicator_pubkey, &replicator_account);
        // check that 4x credits worth of points are available
        assert_eq!(storage_accounts.points(), credits * 2 * 2);

        storage_accounts.store(&validator_pubkey, &validator_account);
        storage_accounts.store(&replicator_pubkey, &replicator_account);
        // check that storing again has no effect
        assert_eq!(storage_accounts.points(), credits * 2 * 2);

        let storage_contract = &mut replicator_account.state().unwrap();
        if let StorageContract::ReplicatorStorage {
            credits: account_credits,
            ..
        } = storage_contract
        {
            account_credits.current_epoch += 1;
        }
        replicator_account.set_state(storage_contract).unwrap();
        storage_accounts.store(&replicator_pubkey, &replicator_account);

        // check that incremental store increases credits
        assert_eq!(storage_accounts.points(), credits * 2 * 2 + 1);

        assert_eq!(storage_accounts.claim_points(), credits * 2 * 2 + 1);
        // check that once redeemed, the points are gone
        assert_eq!(storage_accounts.claim_points(), 0);
    }

    pub fn create_storage_accounts_with_credits(
        credits: u64,
    ) -> ((Pubkey, Account), (Pubkey, Account)) {
        let validator_pubkey = Pubkey::new_rand();
        let replicator_pubkey = Pubkey::new_rand();

        let mut validator_account =
            Account::new(1, STORAGE_ACCOUNT_SPACE as usize, &solana_storage_api::id());
        let mut validator = StorageAccount::new(validator_pubkey, &mut validator_account);
        validator
            .initialize_validator_storage(validator_pubkey)
            .unwrap();
        let storage_contract = &mut validator_account.state().unwrap();
        if let StorageContract::ValidatorStorage {
            credits: account_credits,
            ..
        } = storage_contract
        {
            account_credits.current_epoch = credits;
        }
        validator_account.set_state(storage_contract).unwrap();

        let mut replicator_account =
            Account::new(1, STORAGE_ACCOUNT_SPACE as usize, &solana_storage_api::id());
        let mut replicator = StorageAccount::new(replicator_pubkey, &mut replicator_account);
        replicator
            .initialize_replicator_storage(replicator_pubkey)
            .unwrap();
        let storage_contract = &mut replicator_account.state().unwrap();
        if let StorageContract::ReplicatorStorage {
            credits: account_credits,
            ..
        } = storage_contract
        {
            account_credits.current_epoch = credits;
        }
        replicator_account.set_state(storage_contract).unwrap();

        (
            (validator_pubkey, validator_account),
            (replicator_pubkey, replicator_account),
        )
    }
}
