use tonic;
tonic::include_proto!("accountsdb_repl");

trait ReplicaUpdatedSlotsServer {
    fn get_updated_slots(request: &ReplicaUpdatedSlotsRequest) -> Result<ReplicaUpdatedSlotsResponse, tonic::Status>;
}

struct ReplicaUpdatedSlotsServerImpl {

}


impl ReplicaUpdatedSlotsServer for ReplicaUpdatedSlotsServerImpl {
    fn get_updated_slots(_request: &ReplicaUpdatedSlotsRequest) -> Result<ReplicaUpdatedSlotsResponse, tonic::Status>  {
        Err(tonic::Status::unimplemented("Not implemented yet"))
    }
   
}