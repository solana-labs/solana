/// Module responsible for notifying the plugins of accounts update
use {
    crate::accountsdb_plugin_manager::AccountsDbPluginManager,
    log::*,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        ReplicaAccountInfo, ReplicaAccountMeta, SlotStatus,
    },
    solana_runtime::{accounts_db::AccountsUpdateNotifierIntf, append_vec::StoredAccountMeta},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
    },
    std::sync::{Arc, RwLock},
};
#[derive(Debug)]
pub(crate) struct AccountsUpdateNotifierImpl {
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
}

impl AccountsUpdateNotifierIntf for AccountsUpdateNotifierImpl {
    fn notify_account_update(
        &self,
        slot: Slot,
        pubkey: &Pubkey,
        hash: Option<&Hash>,
        account: &AccountSharedData,
    ) {
        if let Some(account_info) = self.accountinfo_from_shared_account_data(pubkey, hash, account)
        {
            self.notify_plugins_of_account_update(account_info, slot);
        }
    }

    fn notify_account_data_at_start(&self, slot: Slot, account: &StoredAccountMeta) {
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

    fn accountinfo_from_shared_account_data(
        &self,
        pubkey: &Pubkey,
        hash: Option<&Hash>,
        account: &AccountSharedData,
    ) -> Option<ReplicaAccountInfo> {
        let account_meta = ReplicaAccountMeta {
            pubkey: bs58::encode(pubkey).into_string(),
            lamports: account.lamports(),
            owner: bs58::encode(account.owner()).into_string(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
        };
        let data = account.data().to_vec();
        Some(ReplicaAccountInfo {
            account_meta,
            hash: hash.map(|hash| bs58::encode(hash).into_string()),
            data,
        })
    }

    fn accountinfo_from_stored_account_meta(
        &self,
        stored_account_meta: &StoredAccountMeta,
    ) -> Option<ReplicaAccountInfo> {
        let account_meta = ReplicaAccountMeta {
            pubkey: bs58::encode(stored_account_meta.meta.pubkey).into_string(),
            lamports: stored_account_meta.account_meta.lamports,
            owner: bs58::encode(stored_account_meta.account_meta.owner).into_string(),
            executable: stored_account_meta.account_meta.executable,
            rent_epoch: stored_account_meta.account_meta.rent_epoch,
        };
        let data = stored_account_meta.data.to_vec();
        Some(ReplicaAccountInfo {
            account_meta,
            hash: Some(bs58::encode(stored_account_meta.hash.0).into_string()),
            data,
        })
    }

    fn notify_plugins_of_account_update(&self, account: ReplicaAccountInfo, slot: Slot) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();

        if plugin_manager.plugins.is_empty() {
            return;
        }
        for plugin in plugin_manager.plugins.iter_mut() {
            match plugin.update_account(&account, slot) {
                Err(err) => {
                    error!(
                        "Failed to update account {:?} at slot {:?}, error: {:?}",
                        account.account_meta.pubkey, slot, err
                    )
                }
                Ok(_) => {
                    trace!(
                        "Successfully updated account {:?} at slot {:?}",
                        account.account_meta.pubkey,
                        slot
                    );
                }
            }
        }
    }

    pub fn notify_slot_status(&self, slot: Slot, parent: Option<Slot>, slot_status: SlotStatus) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        for plugin in plugin_manager.plugins.iter_mut() {
            match plugin.update_slot_status(slot, parent, slot_status.clone()) {
                Err(err) => {
                    error!(
                        "Failed to update slot status at slot {:?}, error: {:?}",
                        slot, err
                    )
                }
                Ok(_) => {
                    trace!("Successfully updated slot status at slot {:?}", slot);
                }
            }
        }
    }
}
