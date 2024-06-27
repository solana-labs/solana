use {
    assert_cmd::prelude::*,
    solana_sdk::signer::keypair::{write_keypair_file, Keypair},
    std::process::Command,
    tempfile::TempDir,
};

#[test]
fn test_use_the_same_path_for_accounts_and_snapshots() {
    let temp_dir = TempDir::new().unwrap();
    let temp_dir_path = temp_dir.path();

    let id_json_path = temp_dir_path.join("id.json");
    let id_json_str = id_json_path.to_str().unwrap();

    let keypair = Keypair::new();
    write_keypair_file(&keypair, id_json_str).unwrap();

    let temp_dir_str = temp_dir_path.to_str().unwrap();

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.args([
        "--identity",
        id_json_str,
        "--log",
        "-",
        "--no-voting",
        "--accounts",
        temp_dir_str,
        "--snapshots",
        temp_dir_str,
    ]);
    cmd.assert().failure().stderr(predicates::str::contains(
        "The --accounts and --snapshots paths must be unique",
    ));
}
