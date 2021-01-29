use serde_json::Value;
use solana_cli::{
    cli::{process_command, CliCommand, CliConfig},
    program::ProgramCliCommand,
};
use solana_client::rpc_client::RpcClient;
use solana_core::test_validator::TestValidator;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    account_utils::StateMut,
    bpf_loader,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{fs::File, io::Read, path::PathBuf, str::FromStr};

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
    let faucet_addr = run_local_faucet(mint_keypair, None);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

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
        .get("ProgramId")
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
        use_deprecated_loader: false,
        allow_excessive_balance: false,
    };
    process_command(&config).unwrap();
    let account1 = rpc_client
        .get_account(&custom_address_keypair.pubkey())
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
        .get_account(&custom_address_keypair.pubkey())
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
    let faucet_addr = run_local_faucet(mint_keypair, None);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

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

    // Deploy a program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_id_str = json
        .as_object()
        .unwrap()
        .get("ProgramId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_id = Pubkey::from_str(&program_id_str).unwrap();

    // Attempt to upgrade the program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_id),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
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
    let faucet_addr = run_local_faucet(mint_keypair, None);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

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
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: Some(2),
        program_pubkey: Some(program_keypair.pubkey()),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_pubkey_str = json
        .as_object()
        .unwrap()
        .get("ProgramId")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        program_keypair.pubkey(),
        Pubkey::from_str(&program_pubkey_str).unwrap()
    );
    let program_account = rpc_client.get_account(&program_keypair.pubkey()).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
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
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_pubkey_str = json
        .as_object()
        .unwrap()
        .get("ProgramId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_pubkey = Pubkey::from_str(&program_pubkey_str).unwrap();
    let program_account = rpc_client.get_account(&program_pubkey).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
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
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_pubkey),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
    });
    process_command(&config).unwrap();
    let program_account = rpc_client.get_account(&program_pubkey).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
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
        program_pubkey,
        upgrade_authority_index: Some(1),
        new_upgrade_authority: Some(new_upgrade_authority.pubkey()),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_upgrade_authority_str = json
        .as_object()
        .unwrap()
        .get("Authority")
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
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_pubkey),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: None,
    });
    process_command(&config).unwrap();
    let program_account = rpc_client.get_account(&program_pubkey).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert_eq!(program_account.executable, true);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
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

    // Get upgrade authority
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::GetAuthority {
        account_pubkey: Some(program_pubkey),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Program upgrade authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        new_upgrade_authority.pubkey(),
        Pubkey::from_str(&authority_pubkey_str).unwrap()
    );

    // Set no authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
        program_pubkey,
        upgrade_authority_index: Some(1),
        new_upgrade_authority: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_upgrade_authority_str = json
        .as_object()
        .unwrap()
        .get("Authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(new_upgrade_authority_str, "None");

    // Upgrade with no authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_pubkey),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: None,
    });
    process_command(&config).unwrap_err();

    // deploy with finality
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_pubkey_str = json
        .as_object()
        .unwrap()
        .get("ProgramId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_pubkey = Pubkey::from_str(&program_pubkey_str).unwrap();
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    if let UpgradeableLoaderState::ProgramData {
        slot: _,
        upgrade_authority_address,
    } = programdata_account.state().unwrap()
    {
        assert_eq!(upgrade_authority_address, None);
    } else {
        panic!("not a buffer account");
    }

    // Get buffer authority
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::GetAuthority {
        account_pubkey: Some(program_pubkey),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Program upgrade authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!("None", authority_pubkey_str);
}

#[test]
fn test_cli_program_write_buffer() {
    solana_logger::setup();

    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let mint_keypair = Keypair::new();
    let test_validator = TestValidator::with_no_fees(mint_keypair.pubkey());
    let faucet_addr = run_local_faucet(mint_keypair, None);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(pathbuf.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(max_len).unwrap(),
        )
        .unwrap();
    let minimum_balance_for_buffer_default = rpc_client
        .get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(max_len * 2).unwrap(),
        )
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer with default params
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: pathbuf.to_str().unwrap().to_string(),
        buffer_signer_index: None,
        buffer_pubkey: None,
        buffer_authority_signer_index: None,
        max_len: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Buffer")
        .unwrap()
        .as_str()
        .unwrap();
    let new_buffer_pubkey = Pubkey::from_str(&buffer_pubkey_str).unwrap();
    let buffer_account = rpc_client.get_account(&new_buffer_pubkey).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer_default);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
        program_data[..]
    );

    // Specify buffer keypair and max_len
    let buffer_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: pathbuf.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: None,
        max_len: Some(max_len),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Buffer")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        buffer_keypair.pubkey(),
        Pubkey::from_str(&buffer_pubkey_str).unwrap()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
        program_data[..]
    );

    // Get buffer authority
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::GetAuthority {
        account_pubkey: Some(buffer_keypair.pubkey()),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Buffer authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        keypair.pubkey(),
        Pubkey::from_str(&authority_pubkey_str).unwrap()
    );

    // Specify buffer authority
    let buffer_keypair = Keypair::new();
    let authority_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &authority_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: pathbuf.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: Some(2),
        max_len: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Buffer")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        buffer_keypair.pubkey(),
        Pubkey::from_str(&buffer_pubkey_str).unwrap()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer_default);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(authority_keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
        program_data[..]
    );

    // Specify authority only
    let buffer_keypair = Keypair::new();
    let authority_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &authority_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: pathbuf.to_str().unwrap().to_string(),
        buffer_signer_index: None,
        buffer_pubkey: None,
        buffer_authority_signer_index: Some(2),
        max_len: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Buffer")
        .unwrap()
        .as_str()
        .unwrap();
    let buffer_pubkey = Pubkey::from_str(&buffer_pubkey_str).unwrap();
    let buffer_account = rpc_client.get_account(&buffer_pubkey).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer_default);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(authority_keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::buffer_data_offset().unwrap()..],
        program_data[..]
    );

    // Get buffer authority
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::GetAuthority {
        account_pubkey: Some(buffer_pubkey),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("Buffer authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        authority_keypair.pubkey(),
        Pubkey::from_str(&authority_pubkey_str).unwrap()
    );
}

