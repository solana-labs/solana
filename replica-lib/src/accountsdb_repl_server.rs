use {
    log::*,
    std::{
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
    tokio::runtime::Runtime,
    tonic::{self, transport},
};

tonic::include_proto!("accountsdb_repl");

pub trait ReplicaUpdatedSlotsServer {
    fn get_updated_slots(
        &self,
        request: &ReplicaUpdatedSlotsRequest,
    ) -> Result<ReplicaUpdatedSlotsResponse, tonic::Status>;

    fn join(&mut self) -> thread::Result<()>;
}

pub trait ReplicaAccountsServer {
    fn get_slot_accounts(
        &self,
        request: &ReplicaAccountsRequest,
    ) -> Result<ReplicaAccountsResponse, tonic::Status>;
    fn join(&mut self) -> thread::Result<()>;
}

#[derive(Clone)]
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

    pub fn join(self) -> thread::Result<()> {
        self.updated_slots_server.write().unwrap().join()?;
        self.accounts_server.write().unwrap().join()
    }
}

async fn run_accountsdb_repl_server(
    server: AccountsDbReplServer,
) -> Result<(), tonic::transport::Error> {
    transport::Server::builder()
        .add_service(accounts_db_repl_server::AccountsDbReplServer::new(server))
        .serve("[::1]:50051".parse().unwrap())
        .await
}

fn run_accountsdb_repl_server_in_runtime(runtime: Arc<Runtime>, server: AccountsDbReplServer) {
    let result = runtime.block_on(run_accountsdb_repl_server(server));
    match result {
        Ok(_) => {
            info!("AccountsDbReplServer finished");
        }
        Err(err) => {
            error!("AccountsDbReplServer finished in error: {:}?", err);
        }
    }
}

pub fn start(server: &AccountsDbReplServer) -> JoinHandle<()> {
    let server = server.clone();
    let worker_thread = 1;
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_thread)
            .thread_name("sol-accountsdb-repl-wrk")
            .enable_all()
            .build()
            .expect("Runtime"),
    );

    Builder::new()
        .name("sol-accountsdb-repl-rt".to_string())
        .spawn(move || {
            run_accountsdb_repl_server_in_runtime(runtime, server);
        })
        .unwrap()
}
