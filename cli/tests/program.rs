use serde_json::Value;
use solana_cli::{
    cli::{process_command, CliCommand, CliConfig},
    program::ProgramCliCommand,
};
use solana_client::rpc_client::RpcClient;
use solana_core::test_validator::TestValidator;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    bpf_loader,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{fs::File, io::Read, path::PathBuf, str::FromStr, sync::mpsc::channel};

#[test]
fn test_cli_program_deploy_non_upgradeable() {
    solana_logger::setup();

    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let mint_keypair = Keypair::new();
    let test_validator = TestValidator::with_no_fees(mint_keypair.pubkey());

    let (sender, receiver) = channel();
    run_local_faucet(mint_keypair, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new(test_validator.rpc_url());

    let mut file = File::open(pathbuf.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let minimum_balance_for_rent_exemption = rpc_client
        .get_minimum_balance_for_rent_exemption(program_data.len())
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 4 * minimum_balance_for_rent_exemption, // min balance for rent exemption for three programs + leftover for tx processing
    };
    process_command(&config).unwrap();

    config.command = CliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        address: None,
        use_deprecated_loader: false,
        allow_excessive_balance: false,
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
    let account0 = rpc_client
        .get_account_with_commitment(&program_id, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
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
        use_deprecated_loader: false,
        allow_excessive_balance: false,
    };
    process_command(&config).unwrap();
    let account1 = rpc_client
        .get_account_with_commitment(&custom_address_keypair.pubkey(), CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(account1.lamports, minimum_balance_for_rent_exemption);
    assert_eq!(account1.owner, bpf_loader::id());
    assert_eq!(account1.executable, true);
    assert_eq!(account1.data, account0.data);

    // Attempt to redeploy to the same address
    process_command(&config).unwrap_err();

    // Attempt to deploy to account with excess balance
    let custom_address_keypair = Keypair::new();
    config.signers = vec![&custom_address_keypair];
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 2 * minimum_balance_for_rent_exemption, // Anything over minimum_balance_for_rent_exemption should trigger err
    };
    process_command(&config).unwrap();
    config.signers = vec![&keypair, &custom_address_keypair];
    config.command = CliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        address: Some(1),
        use_deprecated_loader: false,
        allow_excessive_balance: false,
    };
    process_command(&config).unwrap_err();

    // Use forcing parameter to deploy to account with excess balance
    config.command = CliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        address: Some(1),
        use_deprecated_loader: false,
        allow_excessive_balance: true,
    };
    process_command(&config).unwrap();
    let account2 = rpc_client
        .get_account_with_commitment(&custom_address_keypair.pubkey(), CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(account2.lamports, 2 * minimum_balance_for_rent_exemption);
    assert_eq!(account2.owner, bpf_loader::id());
    assert_eq!(account2.executable, true);
    assert_eq!(account2.data, account0.data);
}

#[test]
fn test_cli_program_deploy_no_authority() {
    solana_logger::setup();

    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let mint_keypair = Keypair::new();
    let test_validator = TestValidator::with_no_fees(mint_keypair.pubkey());

    let (sender, receiver) = channel();
    run_local_faucet(mint_keypair, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new(test_validator.rpc_url());

    let mut file = File::open(pathbuf.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_programdata = rpc_client
        .get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(max_len).unwrap(),
        )
        .unwrap();
    let minimum_balance_for_program = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::program_len().unwrap())
        .unwrap();
    let upgrade_authority = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 100 * minimum_balance_for_programdata + minimum_balance_for_program,
    };
    config.signers = vec![&keypair];
    process_command(&config).unwrap();

    // Deploy a program with no authority
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: None,
        upgrade_authority_pubkey: None,
        max_len: None,
    });
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

    // Attempt to upgrade the program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        program_signer_index: None,
        program_pubkey: Some(program_id),
        buffer_signer_index: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: Some(1),
        upgrade_authority_pubkey: Some(upgrade_authority.pubkey()),
        max_len: None,
    });
    process_command(&config).unwrap_err();
}

