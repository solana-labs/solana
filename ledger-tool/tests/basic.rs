use assert_cmd::prelude::*;
use solana_ledger::create_new_tmp_ledger;
use solana_ledger::genesis_utils::create_genesis_config;
use std::process::Command;
use std::process::Output;

fn run_ledger_tool(args: &[&str]) -> Output {
    Command::cargo_bin(env!("CARGO_PKG_NAME"))
        .unwrap()
        .args(args)
        .output()
        .unwrap()
}

fn count_newlines(chars: &[u8]) -> usize {
    chars.iter().filter(|&c| *c == '\n' as u8).count()
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

#[test]
fn nominal() {
    let genesis_config = create_genesis_config(100).genesis_config;
    let ticks_per_slot = genesis_config.ticks_per_slot;
    let meta_lines = 2;

    let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);
    let ticks = ticks_per_slot as usize;

    let ledger_path = ledger_path.to_str().unwrap();

    // Basic validation
    let output = run_ledger_tool(&["-l", &ledger_path, "verify"]);
    assert!(output.status.success());

    // Print everything
    let output = run_ledger_tool(&["-l", &ledger_path, "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), ticks + meta_lines + 1);
}
