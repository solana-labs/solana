use crate::accountsdb_repl_server::{self, ReplicaAccountsServer};

pub(crate) struct ReplicaAccountsServerImpl {}
use std::{sync::{Arc, RwLock}, thread};

impl ReplicaAccountsServer for ReplicaAccountsServerImpl {
    fn get_slot_accounts(
        &self,
        _request: &accountsdb_repl_server::ReplicaAccountsRequest,
    ) -> Result<accountsdb_repl_server::ReplicaAccountsResponse, tonic::Status> {
        Err(tonic::Status::unimplemented("Not implemented yet"))
    }

    fn join(&self) -> thread::Result<()> {
        Ok(())
    }
}

impl ReplicaAccountsServerImpl {
    pub fn new() -> Self {
        Self {}
    }


}
