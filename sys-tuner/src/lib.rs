use log::*;

pub const SAFECOIN_SYS_TUNER_PATH: &str = "/tmp/safecoin-sys-tuner";

#[cfg(unix)]
pub fn request_realtime_poh() {
    info!("Sending tuning request");
    let status = unix_socket::UnixStream::connect(SAFECOIN_SYS_TUNER_PATH);
    match status {
        Ok(_) => info!("Successfully sent tuning request"),
        Err(err) => warn!(
            "Failed to send tuning request, is `safecoin-sys-tuner` running? {:?}",
            err
        ),
    }
}

#[cfg(not(unix))]
pub fn request_realtime_poh() {
    info!("Tuning request ignored on this platform");
}
