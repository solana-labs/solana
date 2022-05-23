#[derive(PartialEq, Eq, Debug)]
pub enum CrdsGossipError {
    NoPeers,
    PushMessageTimeout,
    PushMessageOldVersion,
    BadPruneDestination,
    PruneMessageTimeout,
}