#[test]
fn test_cli_program_set_buffer_authority() {
    solana_logger::setup();

    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let mint_keypair = Keypair::new();
    let test_validator = TestValidator::with_no_fees(mint_keypair.pubkey());
    let faucet_addr = run_local_faucet(mint_keypair, None);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(pathbuf.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(max_len).unwrap(),
        )
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer
    let buffer_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: pathbuf.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: None,
        max_len: None,
    });
    process_command(&config).unwrap();
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }

    // Set new authority
    let new_buffer_authority = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
        buffer_pubkey: buffer_keypair.pubkey(),
        buffer_authority_index: Some(0),
        new_buffer_authority: new_buffer_authority.pubkey(),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_buffer_authority_str = json
        .as_object()
        .unwrap()
        .get("Authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        Pubkey::from_str(&new_buffer_authority_str).unwrap(),
        new_buffer_authority.pubkey()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(new_buffer_authority.pubkey()));
    } else {
        panic!("not a buffer account");
    }

    // Set authority to buffer
    config.signers = vec![&keypair, &new_buffer_authority];
    config.command = CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
        buffer_pubkey: buffer_keypair.pubkey(),
        buffer_authority_index: Some(1),
        new_buffer_authority: buffer_keypair.pubkey(),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_authority_str = json
        .as_object()
        .unwrap()
        .get("Authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        Pubkey::from_str(&buffer_authority_str).unwrap(),
        buffer_keypair.pubkey()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(buffer_keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
}

#[test]
fn test_cli_program_mismatch_buffer_authority() {
    solana_logger::setup();

    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let mint_keypair = Keypair::new();
    let test_validator = TestValidator::with_no_fees(mint_keypair.pubkey());
    let faucet_addr = run_local_faucet(mint_keypair, None);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(pathbuf.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(
            UpgradeableLoaderState::programdata_len(max_len).unwrap(),
        )
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer
    let buffer_authority = Keypair::new();
    let buffer_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &buffer_authority];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: pathbuf.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: Some(2),
        max_len: None,
    });
    process_command(&config).unwrap();
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(buffer_authority.pubkey()));
    } else {
        panic!("not a buffer account");
    }

    // Attempt to deploy with mismatched authority
    let upgrade_authority = Keypair::new();
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
    });
    process_command(&config).unwrap_err();

    // Attempt to deploy matched authority
    config.signers = vec![&keypair, &buffer_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(pathbuf.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
    });
    process_command(&config).unwrap();
}
