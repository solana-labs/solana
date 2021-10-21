use {
    crate::{accounts_db::AccountsDb, append_vec::StoredAccountMeta},
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_sdk::{account::AccountSharedData, clock::Slot, pubkey::Pubkey},
    std::collections::{hash_map::Entry, HashMap, HashSet},
};

#[derive(Default)]
pub struct AccountsDbPluginNotifyAtSnapshotRestoreStats {
    pub total_accounts: usize,
    pub skipped_accounts: usize,
    pub notified_accounts: usize,
    pub elapsed_filtering_us: usize,
    pub total_pure_notify: usize,
    pub total_pure_bookeeping: usize,
    pub elapsed_notifying_us: usize,
}

impl AccountsDbPluginNotifyAtSnapshotRestoreStats {
    pub fn report(&self) {
        datapoint_info!(
            "accountsdb_plugin_notify_account_restore_from_snapshot_summary",
            ("total_accounts", self.total_accounts, i64),
            ("skipped_accounts", self.skipped_accounts, i64),
            ("notified_accounts", self.notified_accounts, i64),
            ("elapsed_filtering_us", self.elapsed_filtering_us, i64),
            ("elapsed_notifying_us", self.elapsed_notifying_us, i64),
            ("total_pure_notify_us", self.total_pure_notify, i64),
            ("total_pure_bookeeping_us", self.total_pure_bookeeping, i64),
        );
    }
}

impl AccountsDb {
    /// Notify the plugins of of account data when AccountsDb is restored from a snapshot. The data is streamed
    /// in the reverse order of the slots so that an account is only streamed once. At a slot, if the accounts is updated
    /// multiple times only the last write (with highest write_version) is notified.
    pub fn notify_account_restore_from_snapshot(&self) {
        if self.accounts_update_notifier.is_none() {
            return;
        }

        let mut slots = self.storage.all_slots();
        let mut notified_accounts: HashSet<Pubkey> = HashSet::default();
        let mut notify_stats = AccountsDbPluginNotifyAtSnapshotRestoreStats::default();

        slots.sort_by(|a, b| b.cmp(a));
        for slot in slots {
            self.notify_accounts_in_slot(slot, &mut notified_accounts, &mut notify_stats);
        }

        let accounts_update_notifier = self.accounts_update_notifier.as_ref().unwrap();
        let notifier = &accounts_update_notifier.read().unwrap();
        notifier.notify_end_of_restore_from_snapshot();
        notify_stats.report();
    }

    pub fn notify_account_at_accounts_update(
        &self,
        slot: Slot,
        accounts: &[(&Pubkey, &AccountSharedData)],
    ) {
        if let Some(accounts_update_notifier) = &self.accounts_update_notifier {
            let notifier = &accounts_update_notifier.read().unwrap();

            for account in accounts {
                let pubkey = account.0;
                let account = account.1;
                notifier.notify_account_update(slot, pubkey, account);
            }
        }
    }

    fn notify_accounts_in_slot(
        &self,
        slot: Slot,
        notified_accounts: &mut HashSet<Pubkey>,
        notify_stats: &mut AccountsDbPluginNotifyAtSnapshotRestoreStats,
    ) {
        let slot_stores = self.storage.get_slot_stores(slot).unwrap();

        let slot_stores = slot_stores.read().unwrap();
        let mut accounts_to_stream: HashMap<Pubkey, StoredAccountMeta> = HashMap::default();
        let mut measure_filter = Measure::start("accountsdb-plugin-filtering-accounts");
        for (_, storage_entry) in slot_stores.iter() {
            let mut accounts = storage_entry.all_accounts();
            let account_len = accounts.len();
            notify_stats.total_accounts += account_len;
            accounts.drain(..).into_iter().for_each(|account| {
                if notified_accounts.contains(&account.meta.pubkey) {
                    notify_stats.skipped_accounts += 1;
                    return;
                }
                match accounts_to_stream.entry(account.meta.pubkey) {
                    Entry::Occupied(mut entry) => {
                        let existing_account = entry.get();
                        if account.meta.write_version > existing_account.meta.write_version {
                            entry.insert(account);
                        } else {
                            notify_stats.skipped_accounts += 1;
                        }
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(account);
                    }
                }
            });
        }
        measure_filter.stop();
        notify_stats.elapsed_filtering_us += measure_filter.as_us() as usize;

        self.notify_filtered_accounts(slot, notified_accounts, &accounts_to_stream, notify_stats);
    }

