#![allow(clippy::arithmetic_side_effects)]
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

macro_rules! pubkey {
    ($arg:expr, $help:expr) => {
        $arg.takes_value(true)
            .validator(is_valid_pubkey)
            .help(concat!($help, ACCOUNT_STRING!()))
    };
}

#[macro_use]
extern crate const_format;

extern crate serde_derive;

pub mod address_lookup_table;
pub mod checks;
pub mod clap_app;
pub mod cli;
pub mod cluster_query;
pub mod compute_unit_price;
pub mod feature;
pub mod inflation;
pub mod memo;
pub mod nonce;
pub mod program;
pub mod program_v4;
pub mod spend_utils;
pub mod stake;
pub mod test_utils;
pub mod validator_info;
pub mod vote;
pub mod wallet;