#[test]
fn test_cli_program_deploy_with_authority() {
    solana_logger::setup();

    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let mint_keypair = Keypair::new();
    let test_validator = TestValidator::with_no_fees(mint_keypair.pubkey());

    let (sender, receiver) = channel();
    run_local_faucet(mint_keypair, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new(test_validator.rpc_url());

    let mut file = File::open(pathbuf.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_programdata = rpc_client
        .get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(max_len).unwrap(),
        )
        .unwrap();
    let minimum_balance_for_program = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::program_len().unwrap())
        .unwrap();
    let upgrade_authority = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 100 * minimum_balance_for_programdata + minimum_balance_for_program,
    };
    process_command(&config).unwrap();

    // Deploy the upgradeable program with specified program_id
    let program_keypair = Keypair::new();
    config.signers = vec![&keypair, &upgrade_authority, &program_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        program_signer_index: Some(2),
        program_pubkey: Some(program_keypair.pubkey()),
        buffer_signer_index: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: Some(1),
        upgrade_authority_pubkey: Some(upgrade_authority.pubkey()),
        max_len: Some(max_len),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_id_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        program_keypair.pubkey(),
        Pubkey::from_str(&program_id_str).unwrap()
    );
    let program_account = rpc_client
        .get_account_with_commitment(&program_keypair.pubkey(), CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let programdata_account = rpc_client
        .get_account_with_commitment(&programdata_pubkey, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(programdata_account.executable, false);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::programdata_data_offset().unwrap()..],
        program_data[..]
    );

    // Deploy the upgradeable program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: Some(1),
        upgrade_authority_pubkey: Some(upgrade_authority.pubkey()),
        max_len: Some(max_len),
    });
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
    let program_account = rpc_client
        .get_account_with_commitment(&program_id, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client
        .get_account_with_commitment(&programdata_pubkey, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(programdata_account.executable, false);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::programdata_data_offset().unwrap()..],
        program_data[..]
    );

    // Upgrade the program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        program_signer_index: None,
        program_pubkey: Some(program_id),
        buffer_signer_index: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: Some(1),
        upgrade_authority_pubkey: Some(upgrade_authority.pubkey()),
        max_len: Some(max_len),
    });
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
    let program_account = rpc_client
        .get_account_with_commitment(&program_id, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client
        .get_account_with_commitment(&programdata_pubkey, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(programdata_account.executable, false);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::programdata_data_offset().unwrap()..],
        program_data[..]
    );

    // Set a new authority
    let new_upgrade_authority = Keypair::new();
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
        program: program_id,
        upgrade_authority_index: Some(1),
        new_upgrade_authority: Some(new_upgrade_authority.pubkey()),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_upgrade_authority_str = json
        .as_object()
        .unwrap()
        .get("UpgradeAuthority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        Pubkey::from_str(&new_upgrade_authority_str).unwrap(),
        new_upgrade_authority.pubkey()
    );

    // Upgrade with new authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        program_signer_index: None,
        program_pubkey: Some(program_id),
        buffer_signer_index: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: Some(1),
        upgrade_authority_pubkey: Some(new_upgrade_authority.pubkey()),
        max_len: None,
    });
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
    let program_account = rpc_client
        .get_account_with_commitment(&program_id, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client
        .get_account_with_commitment(&programdata_pubkey, CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(programdata_account.executable, false);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::programdata_data_offset().unwrap()..],
        program_data[..]
    );

    // Set no authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
        program: program_id,
        upgrade_authority_index: Some(1),
        new_upgrade_authority: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_upgrade_authority_str = json
        .as_object()
        .unwrap()
        .get("UpgradeAuthority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(new_upgrade_authority_str, "None");

    // Upgrade with no authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: pathbuf.to_str().unwrap().to_string(),
        program_signer_index: None,
        program_pubkey: Some(program_id),
        buffer_signer_index: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: Some(1),
        upgrade_authority_pubkey: Some(new_upgrade_authority.pubkey()),
        max_len: None,
    });
    process_command(&config).unwrap_err();
}
