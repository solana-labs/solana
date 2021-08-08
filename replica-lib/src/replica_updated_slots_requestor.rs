use tonic::{self, transport::Endpoint, Request};

tonic::include_proto!("accountsdb_repl");

use solana_sdk::clock::Slot;
use std::net::SocketAddr;

pub struct ReplicaUpdatedSlotsRequestor {
    client: accounts_db_repl_client::AccountsDbReplClient<tonic::transport::Channel>,
}

pub enum ReplicaRpcError {
    InvalidUrl(String),
    ConnectionError(String),
    GetSlotsError(String),
}

impl From<tonic::transport::Error> for ReplicaRpcError {
    fn from(err: tonic::transport::Error) -> Self {
        ReplicaRpcError::ConnectionError(err.to_string())
    }
}
impl ReplicaUpdatedSlotsRequestor {
    pub async fn connect(rpc_peer: &SocketAddr) -> Result<Self, ReplicaRpcError> {
        let url = format!("http://{}", rpc_peer);
        let endpoint = match Endpoint::from_shared(url.to_string()) {
            Ok(endpoint) => endpoint,
            Err(e) => {
                return Err(ReplicaRpcError::InvalidUrl(e.to_string()));
            }
        };
        let client = accounts_db_repl_client::AccountsDbReplClient::connect(endpoint).await?;
        Ok(ReplicaUpdatedSlotsRequestor { client })
    }

    pub async fn get_updated_slots(
        &mut self,
        last_slot: Slot,
    ) -> Result<Vec<Slot>, ReplicaRpcError> {
        let request = ReplicaUpdatedSlotsRequest {
            last_replicated_slot: last_slot,
        };
        let response = self.client.get_updated_slots(Request::new(request)).await;

        match response {
            Ok(response) => Ok(response.into_inner().updated_slots),
            Err(status) => Err(ReplicaRpcError::GetSlotsError(status.to_string())),
        }
    }
}
