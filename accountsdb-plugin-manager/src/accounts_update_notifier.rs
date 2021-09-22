use std::str::FromStr;

/// Module responsible for notifying the plugins of accounts update
use {
    crate::accountsdb_plugin_manager::AccountsDbPluginManager,
    log::*,
    solana_accountsdb_plugin_intf::accountsdb_plugin_intf::{
        ReplicaAccountInfo, ReplicaAccountMeta, SlotStatus,
    },
    solana_runtime::accounts_db::AccountsUpdateNotifierIntf,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{
        collections::HashSet,
        sync::{Arc, RwLock},
    },
};
#[derive(Debug)]
pub(crate) struct AccountsUpdateNotifierImpl {
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
    accounts_selector: AccountsSelector,
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

#[derive(Debug)]
pub(crate) struct AccountsSelector {
    pub accounts: HashSet<Pubkey>,
    pub owners: HashSet<Pubkey>,
    pub select_all_accounts: bool,
}

impl AccountsSelector {
    pub fn default() -> Self {
        AccountsSelector {
            accounts: HashSet::default(),
            owners: HashSet::default(),
            select_all_accounts: true,
        }
    }

    pub fn new(accounts: &[String], owners: &[String]) -> Self {
        println!("Creating AccountsSelector from accounts: {:?}, owners: {:?}", accounts, owners);

        let select_all_accounts = accounts.iter().any(|key| key == "*");
        if select_all_accounts {
            return AccountsSelector {
                accounts: HashSet::default(),
                owners: HashSet::default(),
                select_all_accounts,
            };
        }
        let accounts = accounts
            .iter()
            .map(|pubkey| Pubkey::from_str(pubkey).unwrap())
            .collect();
        let owners = owners
            .iter()
            .map(|pubkey| Pubkey::from_str(pubkey).unwrap())
            .collect();
        AccountsSelector {
            accounts,
            owners,
            select_all_accounts,
        }
    }

    pub fn is_account_selected(&self, account: &Pubkey, owner: &Pubkey) -> bool {
        self.select_all_accounts || self.accounts.contains(account) || self.owners.contains(owner)
    }
}

impl AccountsUpdateNotifierImpl {
    pub fn new(
        plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
        accounts_selector: AccountsSelector,
    ) -> Self {
        AccountsUpdateNotifierImpl {
            plugin_manager,
            accounts_selector,
        }
    }

    fn accountinfo_from_shared_account_data(
        &self,
        pubkey: &Pubkey,
        hash: Option<&Hash>,
        account: &AccountSharedData,
    ) -> Option<ReplicaAccountInfo> {
        if !self
            .accounts_selector
            .is_account_selected(pubkey, account.owner())
        {
            return None;
        }

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

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
    };

    #[test]
    fn test_create_accounts_selector() {
        AccountsSelector::new(&["9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin".to_string()], &[]);

        AccountsSelector::new(&[], &["9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin".to_string()]);
    }
}