#[derive(PartialEq, Debug)]
pub enum CrdsGossipError {
    NoPeers,
    PushMessageTimeout,
    PushMessageOldVersion,
    BadPruneDestination,
    PruneMessageTimeout,
}
