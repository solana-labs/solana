use log::*;
use unix_socket::UnixStream;

pub const SOLANA_SYS_TUNER_PATH: &str = "/tmp/solana-sys-tuner";

pub fn request_realtime_poh() {
    info!("Sending tuning request");
    let status = UnixStream::connect(SOLANA_SYS_TUNER_PATH);
    info!("Tuning request status {:?}", status);
}
