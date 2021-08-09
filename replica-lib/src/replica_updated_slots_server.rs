use tonic;
// tonic::include_proto!("accountsdb_repl");
use crate::accountsdb_repl_server::{self, ReplicaUpdatedSlotsServer};

pub(crate) struct ReplicaUpdatedSlotsServerImpl {}

impl ReplicaUpdatedSlotsServer for ReplicaUpdatedSlotsServerImpl {
    fn get_updated_slots(
        &self,
        _request: &accountsdb_repl_server::ReplicaUpdatedSlotsRequest,
    ) -> Result<accountsdb_repl_server::ReplicaUpdatedSlotsResponse, tonic::Status> {
        Err(tonic::Status::unimplemented("Not implemented yet"))
    }
}

impl ReplicaUpdatedSlotsServerImpl {
    pub fn new() -> Self {
        Self {}
    }
}
