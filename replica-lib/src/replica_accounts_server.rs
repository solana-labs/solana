use crate::accountsdb_repl_server::{self, ReplicaAccountsServer};

pub(crate) struct ReplicaAccountsServerImpl {}

impl ReplicaAccountsServer for ReplicaAccountsServerImpl {
    fn get_slot_accounts(
        &self,
        _request: &accountsdb_repl_server::ReplicaAccountsRequest,
    ) -> Result<accountsdb_repl_server::ReplicaAccountsResponse, tonic::Status> {
        Err(tonic::Status::unimplemented("Not implemented yet"))
    }
}

impl ReplicaAccountsServerImpl {
    pub fn new() -> Self {
        Self {}
    }
}
