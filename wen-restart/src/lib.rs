pub(crate) mod solana {
    pub(crate) mod wen_restart_proto {
        include!(concat!(env!("OUT_DIR"), "/solana.wen_restart_proto.rs"));
    }
}

pub mod wen_restart;
