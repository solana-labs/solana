use {
    crate::{
        accountsdb_repl_server::AccountsDbReplServer,
        replica_accounts_server::ReplicaAccountsServerImpl,
        replica_updated_slots_server::ReplicaUpdatedSlotsServerImpl,
    },
    std::sync::{Arc, RwLock},
};

pub struct AccountsDbReplServerFactory {}

impl AccountsDbReplServerFactory {
    pub fn build_accountsdb_repl_server() -> AccountsDbReplServer {
        AccountsDbReplServer::new(
            Arc::new(RwLock::new(ReplicaUpdatedSlotsServerImpl::new())),
            Arc::new(RwLock::new(ReplicaAccountsServerImpl::new())),
        )
    }
}
