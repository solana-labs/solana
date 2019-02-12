use solana::blocktree::{create_tmp_sample_ledger, BlocktreeConfig};
use solana_sdk::signature::{Keypair, KeypairUtil};

use assert_cmd::prelude::*;
use std::process::Command;
use std::process::Output;
use std::sync::Arc;

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
    let keypair = Arc::new(Keypair::new());
    let (_mint_keypair, ledger_path, _tick_height, _last_entry_height, _last_id, _last_entry_id) =
        create_tmp_sample_ledger(
            "test_ledger_tool_nominal",
            100,
            9,
            keypair.pubkey(),
            50,
            &BlocktreeConfig::default(),
        );

    // Basic validation
    let output = run_ledger_tool(&["-l", &ledger_path, "verify"]);
    assert!(output.status.success());

    // Print everything
    let output = run_ledger_tool(&["-l", &ledger_path, "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), 10);

    // Only print the first 5 items
    let output = run_ledger_tool(&["-l", &ledger_path, "-n", "5", "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), 5);

    // Skip entries with no hashes
    let output = run_ledger_tool(&["-l", &ledger_path, "-h", "1", "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), 10);

    // Skip entries with fewer than 2 hashes (skip everything)
    let output = run_ledger_tool(&["-l", &ledger_path, "-h", "2", "print"]);
    assert!(output.status.success());
    assert_eq!(count_newlines(&output.stdout), 0);
}
