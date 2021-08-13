use {
    crate::{
        accountsdb_repl_server::AccountsDbReplServer,
        replica_accounts_server::ReplicaAccountsServerImpl,
        replica_updated_slots_server::ReplicaUpdatedSlotsServerImpl,
    },
    crossbeam_channel::Receiver,
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub struct AccountsDbReplServerFactory {}

impl AccountsDbReplServerFactory {
    pub fn build_accountsdb_repl_server(
        confirmed_bank_receiver: Receiver<Slot>,
    ) -> AccountsDbReplServer {
        AccountsDbReplServer::new(
            Arc::new(RwLock::new(ReplicaUpdatedSlotsServerImpl::new(
                confirmed_bank_receiver,
            ))),
            Arc::new(RwLock::new(ReplicaAccountsServerImpl::new())),
        )
    }
}
