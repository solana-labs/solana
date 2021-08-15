use {
    log::*,
    std::{
        net::SocketAddr,
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
struct AccountsDbReplServer {
    updated_slots_server: Arc<RwLock<dyn ReplicaUpdatedSlotsServer + Sync + Send>>,
    accounts_server: Arc<RwLock<dyn ReplicaAccountsServer + Sync + Send>>,
}

/// Implementing the AccountsDbRepl interface declared by the protocol
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

#[derive(Clone)]
pub struct AccountsDbReplServiceConfig {
    pub worker_threads: usize,
    pub replica_server_addr: SocketAddr,
}

/// The service wraps the AccountsDbReplServer to make runnable in the tokio runtime
/// and handles start and stop of the service.
pub struct AccountsDbReplService {
    accountsdb_repl_server: AccountsDbReplServer,
    thread: JoinHandle<()>,
}

impl AccountsDbReplService {
    pub fn new(
        config: AccountsDbReplServiceConfig,
        updated_slots_server: Arc<RwLock<dyn ReplicaUpdatedSlotsServer + Sync + Send>>,
        accounts_server: Arc<RwLock<dyn ReplicaAccountsServer + Sync + Send>>,
    ) -> Self {
        let accountsdb_repl_server =
            AccountsDbReplServer::new(updated_slots_server, accounts_server);

        let worker_threads = config.worker_threads;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(worker_threads)
                .thread_name("sol-accountsdb-repl-wrk")
                .enable_all()
                .build()
                .expect("Runtime"),
        );

        let server_cloned = accountsdb_repl_server.clone();
        let thread = Builder::new()
            .name("sol-accountsdb-repl-rt".to_string())
            .spawn(move || {
                Self::run_accountsdb_repl_server_in_runtime(config, runtime, server_cloned);
            })
            .unwrap();

        Self {
            accountsdb_repl_server,
            thread,
        }
    }

    async fn run_accountsdb_repl_server(
        config: AccountsDbReplServiceConfig,
        server: AccountsDbReplServer,
    ) -> Result<(), tonic::transport::Error> {
        transport::Server::builder()
            .add_service(accounts_db_repl_server::AccountsDbReplServer::new(server))
            .serve(config.replica_server_addr)
            .await
    }

    fn run_accountsdb_repl_server_in_runtime(
        config: AccountsDbReplServiceConfig,
        runtime: Arc<Runtime>, server: AccountsDbReplServer) {
        let result = runtime.block_on(Self::run_accountsdb_repl_server(config, server));
        match result {
            Ok(_) => {
                info!("AccountsDbReplServer finished");
            }
            Err(err) => {
                error!("AccountsDbReplServer finished in error: {:}?", err);
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.accountsdb_repl_server.join()?;
        self.thread.join()
    }
}
