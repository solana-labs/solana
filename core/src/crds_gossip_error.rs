#[derive(PartialEq, Debug)]
pub enum CrdsGossipError {
    NoPeers,
    PushMessageTimeout,
    PushMessageAlreadyReceived,
    PushMessageOldVersion,
    BadPruneDestination,
    PruneMessageTimeout,
}
