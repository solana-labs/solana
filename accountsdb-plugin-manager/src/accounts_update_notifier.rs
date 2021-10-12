/// Module responsible for notifying plugins of account updates
use {
    crate::accountsdb_plugin_manager::AccountsDbPluginManager,
    log::*,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        ReplicaAccountInfo, ReplicaAccountInfoVersions, SlotStatus,
    },
    solana_measure::measure::Measure,
    solana_metrics::*,
    solana_runtime::{
        accounts_update_notifier_interface::AccountsUpdateNotifierInterface,
        append_vec::StoredAccountMeta,
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        pubkey::Pubkey,
    },
    std::sync::{Arc, RwLock},
};
#[derive(Debug)]
pub(crate) struct AccountsUpdateNotifierImpl {
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
}

impl AccountsUpdateNotifierInterface for AccountsUpdateNotifierImpl {
    fn notify_account_update(&self, slot: Slot, pubkey: &Pubkey, account: &AccountSharedData) {
        if let Some(account_info) = self.accountinfo_from_shared_account_data(pubkey, account) {
            self.notify_plugins_of_account_update(account_info, slot);
        }
    }

    fn notify_account_restore_from_snapshot(&self, slot: Slot, account: &StoredAccountMeta) {
        if let Some(account_info) = self.accountinfo_from_stored_account_meta(account) {
            self.notify_plugins_of_account_update(account_info, slot);
        }
    }

    fn notify_slot_confirmed(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Confirmed);
    }

    fn notify_slot_processed(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Processed);
    }

    fn notify_slot_rooted(&self, slot: Slot, parent: Option<Slot>) {
        self.notify_slot_status(slot, parent, SlotStatus::Rooted);
    }
}

impl AccountsUpdateNotifierImpl {
    pub fn new(plugin_manager: Arc<RwLock<AccountsDbPluginManager>>) -> Self {
        AccountsUpdateNotifierImpl { plugin_manager }
    }

    fn accountinfo_from_shared_account_data<'a>(
        &self,
        pubkey: &'a Pubkey,
        account: &'a AccountSharedData,
    ) -> Option<ReplicaAccountInfo<'a>> {
        Some(ReplicaAccountInfo {
            pubkey: pubkey.as_ref(),
            lamports: account.lamports(),
            owner: account.owner().as_ref(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
            data: account.data(),
        })
    }

    fn accountinfo_from_stored_account_meta<'a>(
        &self,
        stored_account_meta: &'a StoredAccountMeta,
    ) -> Option<ReplicaAccountInfo<'a>> {
        Some(ReplicaAccountInfo {
            pubkey: stored_account_meta.meta.pubkey.as_ref(),
            lamports: stored_account_meta.account_meta.lamports,
            owner: stored_account_meta.account_meta.owner.as_ref(),
            executable: stored_account_meta.account_meta.executable,
            rent_epoch: stored_account_meta.account_meta.rent_epoch,
            data: stored_account_meta.data,
        })
    }

    fn notify_plugins_of_account_update(&self, account: ReplicaAccountInfo, slot: Slot) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }
        for plugin in plugin_manager.plugins.iter_mut() {
            let mut measure = Measure::start("accountsdb-plugin-update-account");
            match plugin.update_account(ReplicaAccountInfoVersions::V0_0_1(&account), slot) {
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
            inc_new_counter_info!(
                "accountsdb-plugin-update-account-ms",
                measure.as_ms() as usize,
                100000,
                100000
            );
        }
    }

    pub fn notify_slot_status(&self, slot: Slot, parent: Option<Slot>, slot_status: SlotStatus) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        for plugin in plugin_manager.plugins.iter_mut() {
            let mut measure = Measure::start("accountsdb-plugin-update-slot");
            match plugin.update_slot_status(slot, parent, slot_status.clone()) {
                Err(err) => {
                    error!(
                        "Failed to update slot status at slot {}, error: {} to plugin {}",
                        slot,
                        err,
                        plugin.name()
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated slot status at slot {} to plugin {}",
                        slot,
                        plugin.name()
                    );
                }
            }
            measure.stop();
            inc_new_counter_info!(
                "accountsdb-plugin-update-slot-ms",
                measure.as_ms() as usize,
                1000,
                1000
            );
        }
    }
}
