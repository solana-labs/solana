pub(crate) mod epoch_stakes_map;
pub(crate) mod heaviest_fork_aggregate;
pub(crate) mod last_voted_fork_slots_aggregate;

pub(crate) mod solana {
    pub(crate) mod wen_restart_proto {
        include!(concat!(env!("OUT_DIR"), "/solana.wen_restart_proto.rs"));
    }
}

pub mod wen_restart;
