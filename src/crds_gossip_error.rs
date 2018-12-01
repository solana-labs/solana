#[derive(PartialEq, Debug)]
pub enum CrdsGossipError {
    NoPeers,
    PushMessageTimeout,
    PushMessagePrune,
    PushMessageOldVersion,
    BadPruneDestination,
    PruneMessageTimeout,
}
