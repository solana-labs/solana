#![allow(clippy::integer_arithmetic)]
/// Integration testing for the PostgreSQL plugin


use {
    log::*,
    serial_test::serial,
    solana_core::validator::ValidatorConfig,
};

#[test]
#[serial]
fn test_postgres_plugin() {
    libc::dl_iterate_phdr();
}