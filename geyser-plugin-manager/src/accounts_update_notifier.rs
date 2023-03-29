/// Module responsible for notifying plugins of account updates
use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    log::*,
    solana_geyser_plugin_interface::geyser_plugin_interface::{
        ReplicaAccountInfoV3, ReplicaAccountInfoVersions,
    },
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_runtime::{
        account_storage::meta::StoredAccountMeta,
        accounts_update_notifier_interface::AccountsUpdateNotifierInterface,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        pubkey::Pubkey,
        transaction::SanitizedTransaction,
    },
    std::sync::{Arc, RwLock},
};
#[derive(Debug)]
pub(crate) struct AccountsUpdateNotifierImpl {
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
}

impl AccountsUpdateNotifierInterface for AccountsUpdateNotifierImpl {
    fn notify_account_update(
        &self,
        slot: Slot,
        account: &AccountSharedData,
        txn: &Option<&SanitizedTransaction>,
        pubkey: &Pubkey,
        write_version: u64,
    ) {
        if let Some(account_info) =
            self.accountinfo_from_shared_account_data(account, txn, pubkey, write_version)
        {
            self.notify_plugins_of_account_update(account_info, slot, false);
        }
    }

    fn notify_account_restore_from_snapshot(&self, slot: Slot, account: &StoredAccountMeta) {
        let mut measure_all = Measure::start("geyser-plugin-notify-account-restore-all");
        let mut measure_copy = Measure::start("geyser-plugin-copy-stored-account-info");

        let account = self.accountinfo_from_stored_account_meta(account);
        measure_copy.stop();

        inc_new_counter_debug!(
            "geyser-plugin-copy-stored-account-info-us",
            measure_copy.as_us() as usize,
            100000,
            100000
        );

        if let Some(account_info) = account {
            self.notify_plugins_of_account_update(account_info, slot, true);
        }
        measure_all.stop();

        inc_new_counter_debug!(
            "geyser-plugin-notify-account-restore-all-us",
            measure_all.as_us() as usize,
            100000,
            100000
        );
    }

    fn notify_end_of_restore_from_snapshot(&self) {
        let plugin_manager = self.plugin_manager.read().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        for plugin in plugin_manager.plugins.iter() {
            let mut measure = Measure::start("geyser-plugin-end-of-restore-from-snapshot");
            match plugin.notify_end_of_startup() {
                Err(err) => {
                    error!(
                        "Failed to notify the end of restore from snapshot, error: {} to plugin {}",
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully notified the end of restore from snapshot to plugin {}",
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "geyser-plugin-end-of-restore-from-snapshot",
                measure.as_us() as usize
            );
        }
    }
}

impl AccountsUpdateNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        AccountsUpdateNotifierImpl { plugin_manager }
    }

    fn accountinfo_from_shared_account_data<'a>(
        &self,
        account: &'a AccountSharedData,
        txn: &'a Option<&'a SanitizedTransaction>,
        pubkey: &'a Pubkey,
        write_version: u64,
    ) -> Option<ReplicaAccountInfoV3<'a>> {
        Some(ReplicaAccountInfoV3 {
            pubkey: pubkey.as_ref(),
            lamports: account.lamports(),
            owner: account.owner().as_ref(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
            data: account.data(),
            write_version,
            txn: *txn,
        })
    }

    fn accountinfo_from_stored_account_meta<'a>(
        &self,
        stored_account_meta: &'a StoredAccountMeta,
    ) -> Option<ReplicaAccountInfoV3<'a>> {
        Some(ReplicaAccountInfoV3 {
            pubkey: stored_account_meta.pubkey().as_ref(),
            lamports: stored_account_meta.lamports(),
            owner: stored_account_meta.owner().as_ref(),
            executable: stored_account_meta.executable(),
            rent_epoch: stored_account_meta.rent_epoch(),
            data: stored_account_meta.data(),
            write_version: stored_account_meta.write_version(),
            txn: None,
        })
    }

    fn notify_plugins_of_account_update(
        &self,
        account: ReplicaAccountInfoV3,
        slot: Slot,
        is_startup: bool,
    ) {
        let mut measure2 = Measure::start("geyser-plugin-notify_plugins_of_account_update");
        let plugin_manager = self.plugin_manager.read().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }
        for plugin in plugin_manager.plugins.iter() {
            let mut measure = Measure::start("geyser-plugin-update-account");
            match plugin.update_account(
                ReplicaAccountInfoVersions::V0_0_3(&account),
                slot,
                is_startup,
            ) {
                Err(err) => {
                    error!(
                        "Failed to update account {} at slot {}, error: {} to plugin {}",
                        bs58::encode(account.pubkey).into_string(),
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated account {} at slot {} to plugin {}",
                        bs58::encode(account.pubkey).into_string(),
                        slot,
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_debug!(
                "geyser-plugin-update-account-us",
                measure.as_us() as usize,
                100000,
                100000
            );
        }
        measure2.stop();
        inc_new_counter_debug!(
            "geyser-plugin-notify_plugins_of_account_update-us",
            measure2.as_us() as usize,
            100000,
            100000
        );
    }
}
