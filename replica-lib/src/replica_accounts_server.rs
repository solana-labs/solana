use {
    crate::accountsdb_repl_server::{
        self, ReplicaAccountData, ReplicaAccountInfo, ReplicaAccountMeta, ReplicaAccountsServer,
    },
    solana_runtime::{
        accounts_cache::CachedAccount, accounts_db::LoadedAccount, append_vec::StoredAccountMeta,
        bank_forks::BankForks,
    },
    solana_sdk::account::Account,
    std::{
        cmp::Eq,
        sync::{Arc, RwLock},
        thread,
    },
};

pub(crate) struct ReplicaAccountsServerImpl {
    bank_forks: Arc<RwLock<BankForks>>,
}

impl Eq for ReplicaAccountInfo {}

impl ReplicaAccountInfo {
    fn from_stored_account_meta(stored_account_meta: &StoredAccountMeta) -> Self {
        let account_meta = Some(ReplicaAccountMeta {
            pubkey: stored_account_meta.meta.pubkey.to_bytes().to_vec(),
            lamports: stored_account_meta.account_meta.lamports,
            owner: stored_account_meta.account_meta.owner.to_bytes().to_vec(),
            executable: stored_account_meta.account_meta.executable,
            rent_epoch: stored_account_meta.account_meta.rent_epoch,
        });
        let data = Some(ReplicaAccountData {
            data: stored_account_meta.data.to_vec(),
        });
        ReplicaAccountInfo {
            account_meta,
            hash: stored_account_meta.hash.0.to_vec(),
            data,
        }
    }

    fn from_cached_account(cached_account: &CachedAccount) -> Self {
        let account = Account::from(cached_account.account.clone());
        let account_meta = Some(ReplicaAccountMeta {
            pubkey: cached_account.pubkey().to_bytes().to_vec(),
            lamports: account.lamports,
            owner: account.owner.to_bytes().to_vec(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        });
        let data = Some(ReplicaAccountData {
            data: account.data.to_vec(),
        });
        ReplicaAccountInfo {
            account_meta,
            hash: cached_account.hash().0.to_vec(),
            data,
        }
    }
}

impl ReplicaAccountsServer for ReplicaAccountsServerImpl {
    fn get_slot_accounts(
        &self,
        request: &accountsdb_repl_server::ReplicaAccountsRequest,
    ) -> Result<accountsdb_repl_server::ReplicaAccountsResponse, tonic::Status> {
        let slot = request.slot;

        match self.bank_forks.read().unwrap().get(slot) {
            None => Err(tonic::Status::not_found("The slot is not found")),
            Some(bank) => {
                let accounts = bank.rc.accounts.scan_slot(slot, |account| match account {
                    LoadedAccount::Stored(stored_account_meta) => Some(
                        ReplicaAccountInfo::from_stored_account_meta(&stored_account_meta),
                    ),
                    LoadedAccount::Cached((_pubkey, cached_account)) => {
                        Some(ReplicaAccountInfo::from_cached_account(&cached_account))
                    }
                });

                Ok(accountsdb_repl_server::ReplicaAccountsResponse { accounts })
            }
        }
    }

    fn join(&mut self) -> thread::Result<()> {
        Ok(())
    }
}

impl ReplicaAccountsServerImpl {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        Self { bank_forks }
    }
}
