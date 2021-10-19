use {
    crate::{accounts_db::AccountsDb, append_vec::StoredAccountMeta},
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_sdk::{account::AccountSharedData, clock::Slot, pubkey::Pubkey},
    std::collections::{HashMap, HashSet},
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

    fn notify_accounts_in_slot<'a>(
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
                match accounts_to_stream.get(&account.meta.pubkey) {
                    Some(existing_account) => {
                        if account.meta.write_version > existing_account.meta.write_version {
                            accounts_to_stream.insert(account.meta.pubkey, account);
                        } else {
                            notify_stats.skipped_accounts += 1;
                        }
                    }
                    None => {
                        accounts_to_stream.insert(account.meta.pubkey, account);
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
