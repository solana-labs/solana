use std::str::FromStr;

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
    solana_sdk::{
        account::{Account, ReadableAccount},
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{
        collections::HashSet,
        sync::{Arc, RwLock},
    },
};

pub(crate) struct AccountsUpdateNotifier {
    plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
    bank_forks: Arc<RwLock<BankForks>>,
    accounts_selector: AccountsSelector,
}

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

fn accountinfo_from_stored_account_meta(
    accounts_selector: &AccountsSelector,
    stored_account_meta: &StoredAccountMeta,
) -> Option<ReplicaAccountInfo> {
    if !accounts_selector.is_account_selected(
        &stored_account_meta.meta.pubkey,
        &stored_account_meta.account_meta.owner,
    ) {
        return None;
    }

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
        hash: bs58::encode(stored_account_meta.hash.0).into_string(),
        data,
    })
}

#[allow(dead_code)]
fn accountinfo_from_readable_account(
    accounts_selector: &AccountsSelector,
    pubkey: &Pubkey,
    hash: &Hash,
    account: &impl ReadableAccount,
) -> Option<ReplicaAccountInfo> {
    if !accounts_selector.is_account_selected(pubkey, account.owner()) {
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
        hash: bs58::encode(hash).into_string(),
        data,
    })
}

fn accountinfo_from_cached_account(
    accounts_selector: &AccountsSelector,
    cached_account: &CachedAccount,
) -> Option<ReplicaAccountInfo> {
    if !accounts_selector
        .is_account_selected(&cached_account.pubkey(), cached_account.account.owner())
    {
        return None;
    }

    let account = Account::from(cached_account.account.clone());
    let account_meta = ReplicaAccountMeta {
        pubkey: bs58::encode(cached_account.pubkey()).into_string(),
        lamports: account.lamports,
        owner: bs58::encode(account.owner).into_string(),
        executable: account.executable,
        rent_epoch: account.rent_epoch,
    };
    let data = account.data.to_vec();
    Some(ReplicaAccountInfo {
        account_meta,
        hash: bs58::encode(cached_account.hash().0).into_string(),
        data,
    })
}

impl AccountsUpdateNotifier {
    pub fn new(
        plugin_manager: Arc<RwLock<AccountsDbPluginManager>>,
        bank_forks: Arc<RwLock<BankForks>>,
        accounts_selector: AccountsSelector,
    ) -> Self {
        AccountsUpdateNotifier {
            plugin_manager,
            bank_forks,
            accounts_selector,
        }
    }

    #[allow(dead_code)]
    pub fn notify_account_update(self, _slot: Slot, _account: &impl ReadableAccount) {}

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
                        accountinfo_from_stored_account_meta(
                            &self.accounts_selector,
                            &stored_account_meta,
                        )
                    }
                    LoadedAccount::Cached((_pubkey, cached_account)) => {
                        accountinfo_from_cached_account(&self.accounts_selector, &cached_account)
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
