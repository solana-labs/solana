use {
    solana_gossip::{cluster_info::ClusterInfoError, contact_info},
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum RepairVerifyError {
    #[error("IdMismatch")]
    IdMismatch,
    #[error("Malformed")]
    Malformed,
    #[error("SelfRepair")]
    SelfRepair,
    #[error("SigVerify")]
    SigVerify,
    #[error("TimeSkew")]
    TimeSkew,
    #[error("Unsigned")]
    Unsigned,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    ClusterInfo(#[from] ClusterInfoError),
    #[error(transparent)]
    InvalidContactInfo(#[from] contact_info::Error),
    #[error(transparent)]
    RepairVerify(#[from] RepairVerifyError),
    #[error("Send Error")]
    SendError,
    #[error(transparent)]
    Serialize(#[from] std::boxed::Box<bincode::ErrorKind>),
    #[error(transparent)]
    WeightedIndex(#[from] rand::distributions::weighted::WeightedError),
}

pub type Result<T> = std::result::Result<T, Error>;
