use {
    solana_sdk::{message::SanitizedMessage, pubkey::Pubkey},
    std::collections::HashSet,
};

/// Wrapper struct to accumulate locks for a batch of transactions.
#[derive(Debug, Default)]
pub struct ReadWriteAccountSet {
    /// Set of accounts that are locked for read
    read_set: HashSet<Pubkey>,
    /// Set of accounts that are locked for write
    write_set: HashSet<Pubkey>,
}

impl ReadWriteAccountSet {
    /// Returns true if all account locks were available and false otherwise.
    #[allow(dead_code)]
    pub fn check_locks(&self, message: &SanitizedMessage) -> bool {
        message
            .account_keys()
            .iter()
            .enumerate()
            .all(|(index, pubkey)| {
                if message.is_writable(index) {
                    self.can_write(pubkey)
                } else {
                    self.can_read(pubkey)
                }
            })
    }

    /// Add all account locks.
    /// Returns true if all account locks were available and false otherwise.
    pub fn take_locks(&mut self, message: &SanitizedMessage) -> bool {
        message
            .account_keys()
            .iter()
            .enumerate()
            .fold(true, |all_available, (index, pubkey)| {
                if message.is_writable(index) {
                    all_available & self.add_write(pubkey)
                } else {
                    all_available & self.add_read(pubkey)
                }
            })
    }

    /// Clears the read and write sets
    pub fn clear(&mut self) {
        self.read_set.clear();
        self.write_set.clear();
    }

    /// Check if an account can be read-locked
    fn can_read(&self, pubkey: &Pubkey) -> bool {
        !self.write_set.contains(pubkey)
    }

    /// Check if an account can be write-locked
    fn can_write(&self, pubkey: &Pubkey) -> bool {
        !self.write_set.contains(pubkey) && !self.read_set.contains(pubkey)
    }

    /// Add an account to the read-set.
    /// Returns true if the lock was available.
    fn add_read(&mut self, pubkey: &Pubkey) -> bool {
        let can_read = self.can_read(pubkey);
        self.read_set.insert(*pubkey);

        can_read
    }

    /// Add an account to the write-set.
    /// Returns true if the lock was available.
    fn add_write(&mut self, pubkey: &Pubkey) -> bool {
        let can_write = self.can_write(pubkey);
        self.write_set.insert(*pubkey);

        can_write
    }
}

#[cfg(test)]
mod tests {
    use {
        super::ReadWriteAccountSet,
        solana_ledger::genesis_utils::GenesisConfigInfo,
        solana_runtime::{bank::Bank, genesis_utils::create_genesis_config},
        solana_sdk::{
            account::AccountSharedData,
            address_lookup_table::{
                self,
                state::{AddressLookupTable, LookupTableMeta},
            },
            hash::Hash,
            message::{
                v0::{self, MessageAddressTableLookup},
                MessageHeader, VersionedMessage,
            },
            pubkey::Pubkey,
            signature::Keypair,
            signer::Signer,
            transaction::{MessageHash, SanitizedTransaction, VersionedTransaction},
        },
        std::{borrow::Cow, sync::Arc},
    };

