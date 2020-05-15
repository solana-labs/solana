macro_rules! ACCOUNT_STRING {
    () => {
        r#", one of:
  * a base58-encoded public key
  * a path to a keypair file
  * a hyphen; signals a JSON-encoded keypair on stdin
  * the 'ASK' keyword; to recover a keypair via its seed phrase
  * a hardware wallet keypair URL (i.e. usb://ledger)"#
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

#[macro_use]
extern crate serde_derive;

pub mod checks;
pub mod cli;
pub mod cli_output;
pub mod cluster_query;
pub mod display;
pub mod nonce;
pub mod offline;
pub mod spend_utils;
pub mod stake;
pub mod test_utils;
pub mod validator_info;
pub mod vote;