    fn notify_filtered_accounts(
        &self,
        slot: Slot,
        notified_accounts: &mut HashSet<Pubkey>,
        accounts_to_stream: &HashMap<Pubkey, StoredAccountMeta>,
        notify_stats: &mut AccountsDbPluginNotifyAtSnapshotRestoreStats,
    ) {
        let notifier = self
            .accounts_update_notifier
            .as_ref()
            .unwrap()
            .read()
            .unwrap();

        let mut measure_notify = Measure::start("accountsdb-plugin-notifying-accounts");
        for account in accounts_to_stream.values() {
            let mut measure_pure_notify = Measure::start("accountsdb-plugin-notifying-accounts");
            notifier.notify_account_restore_from_snapshot(slot, account);
            measure_pure_notify.stop();

            notify_stats.total_pure_notify += measure_pure_notify.as_us() as usize;

            let mut measure_bookkeep = Measure::start("accountsdb-plugin-notifying-bookeeeping");
            notified_accounts.insert(account.meta.pubkey);
            measure_bookkeep.stop();
            notify_stats.total_pure_bookeeping += measure_bookkeep.as_us() as usize;
        }
        notify_stats.notified_accounts += accounts_to_stream.len();
        measure_notify.stop();
        notify_stats.elapsed_notifying_us += measure_notify.as_us() as usize;
    }
}

