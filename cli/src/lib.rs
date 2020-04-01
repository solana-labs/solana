macro_rules! ACCOUNT_STRING {
    () => {
        r#"Can be:
  * a bs58 pubkey string
  * path to keypair file
  * '-' to take json-encoded keypair string from stdin
  * 'ASK' to ask for a passphrase
  * path to hardware wallet (usb://..)"#
    };
}

#[macro_use]
macro_rules! pubkey {
    ($arg:expr, $help:expr) => {
        $arg.takes_value(true)
            .validator(is_valid_pubkey)
            .help(concat!($help, ACCOUNT_STRING!()))
    };
}

pub mod cli;
pub mod cluster_query;
pub mod display;
pub mod nonce;
pub mod offline;
pub mod stake;
pub mod storage;
pub mod validator_info;
pub mod vote;
