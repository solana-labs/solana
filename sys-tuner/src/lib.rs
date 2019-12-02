use crate::main::SOLANA_SYS_TUNER_PATH;
use log::*;
use unix_socket::UnixStream;

pub mod main;

pub fn request_realtime_poh() {
    info!("Sending tuning request");
    let status = UnixStream::connect(SOLANA_SYS_TUNER_PATH);
    info!("Tuning request status {:?}", status);
}
