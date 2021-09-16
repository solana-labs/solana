/// Module responsible for notifying the plugins of accounts update
use {
    crate::accountsdb_plugin_manager::AccountsDbPluginManager,
    log::*,
    solana_accountsdb_plugin_intf::accountsdb_plugin_intf::{
        ReplicaAccountInfo, ReplicaAccountMeta,
    },
    solana_runtime::{
        accounts_cache::CachedAccount, accounts_db::LoadedAccount, append_vec::StoredAccountMeta,
        bank_forks::BankForks,
    },
    solana_sdk::account::Account,
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub(crate) struct AccountsUpdateNotifier {
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
    bank_forks: Arc<RwLock<BankForks>>,
}

fn accountinfo_from_stored_account_meta(
    stored_account_meta: &StoredAccountMeta,
) -> ReplicaAccountInfo {
    let account_meta = ReplicaAccountMeta {
        pubkey: stored_account_meta.meta.pubkey.to_bytes().to_vec(),
        lamports: stored_account_meta.account_meta.lamports,
        owner: stored_account_meta.account_meta.owner.to_bytes().to_vec(),
        executable: stored_account_meta.account_meta.executable,
        rent_epoch: stored_account_meta.account_meta.rent_epoch,
    };
    let data = stored_account_meta.data.to_vec();
    ReplicaAccountInfo {
        account_meta,
        hash: stored_account_meta.hash.0.to_vec(),
        data,
    }
}

fn accountinfo_from_cached_account(cached_account: &CachedAccount) -> ReplicaAccountInfo {
    let account = Account::from(cached_account.account.clone());
    let account_meta = ReplicaAccountMeta {
        pubkey: cached_account.pubkey().to_bytes().to_vec(),
        lamports: account.lamports,
        owner: account.owner.to_bytes().to_vec(),
        executable: account.executable,
        rent_epoch: account.rent_epoch,
    };
    let data = account.data.to_vec();
    ReplicaAccountInfo {
        account_meta,
        hash: cached_account.hash().0.to_vec(),
        data,
    }
}

impl AccountsUpdateNotifier {
    pub fn new(
        plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        AccountsUpdateNotifier {
            plugin_manager,
            bank_forks,
        }
    }
    pub fn notify_slot_confirmed(&self, slot: Slot) {
        let mut plugin_manager = self.plugin_manager.write().unwrap();
        if plugin_manager.plugins.is_empty() {
            return;
        }

        match self.bank_forks.read().unwrap().get(slot) {
            None => {
                info!("The slot is not found {:?}", slot);
            }
            Some(bank) => {
                let accounts = bank.rc.accounts.scan_slot(slot, |account| match account {
                    LoadedAccount::Stored(stored_account_meta) => {
                        Some(accountinfo_from_stored_account_meta(&stored_account_meta))
                    }
                    LoadedAccount::Cached((_pubkey, cached_account)) => {
                        Some(accountinfo_from_cached_account(&cached_account))
                    }
                });

                for account in accounts {
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
            }
        }
    }
}
