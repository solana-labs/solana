use {
    crate::accountsdb_repl_server::{self, ReplicaAccountsServer},
    solana_runtime::{bank_forks::BankForks},
    std::{
        sync::{Arc, RwLock},
    },
};

pub(crate) struct ReplicaAccountsServerImpl {
    bank_forks: Arc<RwLock<BankForks>>,
}


use std::thread;

impl ReplicaAccountsServer for ReplicaAccountsServerImpl {
    fn get_slot_accounts(
        &self,
        request: &accountsdb_repl_server::ReplicaAccountsRequest,
    ) -> Result<accountsdb_repl_server::ReplicaAccountsResponse, tonic::Status> {

        let slot = request.slot;

        match self.bank_forks.read().unwrap().get(slot) {
            None => Err(tonic::Status::not_found("The slot is not found")),
            Some(bank) => {
                let snapshot_storages = bank.get_snapshot_storages();
                for snapshot_storage in snapshot_storages {
                    for account_storage_entry in snapshot_storage {

                    }
                }

                Err(tonic::Status::unimplemented("The function is not implemented yet"))
            }
        }
    }

    fn join(&mut self) -> thread::Result<()> {
        Ok(())
    }
}

impl ReplicaAccountsServerImpl {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        Self {bank_forks}
    }
}
