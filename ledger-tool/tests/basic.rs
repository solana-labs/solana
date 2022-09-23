use {
    assert_cmd::prelude::*,
    solana_ledger::{
        create_new_tmp_ledger, create_new_tmp_ledger_fifo, genesis_utils::create_genesis_config,
    },
    std::process::{Command, Output},
};

fn run_ledger_tool(args: &[&str]) -> Output {
    Command::cargo_bin(env!("CARGO_PKG_NAME"))
        .unwrap()
        .args(args)
        .output()
        .unwrap()
}

fn count_newlines(chars: &[u8]) -> usize {
    bytecount::count(chars, b'\n')
}

#[test]
fn bad_arguments() {
    // At least a ledger path is required
    assert!(!run_ledger_tool(&[]).status.success());

    // Invalid ledger path should fail
    assert!(!run_ledger_tool(&["-l", "invalid_ledger", "verify"])
        .status
        .success());
}

fn nominal_test_helper(ledger_path: &str, ticks: usize) {
    let meta_lines = 2;
    let summary_lines = 1;

    let output = run_ledger_tool(&["-l", ledger_path, "verify"]);
    assert!(output.status.success());

    // Print everything
    let output = run_ledger_tool(&["-l", ledger_path, "print", "-vvv"]);
    assert!(output.status.success());
    assert_eq!(
        count_newlines(&output.stdout)
            .saturating_sub(meta_lines)
            .saturating_sub(summary_lines),
        ticks
    );
}

#[test]
fn nominal_default() {
    let genesis_config = create_genesis_config(100).genesis_config;
    let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);
    nominal_test_helper(
        ledger_path.to_str().unwrap(),
        genesis_config.ticks_per_slot as usize,
    );
}

#[test]
fn nominal_fifo() {
    let genesis_config = create_genesis_config(100).genesis_config;
    let (ledger_path, _blockhash) = create_new_tmp_ledger_fifo!(&genesis_config);
    nominal_test_helper(
        ledger_path.to_str().unwrap(),
        genesis_config.ticks_per_slot as usize,
    );
}
