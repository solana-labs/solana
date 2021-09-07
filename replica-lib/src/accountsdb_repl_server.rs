use {
    futures_util::FutureExt,
    log::*,
    std::{
        net::SocketAddr,
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
    tokio::{
        runtime::Runtime,
        sync::oneshot::{self, Receiver, Sender},
    },
    tonic::{self, transport},
};

tonic::include_proto!("accountsdb_repl");

pub trait ReplicaSlotConfirmationServer {
    fn get_confirmed_slots(
        &self,
        request: &ReplicaSlotConfirmationRequest,
    ) -> Result<ReplicaSlotConfirmationResponse, tonic::Status>;

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
    confirmed_slots_server: Arc<RwLock<dyn ReplicaSlotConfirmationServer + Sync + Send>>,
    accounts_server: Arc<RwLock<dyn ReplicaAccountsServer + Sync + Send>>,
}

/// Implementing the AccountsDbRepl interface declared by the protocol
#[tonic::async_trait]
impl accounts_db_repl_server::AccountsDbRepl for AccountsDbReplServer {
    async fn get_confirmed_slots(
        &self,
        request: tonic::Request<ReplicaSlotConfirmationRequest>,
    ) -> Result<tonic::Response<ReplicaSlotConfirmationResponse>, tonic::Status> {
        let server = self.confirmed_slots_server.read().unwrap();
        let result = server.get_confirmed_slots(&request.into_inner());
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
        confirmed_slots_server: Arc<RwLock<dyn ReplicaSlotConfirmationServer + Sync + Send>>,
        accounts_server: Arc<RwLock<dyn ReplicaAccountsServer + Sync + Send>>,
    ) -> Self {
        Self {
            confirmed_slots_server,
            accounts_server,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.confirmed_slots_server.write().unwrap().join()?;
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
    exit_signal_sender: Sender<()>,
}

impl AccountsDbReplService {
    pub fn new(
        config: AccountsDbReplServiceConfig,
        confirmed_slots_server: Arc<RwLock<dyn ReplicaSlotConfirmationServer + Sync + Send>>,
        accounts_server: Arc<RwLock<dyn ReplicaAccountsServer + Sync + Send>>,
    ) -> Self {
        let accountsdb_repl_server =
            AccountsDbReplServer::new(confirmed_slots_server, accounts_server);

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
        let (exit_signal_sender, exit_signal_receiver) = oneshot::channel::<()>();

        let thread = Builder::new()
            .name("sol-accountsdb-repl-rt".to_string())
            .spawn(move || {
                Self::run_accountsdb_repl_server_in_runtime(
                    config,
                    runtime,
                    server_cloned,
                    exit_signal_receiver,
                );
            })
            .unwrap();

        Self {
            accountsdb_repl_server,
            thread,
            exit_signal_sender,
        }
    }

    async fn run_accountsdb_repl_server(
        config: AccountsDbReplServiceConfig,
        server: AccountsDbReplServer,
        exit_signal: Receiver<()>,
    ) -> Result<(), tonic::transport::Error> {
        info!(
            "Running AccountsDbReplServer at the endpoint: {:?}",
            config.replica_server_addr
        );
        transport::Server::builder()
            .add_service(accounts_db_repl_server::AccountsDbReplServer::new(server))
            .serve_with_shutdown(config.replica_server_addr, exit_signal.map(drop))
            .await
    }

    fn run_accountsdb_repl_server_in_runtime(
        config: AccountsDbReplServiceConfig,
        runtime: Arc<Runtime>,
        server: AccountsDbReplServer,
        exit_signal: Receiver<()>,
    ) {
        let result = runtime.block_on(Self::run_accountsdb_repl_server(
            config,
            server,
            exit_signal,
        ));
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
        let _ = self.exit_signal_sender.send(());
        self.accountsdb_repl_server.join()?;
        self.thread.join()
    }
}
