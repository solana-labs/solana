use serde_json::Value;
use solana_cli::cli::{process_command, CliCommand, CliConfig};
use solana_client::rpc_client::RpcClient;
use solana_core::validator::TestValidator;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    bpf_loader,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{
    fs::{remove_dir_all, File},
    io::Read,
    path::PathBuf,
    str::FromStr,
    sync::mpsc::channel,
};

#[test]
fn test_cli_deploy_program() {
    solana_logger::setup();

    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run();

    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut file = File::open(pathbuf.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let minimum_balance_for_rent_exemption = rpc_client
        .get_minimum_balance_for_rent_exemption(program_data.len())
        .unwrap();

    let mut config = CliConfig::default();
    let keypair = Keypair::new();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 3 * minimum_balance_for_rent_exemption, // min balance for rent exemption for two programs + leftover for tx processing
    };
    config.signers = vec![&keypair];
    process_command(&config).unwrap();

    config.command = CliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        address: None,
    };

    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_id_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_id = Pubkey::from_str(&program_id_str).unwrap();
    let account0 = rpc_client.get_account(&program_id).unwrap();
    assert_eq!(account0.lamports, minimum_balance_for_rent_exemption);
    assert_eq!(account0.owner, bpf_loader::id());
    assert_eq!(account0.executable, true);

    let mut file = File::open(pathbuf.to_str().unwrap().to_string()).unwrap();
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();

    assert_eq!(account0.data, elf);

    // Test custom address
    let custom_address_keypair = Keypair::new();
    config.signers = vec![&keypair, &custom_address_keypair];
    config.command = CliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        address: Some(1),
    };
    process_command(&config).unwrap();
    let account1 = rpc_client
        .get_account(&custom_address_keypair.pubkey())
        .unwrap();
    assert_eq!(account1.lamports, minimum_balance_for_rent_exemption);
    assert_eq!(account1.owner, bpf_loader::id());
    assert_eq!(account1.executable, true);
    assert_eq!(account0.data, account1.data);

    // Attempt to redeploy to the same address
    process_command(&config).unwrap_err();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
