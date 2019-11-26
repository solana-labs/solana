use crate::main::SOLANA_SYS_TUNER_PATH;
use unix_socket::UnixStream;

pub mod main;

pub fn request_system_tuning() {
    let _ignore = UnixStream::connect(SOLANA_SYS_TUNER_PATH);
}
