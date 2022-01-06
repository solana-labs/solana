#![allow(clippy::integer_arithmetic)]
/// Integration testing for the PostgreSQL plugin


use {
    log::*,
    libloading::{Library, Symbol},
    serial_test::serial,
    solana_core::validator::ValidatorConfig,
    solana_accountsdb_plugin_postgres,
};

#[test]
#[serial]
fn test_postgres_plugin() {
    unsafe {
        let lib = Library::new("libsolana_accountsdb_plugin_postgres.so").unwrap();
    }
}