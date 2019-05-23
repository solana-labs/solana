#[macro_use]
extern crate solana;

use assert_cmd::prelude::*;
use solana::blocktree::create_new_tmp_ledger;
use solana::genesis_utils::create_genesis_block;
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
    let genesis_block = create_genesis_block(100).genesis_block;
    let ticks_per_slot = genesis_block.ticks_per_slot;

    let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
    let ticks = ticks_per_slot as usize;

    // Basic validation
    let output = run_ledger_tool(&["-l", &ledger_path, "verify"]);
    assert!(output.status.success());

    // Print everything
    let output = run_ledger_tool(&["-l", &ledger_path, "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), ticks);

    // Only print the first 5 items
    let output = run_ledger_tool(&["-l", &ledger_path, "-n", "5", "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), 5);

    // Skip entries with no hashes
    let output = run_ledger_tool(&["-l", &ledger_path, "-h", "1", "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), ticks);

    // Skip entries with fewer than 2 hashes (skip everything)
    let output = run_ledger_tool(&["-l", &ledger_path, "-h", "2", "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), 0);
}
