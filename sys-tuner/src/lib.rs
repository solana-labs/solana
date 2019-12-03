use log::*;

pub const SOLANA_SYS_TUNER_PATH: &str = "/tmp/solana-sys-tuner";

#[cfg(unix)]
pub fn request_realtime_poh() {
    info!("Sending tuning request");
    let status = unix_socket::UnixStream::connect(SOLANA_SYS_TUNER_PATH);
    info!("Tuning request status {:?}", status);
}

#[cfg(not(unix))]
pub fn request_realtime_poh() {
    info!("Tuning request ignored on this platform");
}