#[cfg(test)]
pub mod tests {
    use {
        crate::{
            accounts_db::AccountsDb,
            accounts_update_notifier_interface::{
                AccountsUpdateNotifier, AccountsUpdateNotifierInterface,
            },
            append_vec::StoredAccountMeta,
        },
        dashmap::DashMap,
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            clock::Slot,
            pubkey::Pubkey,
        },
        std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    };

    impl AccountsDb {
        pub fn set_accountsdb_plugin_notifer(&mut self, notifier: Option<AccountsUpdateNotifier>) {
            self.accounts_update_notifier = notifier;
        }
    }

    #[derive(Debug, Default)]
    struct AccountsDbTestPlugin {
        pub accounts_at_snapshot_restore: DashMap<Pubkey, Vec<(Slot, AccountSharedData)>>,
        pub is_startup_done: AtomicBool,
    }

    impl AccountsUpdateNotifierInterface for AccountsDbTestPlugin {
        /// Notified when an account is updated at runtime, due to transaction activities
        fn notify_account_update(&self, slot: Slot, pubkey: &Pubkey, account: &AccountSharedData) {
            self.accounts_at_snapshot_restore
                .entry(*pubkey)
                .or_insert(Vec::default())
                .push((slot, account.clone()));
        }

        /// Notified when the AccountsDb is initialized at start when restored
        /// from a snapshot.
        fn notify_account_restore_from_snapshot(&self, slot: Slot, account: &StoredAccountMeta) {
            self.accounts_at_snapshot_restore
                .entry(account.meta.pubkey)
                .or_insert(Vec::default())
                .push((slot, account.clone_account()));
        }

        /// Notified when a slot is optimistically confirmed
        fn notify_slot_confirmed(&self, _slot: Slot, _parent: Option<Slot>) {}

        /// Notified when a slot is marked frozen.
        fn notify_slot_processed(&self, _slot: Slot, _parent: Option<Slot>) {}

        /// Notified when a slot is rooted.
        fn notify_slot_rooted(&self, _slot: Slot, _parent: Option<Slot>) {}

        fn notify_end_of_restore_from_snapshot(&self) {
            self.is_startup_done.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn test_notify_account_restore_from_snapshot_once_per_slot() {
        let mut accounts = AccountsDb::new_single_for_tests();
        // Account with key1 is updated twice in the store -- should only get notified once.
        let key1 = solana_sdk::pubkey::new_rand();
        let mut account1_lamports: u64 = 1;
        let account1 =
            AccountSharedData::new(account1_lamports, 1, AccountSharedData::default().owner());
        let slot0 = 0;
        accounts.store_uncached(slot0, &[(&key1, &account1)]);

        account1_lamports = 2;
        let account1 = AccountSharedData::new(account1_lamports, 1, account1.owner());
        accounts.store_uncached(slot0, &[(&key1, &account1)]);
        let notifier = AccountsDbTestPlugin::default();

        let key2 = solana_sdk::pubkey::new_rand();
        let account2_lamports: u64 = 100;
        let account2 =
            AccountSharedData::new(account2_lamports, 1, AccountSharedData::default().owner());

        accounts.store_uncached(slot0, &[(&key2, &account2)]);

        let notifier = Arc::new(RwLock::new(notifier));
        accounts.set_accountsdb_plugin_notifer(Some(notifier.clone()));

        accounts.notify_account_restore_from_snapshot();

        let notifier = notifier.write().unwrap();
        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key1)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[0]
                .1
                .lamports(),
            account1_lamports
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[0].0,
            slot0
        );
        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key2)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key2).unwrap()[0]
                .1
                .lamports(),
            account2_lamports
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key2).unwrap()[0].0,
            slot0
        );

        assert!(notifier.is_startup_done.load(Ordering::Relaxed));
    }

    #[test]
    fn test_notify_account_restore_from_snapshot_once_across_slots() {
        let mut accounts = AccountsDb::new_single_for_tests();
        // Account with key1 is updated twice in two different slots -- should only get notified once.
        // Account with key2 is updated slot0, should get notified once
        // Account with key3 is updated in slot1, should get notified once
        let key1 = solana_sdk::pubkey::new_rand();
        let mut account1_lamports: u64 = 1;
        let account1 =
            AccountSharedData::new(account1_lamports, 1, AccountSharedData::default().owner());
        let slot0 = 0;
        accounts.store_uncached(slot0, &[(&key1, &account1)]);

        let key2 = solana_sdk::pubkey::new_rand();
        let account2_lamports: u64 = 200;
        let account2 =
            AccountSharedData::new(account2_lamports, 1, AccountSharedData::default().owner());
        accounts.store_uncached(slot0, &[(&key2, &account2)]);

        account1_lamports = 2;
        let slot1 = 1;
        let account1 = AccountSharedData::new(account1_lamports, 1, account1.owner());
        accounts.store_uncached(slot1, &[(&key1, &account1)]);
        let notifier = AccountsDbTestPlugin::default();

        let key3 = solana_sdk::pubkey::new_rand();
        let account3_lamports: u64 = 300;
        let account3 =
            AccountSharedData::new(account3_lamports, 1, AccountSharedData::default().owner());
        accounts.store_uncached(slot1, &[(&key3, &account3)]);

        let notifier = Arc::new(RwLock::new(notifier));
        accounts.set_accountsdb_plugin_notifer(Some(notifier.clone()));

        accounts.notify_account_restore_from_snapshot();

        let notifier = notifier.write().unwrap();
        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key1)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[0]
                .1
                .lamports(),
            account1_lamports
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[0].0,
            slot1
        );
        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key2)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key2).unwrap()[0]
                .1
                .lamports(),
            account2_lamports
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key2).unwrap()[0].0,
            slot0
        );
        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key3)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key3).unwrap()[0]
                .1
                .lamports(),
            account3_lamports
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key3).unwrap()[0].0,
            slot1
        );
        assert!(notifier.is_startup_done.load(Ordering::Relaxed));
    }

    #[test]
    fn test_notify_account_at_accounts_update() {
        let mut accounts = AccountsDb::new_single_for_tests();
        let notifier = AccountsDbTestPlugin::default();

        let notifier = Arc::new(RwLock::new(notifier));
        accounts.set_accountsdb_plugin_notifer(Some(notifier.clone()));

        // Account with key1 is updated twice in two different slots -- should only get notified twice.
        // Account with key2 is updated slot0, should get notified once
        // Account with key3 is updated in slot1, should get notified once
        let key1 = solana_sdk::pubkey::new_rand();
        let account1_lamports1: u64 = 1;
        let account1 =
            AccountSharedData::new(account1_lamports1, 1, AccountSharedData::default().owner());
        let slot0 = 0;
        accounts.store_cached(slot0, &[(&key1, &account1)]);

        let key2 = solana_sdk::pubkey::new_rand();
        let account2_lamports: u64 = 200;
        let account2 =
            AccountSharedData::new(account2_lamports, 1, AccountSharedData::default().owner());
        accounts.store_cached(slot0, &[(&key2, &account2)]);

        let account1_lamports2 = 2;
        let slot1 = 1;
        let account1 = AccountSharedData::new(account1_lamports2, 1, account1.owner());
        accounts.store_cached(slot1, &[(&key1, &account1)]);

        let key3 = solana_sdk::pubkey::new_rand();
        let account3_lamports: u64 = 300;
        let account3 =
            AccountSharedData::new(account3_lamports, 1, AccountSharedData::default().owner());
        accounts.store_cached(slot1, &[(&key3, &account3)]);

        let notifier = notifier.write().unwrap();
        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key1)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[0]
                .1
                .lamports(),
            account1_lamports1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[0].0,
            slot0
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[1]
                .1
                .lamports(),
            account1_lamports2
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key1).unwrap()[1].0,
            slot1
        );

        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key2)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key2).unwrap()[0]
                .1
                .lamports(),
            account2_lamports
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key2).unwrap()[0].0,
            slot0
        );
        assert_eq!(
            notifier
                .accounts_at_snapshot_restore
                .get(&key3)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key3).unwrap()[0]
                .1
                .lamports(),
            account3_lamports
        );
        assert_eq!(
            notifier.accounts_at_snapshot_restore.get(&key3).unwrap()[0].0,
            slot1
        );
    }
}
