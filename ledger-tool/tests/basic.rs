use {
    assert_cmd::prelude::*,
    solana_ledger::{
        blockstore, blockstore::Blockstore, blockstore_options::ShredStorageType,
        create_new_tmp_ledger_auto_delete, create_new_tmp_ledger_fifo_auto_delete,
        genesis_utils::create_genesis_config, get_tmp_ledger_path_auto_delete,
    },
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

#[test]
fn bad_arguments() {
    // At least a ledger path is required
    assert!(!run_ledger_tool(&[]).status.success());

    // Invalid ledger path should fail
    assert!(!run_ledger_tool(&["-l", "invalid_ledger", "verify"])
        .status
        .success());
}

fn nominal_test_helper(ledger_path: &str) {
    let output = run_ledger_tool(&["-l", ledger_path, "verify"]);
    assert!(output.status.success());

    let output = run_ledger_tool(&["-l", ledger_path, "print", "-vv"]);
    assert!(output.status.success());
}

#[test]
fn nominal_default() {
    let genesis_config = create_genesis_config(100).genesis_config;
    let (ledger_path, _blockhash) = create_new_tmp_ledger_auto_delete!(&genesis_config);
    nominal_test_helper(ledger_path.path().to_str().unwrap());
}

#[test]
fn nominal_fifo() {
    let genesis_config = create_genesis_config(100).genesis_config;
    let (ledger_path, _blockhash) = create_new_tmp_ledger_fifo_auto_delete!(&genesis_config);
    nominal_test_helper(ledger_path.path().to_str().unwrap());
}

fn insert_test_shreds(ledger_path: &Path, ending_slot: u64) {
    let blockstore = Blockstore::open(ledger_path).unwrap();
    let (shreds, _) = blockstore::make_many_slot_entries(
        /*start_slot:*/ 0,
        ending_slot,
        /*entries_per_slot:*/ 10,
    );
    blockstore.insert_shreds(shreds, None, false).unwrap();
}

fn ledger_tool_copy_test(src_shred_compaction: &str, dst_shred_compaction: &str) {
    let genesis_config = create_genesis_config(100).genesis_config;

    let (ledger_path, _blockhash) = match src_shred_compaction {
        "fifo" => create_new_tmp_ledger_fifo_auto_delete!(&genesis_config),
        _ => create_new_tmp_ledger_auto_delete!(&genesis_config),
    };
    const LEDGER_TOOL_COPY_TEST_SHRED_COUNT: u64 = 25;
    const LEDGER_TOOL_COPY_TEST_ENDING_SLOT: u64 = LEDGER_TOOL_COPY_TEST_SHRED_COUNT + 1;
    insert_test_shreds(ledger_path.path(), LEDGER_TOOL_COPY_TEST_ENDING_SLOT);
    let ledger_path = ledger_path.path().to_str().unwrap();

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
    }
}

#[test]
fn copy_test() {
    ledger_tool_copy_test("level", "level");
    ledger_tool_copy_test("level", "fifo");
    ledger_tool_copy_test("fifo", "level");
    ledger_tool_copy_test("fifo", "fifo");
}
