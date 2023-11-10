use solana_geyser_plugin_interface::geyser_plugin_interface::ReplicaPreviousAccountInfo;
/// Module responsible for notifying plugins of account updates
use {
    crate::geyser_plugin_manager::GeyserPluginManager,
    log::*,
    solana_accounts_db::{
        account_storage::meta::StoredAccountMeta,
        accounts_update_notifier_interface::AccountsUpdateNotifierInterface,
    },
    solana_geyser_plugin_interface::geyser_plugin_interface::{
        ReplicaAccountInfoV3, ReplicaAccountInfoV4, ReplicaAccountInfoVersions,
    },
    solana_measure::measure::Measure,
    solana_metrics::*,
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
        previous_account_state: Option<&AccountSharedData>,
    ) {
        self.notify_plugins_of_account_update(
            account,
            txn,
            pubkey,
            write_version,
            previous_account_state,
            slot,
            false,
        );
    }

    fn notify_account_restore_from_snapshot(&self, slot: Slot, account: &StoredAccountMeta) {
        let mut measure_all = Measure::start("geyser-plugin-notify-account-restore-all");

        self.notify_plugins_of_account_update(
            account,
            &None,
            account.pubkey(),
            account.write_version(),
            None,
            slot,
            true,
        );
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

    fn enable_preexecution_account_states_notification(&self) -> bool {
        let manager = self.plugin_manager.read().unwrap();
        manager.enable_preexecution_account_states_notification()
    }
}

impl AccountsUpdateNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<GeyserPluginManager>>) -> Self {
        AccountsUpdateNotifierImpl { plugin_manager }
    }

    fn accountinfo_from_shared_account_data_with_previous_state<'a, T: ReadableAccount>(
        &self,
        account: &'a T,
        txn: &'a Option<&'a SanitizedTransaction>,
        pubkey: &'a Pubkey,
        write_version: u64,
        previous_account_state: Option<&'a AccountSharedData>,
    ) -> ReplicaAccountInfoV4<'a> {
        ReplicaAccountInfoV4 {
            pubkey: pubkey.as_ref(),
            lamports: account.lamports(),
            owner: account.owner().as_ref(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
            data: account.data(),
            write_version,
            txn: *txn,
            previous_account_state: previous_account_state.map(|account| {
                ReplicaPreviousAccountInfo {
                    data: account.data(),
                    executable: account.executable(),
                    lamports: account.lamports(),
                    owner: account.owner().as_ref(),
                    rent_epoch: account.rent_epoch(),
                }
            }),
        }
    }

    fn accountinfo_from_shared_account_data<'a, T: ReadableAccount>(
        &self,
        account: &'a T,
        txn: &'a Option<&'a SanitizedTransaction>,
        pubkey: &'a Pubkey,
        write_version: u64,
    ) -> ReplicaAccountInfoV3<'a> {
        ReplicaAccountInfoV3 {
            pubkey: pubkey.as_ref(),
            lamports: account.lamports(),
            owner: account.owner().as_ref(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
            data: account.data(),
            write_version,
            txn: *txn,
        }
    }

    fn notify_plugins_of_account_update<T: ReadableAccount>(
        &self,
        account: &T,
        txn: &Option<&SanitizedTransaction>,
        pubkey: &Pubkey,
        write_version: u64,
        previous_account_state: Option<&AccountSharedData>,
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
            let res = if plugin.enable_pre_trasaction_execution_accounts_data() {
                let account = self.accountinfo_from_shared_account_data_with_previous_state(
                    account,
                    txn,
                    pubkey,
                    write_version,
                    previous_account_state,
                );
                plugin.update_account(
                    ReplicaAccountInfoVersions::V0_0_4(&account),
                    slot,
                    is_startup,
                )
            } else {
                let account =
                    self.accountinfo_from_shared_account_data(account, txn, pubkey, write_version);
                plugin.update_account(
                    ReplicaAccountInfoVersions::V0_0_3(&account),
                    slot,
                    is_startup,
                )
            };
            match res {
                Err(err) => {
                    error!(
                        "Failed to update account {} at slot {}, error: {} to plugin {}",
                        bs58::encode(pubkey).into_string(),
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated account {} at slot {} to plugin {}",
                        bs58::encode(pubkey).into_string(),
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
