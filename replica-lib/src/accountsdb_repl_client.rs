use {
    log::*,
    solana_sdk::clock::Slot,
    std::{net::SocketAddr, sync::Arc},
    tokio::runtime::Runtime,
    tonic::{self, transport::Endpoint, Request},
};

tonic::include_proto!("accountsdb_repl");

pub struct AccountsDbReplClient {
    client: accounts_db_repl_client::AccountsDbReplClient<tonic::transport::Channel>,
}

#[derive(Debug)]
pub enum ReplicaRpcError {
    InvalidUrl(String),
    ConnectionError(String),
    GetSlotsError(String),
    GetAccountsError(String),
}

impl From<tonic::transport::Error> for ReplicaRpcError {
    fn from(err: tonic::transport::Error) -> Self {
        ReplicaRpcError::ConnectionError(err.to_string())
    }
}

impl AccountsDbReplClient {
    pub async fn connect(rpc_peer: &SocketAddr) -> Result<Self, ReplicaRpcError> {
        let url = format!("http://{}", rpc_peer);
        let endpoint = match Endpoint::from_shared(url.to_string()) {
            Ok(endpoint) => endpoint,
            Err(e) => {
                return Err(ReplicaRpcError::InvalidUrl(e.to_string()));
            }
        };
        let client = accounts_db_repl_client::AccountsDbReplClient::connect(endpoint).await?;
        info!(
            "Successfully connected to the AccountsDb Replication server: {:?}",
            url
        );
        Ok(AccountsDbReplClient { client })
    }

    pub async fn get_confirmed_slots(
        &mut self,
        last_slot: Slot,
    ) -> Result<Vec<Slot>, ReplicaRpcError> {
        let request = ReplicaSlotConfirmationRequest {
            last_replicated_slot: last_slot,
        };
        let response = self.client.get_confirmed_slots(Request::new(request)).await;

        match response {
            Ok(response) => Ok(response.into_inner().updated_slots),
            Err(status) => Err(ReplicaRpcError::GetSlotsError(status.to_string())),
        }
    }

    pub async fn get_slot_accounts(
        &mut self,
        slot: Slot,
    ) -> Result<Vec<ReplicaAccountInfo>, ReplicaRpcError> {
        let request = ReplicaAccountsRequest { slot };
        let response = self.client.get_slot_accounts(Request::new(request)).await;

        match response {
            Ok(response) => Ok(response.into_inner().accounts),
            Err(status) => Err(ReplicaRpcError::GetAccountsError(status.to_string())),
        }
    }
}

#[derive(Clone)]
pub struct AccountsDbReplClientServiceConfig {
    pub worker_threads: usize,
    pub replica_server_addr: SocketAddr,
}

/// The service wrapper over AccountsDbReplClient to make it run in the tokio runtime
pub struct AccountsDbReplClientService {
    runtime: Arc<Runtime>,
    accountsdb_repl_client: AccountsDbReplClient,
}

impl AccountsDbReplClientService {
    pub fn new(config: AccountsDbReplClientServiceConfig) -> Result<Self, ReplicaRpcError> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(config.worker_threads)
                .thread_name("sol-accountsdb-repl-wrk")
                .enable_all()
                .build()
                .expect("Runtime"),
        );

        let accountsdb_repl_client =
            runtime.block_on(AccountsDbReplClient::connect(&config.replica_server_addr))?;

        Ok(Self {
            runtime,
            accountsdb_repl_client,
        })
    }

    pub fn get_confirmed_slots(&mut self, last_slot: Slot) -> Result<Vec<Slot>, ReplicaRpcError> {
        self.runtime
            .block_on(self.accountsdb_repl_client.get_confirmed_slots(last_slot))
    }

    pub fn get_slot_accounts(
        &mut self,
        slot: Slot,
    ) -> Result<Vec<ReplicaAccountInfo>, ReplicaRpcError> {
        self.runtime
            .block_on(self.accountsdb_repl_client.get_slot_accounts(slot))
    }
}