    fn create_test_versioned_message(
        write_keys: &[Pubkey],
        read_keys: &[Pubkey],
        address_table_lookups: Vec<MessageAddressTableLookup>,
    ) -> VersionedMessage {
        VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: write_keys.len() as u8,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: read_keys.len() as u8,
            },
            recent_blockhash: Hash::default(),
            account_keys: write_keys.iter().chain(read_keys.iter()).copied().collect(),
            address_table_lookups,
            instructions: vec![],
        })
    }

    fn create_test_sanitized_transaction(
        write_keypair: &Keypair,
        read_keys: &[Pubkey],
        address_table_lookups: Vec<MessageAddressTableLookup>,
        bank: &Bank,
    ) -> SanitizedTransaction {
        let message = create_test_versioned_message(
            &[write_keypair.pubkey()],
            read_keys,
            address_table_lookups,
        );
        SanitizedTransaction::try_create(
            VersionedTransaction::try_new(message, &[write_keypair]).unwrap(),
            MessageHash::Compute,
            Some(false),
            bank,
        )
        .unwrap()
    }

    fn create_test_address_lookup_table(
        bank: Arc<Bank>,
        num_addresses: usize,
    ) -> (Arc<Bank>, Pubkey) {
        let mut addresses = Vec::with_capacity(num_addresses);
        addresses.resize_with(num_addresses, Pubkey::new_unique);
        let address_lookup_table = AddressLookupTable {
            meta: LookupTableMeta {
                authority: None,
                ..LookupTableMeta::default()
            },
            addresses: Cow::Owned(addresses),
        };

        let address_table_key = Pubkey::new_unique();
        let data = address_lookup_table.serialize_for_tests().unwrap();
        let mut account =
            AccountSharedData::new(1, data.len(), &address_lookup_table::program::id());
        account.set_data(data);
        bank.store_account(&address_table_key, &account);

        let slot = bank.slot() + 1;
        (
            Arc::new(Bank::new_from_parent(bank, &Pubkey::new_unique(), slot)),
            address_table_key,
        )
    }

    fn create_test_bank() -> Arc<Bank> {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        Bank::new_no_wallclock_throttle_for_tests(&genesis_config).0
    }

    // Helper function (could potentially use test_case in future).
    // conflict_index = 0 means write lock conflict with static key
    // conflict_index = 1 means read lock conflict with static key
    // conflict_index = 2 means write lock conflict with address table key
    // conflict_index = 3 means read lock conflict with address table key
    fn test_check_and_take_locks(conflict_index: usize, add_write: bool, expectation: bool) {
        let bank = create_test_bank();
        let (bank, table_address) = create_test_address_lookup_table(bank, 2);
        let tx = create_test_sanitized_transaction(
            &Keypair::new(),
            &[Pubkey::new_unique()],
            vec![MessageAddressTableLookup {
                account_key: table_address,
                writable_indexes: vec![0],
                readonly_indexes: vec![1],
            }],
            &bank,
        );
        let message = tx.message();

        let mut account_locks = ReadWriteAccountSet::default();

        let conflict_key = message.account_keys().get(conflict_index).unwrap();
        if add_write {
            account_locks.add_write(conflict_key);
        } else {
            account_locks.add_read(conflict_key);
        }
        assert_eq!(expectation, account_locks.check_locks(message));
        assert_eq!(expectation, account_locks.take_locks(message));
    }

    #[test]
    fn test_check_and_take_locks_write_write_conflict() {
        test_check_and_take_locks(0, true, false); // static key conflict
        test_check_and_take_locks(2, true, false); // lookup key conflict
    }

    #[test]
    fn test_check_and_take_locks_read_write_conflict() {
        test_check_and_take_locks(0, false, false); // static key conflict
        test_check_and_take_locks(2, false, false); // lookup key conflict
    }

    #[test]
    fn test_check_and_take_locks_write_read_conflict() {
        test_check_and_take_locks(1, true, false); // static key conflict
        test_check_and_take_locks(3, true, false); // lookup key conflict
    }

    #[test]
    fn test_check_and_take_locks_read_read_non_conflict() {
        test_check_and_take_locks(1, false, true); // static key conflict
        test_check_and_take_locks(3, false, true); // lookup key conflict
    }

    #[test]
    pub fn test_write_write_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account = Pubkey::new_unique();
        assert!(account_locks.can_write(&account));
        account_locks.add_write(&account);
        assert!(!account_locks.can_write(&account));
    }

    #[test]
    pub fn test_read_write_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account = Pubkey::new_unique();
        assert!(account_locks.can_read(&account));
        account_locks.add_read(&account);
        assert!(!account_locks.can_write(&account));
        assert!(account_locks.can_read(&account));
    }

    #[test]
    pub fn test_write_read_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account = Pubkey::new_unique();
        assert!(account_locks.can_write(&account));
        account_locks.add_write(&account);
        assert!(!account_locks.can_write(&account));
        assert!(!account_locks.can_read(&account));
    }

    #[test]
    pub fn test_read_read_non_conflict() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account = Pubkey::new_unique();
        assert!(account_locks.can_read(&account));
        account_locks.add_read(&account);
        assert!(account_locks.can_read(&account));
    }

    #[test]
    pub fn test_write_write_different_keys() {
        let mut account_locks = ReadWriteAccountSet::default();
        let account1 = Pubkey::new_unique();
        let account2 = Pubkey::new_unique();
        assert!(account_locks.can_write(&account1));
        account_locks.add_write(&account1);
        assert!(account_locks.can_write(&account2));
        assert!(account_locks.can_read(&account2));
    }
}
