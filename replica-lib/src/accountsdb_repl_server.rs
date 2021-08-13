use std::{sync::{Arc, RwLock}, thread};
use tonic::{self};

tonic::include_proto!("accountsdb_repl");

pub trait ReplicaUpdatedSlotsServer {
    fn get_updated_slots(
        &self,
        request: &ReplicaUpdatedSlotsRequest,
    ) -> Result<ReplicaUpdatedSlotsResponse, tonic::Status>;

    fn join(&self) -> thread::Result<()>;
}

pub trait ReplicaAccountsServer {
    fn get_slot_accounts(
        &self,
        request: &ReplicaAccountsRequest,
    ) -> Result<ReplicaAccountsResponse, tonic::Status>;
    fn join(&self) -> thread::Result<()>;
}

pub struct AccountsDbReplServer {
    updated_slots_server: Arc<RwLock<dyn ReplicaUpdatedSlotsServer + Sync + Send>>,
    accounts_server: Arc<RwLock<dyn ReplicaAccountsServer + Sync + Send>>,
}

#[tonic::async_trait]
impl accounts_db_repl_server::AccountsDbRepl for AccountsDbReplServer {
    async fn get_updated_slots(
        &self,
        request: tonic::Request<ReplicaUpdatedSlotsRequest>,
    ) -> Result<tonic::Response<ReplicaUpdatedSlotsResponse>, tonic::Status> {
        let server = self.updated_slots_server.read().unwrap();
        let result = server.get_updated_slots(&request.into_inner());
        result.map(tonic::Response::new)
    }

    async fn get_slot_accounts(
        &self,
        request: tonic::Request<ReplicaAccountsRequest>,
    ) -> Result<tonic::Response<ReplicaAccountsResponse>, tonic::Status> {
        let server = self.accounts_server.read().unwrap();
        let result = server.get_slot_accounts(&request.into_inner());
        result.map(tonic::Response::new)
    }
}

impl AccountsDbReplServer {
    pub fn new(
        updated_slots_server: Arc<RwLock<dyn ReplicaUpdatedSlotsServer + Sync + Send>>,
        accounts_server: Arc<RwLock<dyn ReplicaAccountsServer + Sync + Send>>,
    ) -> Self {
        Self {
            updated_slots_server,
            accounts_server,
        }
    }

    pub fn join(&self) -> thread::Result<()> {
        self.updated_slots_server.read().unwrap().join()?;
        self.accounts_server.read().unwrap().join()
    }
}
