use {
    assert_cmd::prelude::*,
    solana_entry::entry,
    solana_ledger::{
        blockstore, blockstore::Blockstore, blockstore_options::ShredStorageType,
        create_new_tmp_ledger, create_new_tmp_ledger_fifo, genesis_utils::create_genesis_config,
        get_tmp_ledger_path_auto_delete,
    },
    solana_sdk::hash::Hash,
    std::{
        fs,
        path::Path,
        process::{Command, Output},
    },
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

fn insert_test_shreds(ledger_path: &Path, ending_slot: u64) {
    let blockstore = Blockstore::open(ledger_path).unwrap();
    for i in 1..ending_slot {
        let entries = entry::create_ticks(1, 0, Hash::default());
        let shreds = blockstore::entries_to_test_shreds(
            &entries, i, 0, false, 0, /*merkle_variant:*/ true,
        );
        blockstore.insert_shreds(shreds, None, false).unwrap();
    }
}

fn ledger_tool_copy_test(src_shred_compaction: &str, dst_shred_compaction: &str) {
    let genesis_config = create_genesis_config(100).genesis_config;

    let (ledger_path, _blockhash) = match src_shred_compaction {
        "fifo" => create_new_tmp_ledger_fifo!(&genesis_config),
        _ => create_new_tmp_ledger!(&genesis_config),
    };
    const LEDGER_TOOL_COPY_TEST_SHRED_COUNT: u64 = 25;
    const LEDGER_TOOL_COPY_TEST_ENDING_SLOT: u64 = LEDGER_TOOL_COPY_TEST_SHRED_COUNT + 1;
    insert_test_shreds(&ledger_path, LEDGER_TOOL_COPY_TEST_ENDING_SLOT);
    let ledger_path = ledger_path.to_str().unwrap();

    let target_ledger_path = get_tmp_ledger_path_auto_delete!();
    if dst_shred_compaction == "fifo" {
        let rocksdb_fifo_path = target_ledger_path
            .path()
            .join(ShredStorageType::rocks_fifo(None).blockstore_directory());
        fs::create_dir_all(rocksdb_fifo_path).unwrap();
    }
    let target_ledger_path = target_ledger_path.path().to_str().unwrap();
    let output = run_ledger_tool(&[
        "-l",
        ledger_path,
        "copy",
        "--target-db",
        target_ledger_path,
        "--ending-slot",
        &(LEDGER_TOOL_COPY_TEST_ENDING_SLOT).to_string(),
    ]);
    assert!(output.status.success());
    for slot_id in 0..LEDGER_TOOL_COPY_TEST_ENDING_SLOT {
        let src_slot_output = run_ledger_tool(&["-l", ledger_path, "slot", &slot_id.to_string()]);

        let dst_slot_output =
            run_ledger_tool(&["-l", target_ledger_path, "slot", &slot_id.to_string()]);
        assert!(src_slot_output.status.success());
        assert!(dst_slot_output.status.success());
        assert!(!src_slot_output.stdout.is_empty());
        assert_eq!(src_slot_output.stdout, dst_slot_output.stdout);
    }
}

#[test]
fn copy_test() {
    ledger_tool_copy_test("level", "level");
    ledger_tool_copy_test("level", "fifo");
    ledger_tool_copy_test("fifo", "level");
    ledger_tool_copy_test("fifo", "fifo");
}
